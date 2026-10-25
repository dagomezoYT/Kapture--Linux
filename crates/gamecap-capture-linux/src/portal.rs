//! xdg-desktop-portal ScreenCast + GStreamer `pipewiresrc` (SHM/BGRx fallback).

use std::io::ErrorKind;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use ashpd::desktop::screencast::{CursorMode as PortalCursor, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use gamecap_capture::{CaptureRequest, CaptureSession, VideoCaptureBackend};
use gamecap_types::{
    CaptureBackendKind, CaptureCaps, CaptureError, ColorPrimaries, ColorRange, ColorSpace,
    CursorMode, FrameStorage, MonotonicTimestamp, PixelFormat, SessionId, SourceTimestamp,
    TransferFunction, VideoFormat, VideoFrame,
};

use crate::av1_obu::take_complete_tus;
use crate::gst_pipe::{
    read_exact_timeout, spawn_gst_pipeline, spawn_gst_pipeline_ready, stop_child,
};
use crate::h264_au::take_complete_aus;
use crate::refresh::refresh_hz_for_request;

enum Event {
    Ready { width: u32, height: u32 },
    Frame(VideoFrame),
    Failed(CaptureError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GstEncodeKind {
    Raw,
    NvH264,
    VaAv1,
}

/// Linux M1 capture: portal picker, then GStreamer (GPU encode or BGRx fallback).
pub struct PortalPipeWireCapture {
    started: bool,
    stop: Arc<AtomicBool>,
    events: Option<Receiver<Event>>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Option<JoinHandle<()>>,
    format: Option<VideoFormat>,
    encode: GstEncodeKind,
}

impl Default for PortalPipeWireCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl PortalPipeWireCapture {
    pub fn new() -> Self {
        Self {
            started: false,
            stop: Arc::new(AtomicBool::new(false)),
            events: None,
            child: Arc::new(Mutex::new(None)),
            worker: None,
            format: None,
            encode: GstEncodeKind::Raw,
        }
    }

    /// Encode H.264 in GStreamer NVENC (CPU convert + NVIDIA).
    pub fn with_gst_nvenc(mut self) -> Self {
        self.encode = GstEncodeKind::NvH264;
        self
    }

    /// Encode AV1 on the AMD display GPU with DMA-BUF (no CPU copy).
    pub fn with_gst_va_av1(mut self) -> Self {
        self.encode = GstEncodeKind::VaAv1;
        self
    }
}

impl VideoCaptureBackend for PortalPipeWireCapture {
    fn capabilities(&self) -> CaptureCaps {
        CaptureCaps {
            backend: CaptureBackendKind::PipeWirePortal,
            monitors: Vec::new(),
            supports_window_capture: true,
            supports_hdr: false,
            pixel_formats: vec![PixelFormat::Bgra8, PixelFormat::Nv12],
            max_fps: None,
        }
    }

    fn start(&mut self, request: CaptureRequest) -> Result<CaptureSession, CaptureError> {
        if self.started {
            return Err(CaptureError::InvalidState(
                "capture already started".to_string(),
            ));
        }
        self.stop.store(false, Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(64);
        let stop = Arc::clone(&self.stop);
        let child_slot = Arc::clone(&self.child);
        let cursor = request.cursor;
        let refresh_hz = refresh_hz_for_request(request.frame_rate_mode);
        let encode = self.encode;
        let handle = thread::Builder::new()
            .name("gamecap-portal-gst".to_string())
            .spawn(move || {
                if let Err(err) = run_portal_capture(
                    cursor,
                    refresh_hz,
                    encode,
                    tx.clone(),
                    stop,
                    child_slot,
                ) {
                    let _ = tx.send(Event::Failed(err));
                }
            })
            .map_err(|e| CaptureError::Backend(e.to_string()))?;
        self.worker = Some(handle);
        self.events = Some(rx);
        let events = match self.events.as_ref() {
            Some(events) => events,
            None => {
                return Err(CaptureError::InvalidState(
                    "missing capture event channel".to_string(),
                ))
            }
        };
        match events.recv() {
            Ok(Event::Ready { width, height }) => {
                let mut format = request.format.clone();
                format.width = width;
                format.height = height;
                format.pixel_format = PixelFormat::Bgra8;
                self.format = Some(format.clone());
                self.started = true;
                Ok(CaptureSession {
                    id: SessionId::new(),
                    format,
                })
            }
            Ok(Event::Failed(err)) => Err(err),
            Ok(Event::Frame(_)) => Err(CaptureError::InvalidState(
                "unexpected frame before portal ready".to_string(),
            )),
            Err(_) => Err(CaptureError::Backend(
                "portal capture thread ended before ready".to_string(),
            )),
        }
    }

    fn poll_frame(&mut self, deadline: Instant) -> Result<Option<VideoFrame>, CaptureError> {
        if !self.started {
            return Err(CaptureError::InvalidState(
                "capture not started".to_string(),
            ));
        }
        let rx = match self.events.as_mut() {
            Some(rx) => rx,
            None => {
                return Err(CaptureError::InvalidState(
                    "missing capture event channel".to_string(),
                ))
            }
        };
        let timeout = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(Event::Frame(frame)) => Ok(Some(frame)),
            Ok(Event::Failed(err)) => Err(err),
            Ok(Event::Ready { .. }) => Ok(None),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(CaptureError::DeviceLost(
                "portal/GStreamer capture ended".to_string(),
            )),
        }
    }

    fn reconfigure(&mut self, _request: CaptureRequest) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedCombination(
            gamecap_types::UnsupportedCombination::new(
                "reconfigure",
                "Linux M1 capture does not reconfigure mid-session",
            ),
        ))
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.stop.store(true, Ordering::Release);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                stop_child(&mut child);
            }
        }
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.events = None;
        self.started = false;
        Ok(())
    }
}

fn run_portal_capture(
    cursor: CursorMode,
    refresh_hz: u32,
    encode: GstEncodeKind,
    tx: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<Child>>>,
) -> Result<(), CaptureError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
    rt.block_on(async move {
        let proxy = Screencast::new()
            .await
            .map_err(|e| map_portal_err(e, "Screencast proxy"))?;
        let session = proxy
            .create_session()
            .await
            .map_err(|e| map_portal_err(e, "create_session"))?;
        let portal_cursor = match cursor {
            CursorMode::Embedded => PortalCursor::Embedded,
            CursorMode::Metadata => PortalCursor::Metadata,
            CursorMode::Exclude => PortalCursor::Hidden,
        };
        proxy
            .select_sources(
                &session,
                portal_cursor,
                SourceType::Monitor | SourceType::Window,
                false,
                None,
                PersistMode::DoNot,
            )
            .await
            .map_err(|e| map_portal_err(e, "select_sources"))?;
        let response = proxy
            .start(&session, None)
            .await
            .map_err(|e| map_portal_err(e, "start"))?
            .response()
            .map_err(|e| CaptureError::PermissionDenied(format!("screen share declined: {e}")))?;
        let stream = response
            .streams()
            .first()
            .ok_or_else(|| CaptureError::Backend("portal returned no streams".to_string()))?;
        let node_id = stream.pipe_wire_node_id();
        let (width, height) = portal_size(stream.size());
        if width == 0 || height == 0 {
            return Err(CaptureError::Backend(
                "portal stream has invalid size".to_string(),
            ));
        }
        let fd = proxy
            .open_pipe_wire_remote(&session)
            .await
            .map_err(|e| map_portal_err(e, "open_pipe_wire_remote"))?;
        if tx.send(Event::Ready { width, height }).is_err() {
            return Ok(());
        }
        let result = tokio::task::spawn_blocking(move || {
            gst_frame_loop(
                fd,
                node_id,
                width,
                height,
                refresh_hz,
                encode,
                tx,
                stop,
                child_slot,
            )
        })
        .await
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
        drop(session);
        result
    })
}

fn portal_size(size: impl Into<SizeHint>) -> (u32, u32) {
    size.into().even()
}

struct SizeHint {
    width: i32,
    height: i32,
}

impl SizeHint {
    fn even(self) -> (u32, u32) {
        (
            even_size(self.width.max(0) as u32),
            even_size(self.height.max(0) as u32),
        )
    }
}

impl From<(i32, i32)> for SizeHint {
    fn from(value: (i32, i32)) -> Self {
        Self {
            width: value.0,
            height: value.1,
        }
    }
}

impl From<(u32, u32)> for SizeHint {
    fn from(value: (u32, u32)) -> Self {
        Self {
            width: value.0 as i32,
            height: value.1 as i32,
        }
    }
}

impl From<Option<(i32, i32)>> for SizeHint {
    fn from(value: Option<(i32, i32)>) -> Self {
        match value {
            Some(v) => v.into(),
            None => Self {
                width: 1920,
                height: 1080,
            },
        }
    }
}

impl From<Option<(u32, u32)>> for SizeHint {
    fn from(value: Option<(u32, u32)>) -> Self {
        match value {
            Some(v) => v.into(),
            None => Self {
                width: 1920,
                height: 1080,
            },
        }
    }
}

fn gst_frame_loop(
    fd: OwnedFd,
    node_id: u32,
    width: u32,
    height: u32,
    refresh_hz: u32,
    encode: GstEncodeKind,
    tx: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<Child>>>,
) -> Result<(), CaptureError> {
    rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::empty())
        .map_err(|e| CaptureError::Backend(format!("fcntl: {e}")))?;
    let portal_raw = fd.as_raw_fd();
    let frame_len = PixelFormat::Bgra8
        .buffer_size(width, height)
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
    if frame_len == 0 {
        return Err(CaptureError::Backend("invalid frame size".to_string()));
    }
    let inherit = [fd.as_fd()];
    let pipe = match encode {
        GstEncodeKind::VaAv1 => spawn_va_av1_pipeline(portal_raw, node_id, refresh_hz, &inherit)?,
        GstEncodeKind::NvH264 => {
            spawn_encoded_pipeline(portal_raw, node_id, width, height, refresh_hz, &inherit)?
        }
        GstEncodeKind::Raw => spawn_gst_pipeline(
            &raw_bgrx_tokens(portal_raw, node_id, width, height, frame_len),
            &inherit,
        )
        .map_err(CaptureError::Backend)?,
    };
    let prefix = pipe.prefix;
    let gst_err = std::sync::Arc::clone(&pipe.stderr);
    let mut reader = pipe.reader;
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(pipe.child);
    }
    let origin = Instant::now();
    let mut frame_id = 0u64;
    let mut last_pts_ns = 0u64;
    let pump = match encode {
        GstEncodeKind::VaAv1 => pump_av1_frames(
            &mut reader,
            prefix,
            width,
            height,
            origin,
            refresh_hz,
            &mut last_pts_ns,
            &mut frame_id,
            &tx,
            &stop,
        ),
        GstEncodeKind::NvH264 => pump_encoded_frames(
            &mut reader,
            prefix,
            width,
            height,
            origin,
            refresh_hz,
            &mut last_pts_ns,
            &mut frame_id,
            &tx,
            &stop,
        ),
        GstEncodeKind::Raw => pump_raw_frames(
            &mut reader,
            frame_len,
            width,
            height,
            origin,
            &mut frame_id,
            &tx,
            &stop,
        ),
    };
    let stopped = stop.load(Ordering::Acquire);
    let gst_msg = gst_err
        .lock()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    if let Ok(mut slot) = child_slot.lock() {
        if let Some(mut child) = slot.take() {
            stop_child(&mut child);
        }
    }
    drop(fd);
    pump?;
    if !stopped {
        let detail = if gst_msg.is_empty() {
            String::new()
        } else {
            format!(" {gst_msg}")
        };
        if frame_id == 0 {
            return Err(CaptureError::Backend(format!(
                "GStreamer no produjo fotogramas.{detail}"
            )));
        }
        return Err(CaptureError::DeviceLost(format!(
            "portal/GStreamer capture ended after {frame_id} frames.{detail}"
        )));
    }
    Ok(())
}

fn spawn_va_av1_pipeline(
    portal_raw: i32,
    node_id: u32,
    refresh_hz: u32,
    inherit: &[std::os::fd::BorrowedFd<'_>],
) -> Result<crate::gst_pipe::GstRawPipe, CaptureError> {
    let wait = std::time::Duration::from_secs(2);
    let high_rate = va_av1_tokens(portal_raw, node_id, refresh_hz, false, true);
    match spawn_gst_pipeline_ready(&high_rate, inherit, wait) {
        Ok(pipe) => Ok(pipe),
        Err(_) => {
            let quality = va_av1_tokens(portal_raw, node_id, refresh_hz, false, false);
            match spawn_gst_pipeline_ready(&quality, inherit, wait) {
                Ok(pipe) => Ok(pipe),
                Err(first) => {
                    let low_latency = va_av1_tokens(portal_raw, node_id, refresh_hz, true, false);
                    spawn_gst_pipeline_ready(&low_latency, inherit, wait).map_err(|second| {
                        CaptureError::Backend(format!("AV1: {first}; low-latency: {second}"))
                    })
                }
            }
        }
    }
}

fn va_av1_tokens(
    portal_raw: i32,
    node_id: u32,
    refresh_hz: u32,
    low_latency: bool,
    request_panel_hz: bool,
) -> Vec<String> {
    let hz = refresh_hz.clamp(24, 360);
    let gop = hz.saturating_mul(2).max(48).min(1024);
    let mut tokens = vec![
        "pipewiresrc".to_string(),
        format!("fd={portal_raw}"),
        format!("path={node_id}"),
        "do-timestamp=true".to_string(),
        "keepalive-time=0".to_string(),
        "provide-clock=false".to_string(),
        "min-buffers=8".to_string(),
        "max-buffers=32".to_string(),
        "client-name=GameCap".to_string(),
    ];
    if request_panel_hz {
        tokens.push(format!(
            "stream-properties=\"props,node.latency=(fraction)1/{hz}\""
        ));
    }
    tokens.push("!".to_string());
    if !low_latency {
        tokens.extend([
            "queue".to_string(),
            "max-size-buffers=8".to_string(),
            "max-size-bytes=0".to_string(),
            "max-size-time=0".to_string(),
            "!".to_string(),
        ]);
    }
    tokens.extend([
        "vapostproc".to_string(),
        "qos=false".to_string(),
        "!".to_string(),
    ]);
    tokens.push("vaav1enc".to_string());
    if low_latency {
        tokens.extend([
            "rate-control=cqp".to_string(),
            "qp=72".to_string(),
            "target-usage=2".to_string(),
            format!("key-int-max={gop}"),
            "hierarchical-level=1".to_string(),
            "gf-group-size=1".to_string(),
            "ref-frames=2".to_string(),
            "qos=false".to_string(),
        ]);
    } else {
        tokens.extend([
            "rate-control=qvbr".to_string(),
            "qp=80".to_string(),
            "target-usage=4".to_string(),
            format!("key-int-max={gop}"),
            "qos=false".to_string(),
            "palette-mode=true".to_string(),
        ]);
    }
    tokens.extend([
        "!".to_string(),
        "video/x-av1,stream-format=obu-stream,alignment=tu".to_string(),
        "!".to_string(),
    ]);
    if low_latency {
        tokens.extend([
            "queue".to_string(),
            "max-size-buffers=24".to_string(),
            "max-size-bytes=0".to_string(),
            "max-size-time=0".to_string(),
            "leaky=downstream".to_string(),
            "!".to_string(),
        ]);
    }
    tokens.extend([
        "fdsink".to_string(),
        "fd=$SINK".to_string(),
        "blocksize=1048576".to_string(),
        "sync=false".to_string(),
        "qos=false".to_string(),
    ]);
    tokens
}

fn spawn_encoded_pipeline(
    portal_raw: i32,
    node_id: u32,
    width: u32,
    height: u32,
    refresh_hz: u32,
    inherit: &[std::os::fd::BorrowedFd<'_>],
) -> Result<crate::gst_pipe::GstRawPipe, CaptureError> {
    let y444 = nvenc_tokens(portal_raw, node_id, width, height, refresh_hz, "Y444");
    match spawn_gst_pipeline(&y444, inherit) {
        Ok(pipe) => Ok(pipe),
        Err(_) => {
            let bgrx = nvenc_tokens(portal_raw, node_id, width, height, refresh_hz, "BGRx");
            spawn_gst_pipeline(&bgrx, inherit).map_err(CaptureError::Backend)
        }
    }
}

fn nvenc_tokens(
    portal_raw: i32,
    node_id: u32,
    width: u32,
    height: u32,
    refresh_hz: u32,
    format: &str,
) -> Vec<String> {
    let gop = refresh_hz.saturating_mul(2).max(48);
    vec![
        "pipewiresrc".to_string(),
        format!("fd={portal_raw}"),
        format!("path={node_id}"),
        "do-timestamp=true".to_string(),
        "keepalive-time=0".to_string(),
        "min-buffers=8".to_string(),
        "max-buffers=32".to_string(),
        "client-name=GameCap".to_string(),
        "!".to_string(),
        "queue".to_string(),
        "max-size-buffers=8".to_string(),
        "max-size-bytes=0".to_string(),
        "max-size-time=0".to_string(),
        "!".to_string(),
        "videoconvert".to_string(),
        "n-threads=8".to_string(),
        "dither=none".to_string(),
        "qos=false".to_string(),
        "!".to_string(),
        format!("video/x-raw,format={format},width={width},height={height}"),
        "!".to_string(),
        "cudaupload".to_string(),
        "!".to_string(),
        "nvh264enc".to_string(),
        "preset=p4".to_string(),
        "tune=low-latency".to_string(),
        "bframes=0".to_string(),
        "zerolatency=true".to_string(),
        format!("gop-size={gop}"),
        "rc-mode=vbr".to_string(),
        "const-quality=16".to_string(),
        "qos=false".to_string(),
        "repeat-sequence-header=true".to_string(),
        "aud=true".to_string(),
        "!".to_string(),
        "video/x-h264,stream-format=byte-stream,alignment=au".to_string(),
        "!".to_string(),
        "fdsink".to_string(),
        "fd=$SINK".to_string(),
        "blocksize=1048576".to_string(),
        "sync=false".to_string(),
        "qos=false".to_string(),
    ]
}

fn raw_bgrx_tokens(
    portal_raw: i32,
    node_id: u32,
    width: u32,
    height: u32,
    frame_len: usize,
) -> Vec<String> {
    vec![
        "pipewiresrc".to_string(),
        format!("fd={portal_raw}"),
        format!("path={node_id}"),
        "do-timestamp=true".to_string(),
        "keepalive-time=0".to_string(),
        "min-buffers=8".to_string(),
        "max-buffers=32".to_string(),
        "client-name=GameCap".to_string(),
        "!".to_string(),
        "queue".to_string(),
        "max-size-buffers=8".to_string(),
        "max-size-bytes=0".to_string(),
        "max-size-time=0".to_string(),
        "!".to_string(),
        "videoconvert".to_string(),
        "n-threads=8".to_string(),
        "dither=none".to_string(),
        "qos=false".to_string(),
        "!".to_string(),
        format!("video/x-raw,format=BGRx,width={width},height={height}"),
        "!".to_string(),
        "fdsink".to_string(),
        "fd=$SINK".to_string(),
        format!("blocksize={frame_len}"),
        "sync=false".to_string(),
        "qos=false".to_string(),
    ]
}

fn pump_encoded_frames(
    reader: &mut std::os::unix::net::UnixStream,
    mut leftover: Vec<u8>,
    width: u32,
    height: u32,
    origin: Instant,
    refresh_hz: u32,
    last_pts_ns: &mut u64,
    frame_id: &mut u64,
    tx: &SyncSender<Event>,
    stop: &Arc<AtomicBool>,
) -> Result<(), CaptureError> {
    use std::io::Read;
    let mut chunk = vec![0u8; 256 * 1024];
    while !stop.load(Ordering::Acquire) {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                leftover.extend_from_slice(&chunk[..n]);
                for au in take_complete_aus(&mut leftover) {
                    let ts = stamp_video_pts(origin, last_pts_ns, refresh_hz);
                    let frame = encoded_frame(*frame_id, ts, width, height, au);
                    *frame_id = frame_id.saturating_add(1);
                    if tx.send(Event::Frame(frame)).is_err() {
                        return Ok(());
                    }
                }
            }
            Err(err)
                if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                return Err(CaptureError::Backend(format!("gstreamer read: {err}")));
            }
        }
    }
    Ok(())
}

fn pump_av1_frames(
    reader: &mut std::os::unix::net::UnixStream,
    mut leftover: Vec<u8>,
    width: u32,
    height: u32,
    origin: Instant,
    refresh_hz: u32,
    last_pts_ns: &mut u64,
    frame_id: &mut u64,
    tx: &SyncSender<Event>,
    stop: &Arc<AtomicBool>,
) -> Result<(), CaptureError> {
    use std::io::Read;
    let mut chunk = vec![0u8; 256 * 1024];
    while !stop.load(Ordering::Acquire) {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                leftover.extend_from_slice(&chunk[..n]);
                for tu in take_complete_tus(&mut leftover) {
                    let ts = stamp_video_pts(origin, last_pts_ns, refresh_hz);
                    let frame = encoded_frame(*frame_id, ts, width, height, tu);
                    *frame_id = frame_id.saturating_add(1);
                    if tx.send(Event::Frame(frame)).is_err() {
                        return Ok(());
                    }
                }
            }
            Err(err)
                if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                return Err(CaptureError::Backend(format!("gstreamer read: {err}")));
            }
        }
    }
    Ok(())
}

fn pump_raw_frames(
    reader: &mut std::os::unix::net::UnixStream,
    frame_len: usize,
    width: u32,
    height: u32,
    origin: Instant,
    frame_id: &mut u64,
    tx: &SyncSender<Event>,
    stop: &Arc<AtomicBool>,
) -> Result<(), CaptureError> {
    while !stop.load(Ordering::Acquire) {
        let mut buf = vec![0u8; frame_len];
        match read_exact_timeout(reader, &mut buf) {
            Ok(()) => {
                let ts = MonotonicTimestamp::from_duration(origin.elapsed());
                let frame = VideoFrame {
                    frame_id: *frame_id,
                    capture_ts: ts,
                    source_ts: Some(SourceTimestamp::from_master(ts)),
                    width,
                    height,
                    pixel_format: PixelFormat::Bgra8,
                    color_space: ColorSpace::Bt709,
                    transfer: TransferFunction::Bt709,
                    primaries: ColorPrimaries::Bt709,
                    range: ColorRange::Full,
                    hdr_metadata: None,
                    storage: FrameStorage::Cpu {
                        bytes: std::sync::Arc::from(buf),
                        stride: width as usize * 4,
                    },
                };
                *frame_id = frame_id.saturating_add(1);
                if tx.send(Event::Frame(frame)).is_err() {
                    break;
                }
            }
            Err(err)
                if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                return Err(CaptureError::Backend(format!("gstreamer read: {err}")));
            }
        }
    }
    Ok(())
}

fn encoded_frame(
    frame_id: u64,
    ts: MonotonicTimestamp,
    width: u32,
    height: u32,
    au: Vec<u8>,
) -> VideoFrame {
    VideoFrame {
        frame_id,
        capture_ts: ts,
        source_ts: Some(SourceTimestamp::from_master(ts)),
        width,
        height,
        pixel_format: PixelFormat::Bgra8,
        color_space: ColorSpace::Bt709,
        transfer: TransferFunction::Bt709,
        primaries: ColorPrimaries::Bt709,
        range: ColorRange::Full,
        hdr_metadata: None,
        storage: FrameStorage::Cpu {
            bytes: std::sync::Arc::from(au),
            stride: 1,
        },
    }
}

fn even_size(v: u32) -> u32 {
    v & !1
}

fn stamp_video_pts(origin: Instant, last_ns: &mut u64, refresh_hz: u32) -> MonotonicTimestamp {
    let now = origin.elapsed().as_nanos() as u64;
    let step = 1_000_000_000 / u64::from(refresh_hz.max(1));
    let ts = now.max(last_ns.saturating_add(step));
    *last_ns = ts;
    MonotonicTimestamp::from_nanos(ts)
}

fn map_portal_err(err: ashpd::Error, ctx: &str) -> CaptureError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("cancel") || lower.contains("denied") || lower.contains("dismiss") {
        CaptureError::PermissionDenied(format!("{ctx}: {msg}"))
    } else {
        CaptureError::Backend(format!("{ctx}: {msg}"))
    }
}
