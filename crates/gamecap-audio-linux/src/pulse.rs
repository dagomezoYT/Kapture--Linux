//! PipeWire/Pulse system audio via GStreamer `pulsesrc`.

use std::io::ErrorKind;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use gamecap_audio::{AudioCaptureBackend, AudioRequest, AudioSession, DEFAULT_BLOCK_FRAMES};
use gamecap_types::{
    media_pts_samples, AudioBlock, AudioError, AudioFormat, AudioTrackKind, INTERNAL_SAMPLE_RATE_HZ,
};

use crate::gst_pipe::{read_exact_timeout, spawn_gst_pipeline, stop_child};

enum Event {
    Ready,
    Block(AudioBlock),
    Failed(AudioError),
}

/// Captures the default Pulse/PipeWire monitor (system audio).
pub struct PulseMonitorCapture {
    started: bool,
    stop: Arc<AtomicBool>,
    events: Option<Receiver<Event>>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Option<JoinHandle<()>>,
    format: AudioFormat,
}

impl Default for PulseMonitorCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl PulseMonitorCapture {
    pub fn new() -> Self {
        Self {
            started: false,
            stop: Arc::new(AtomicBool::new(false)),
            events: None,
            child: Arc::new(Mutex::new(None)),
            worker: None,
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
        }
    }
}

impl AudioCaptureBackend for PulseMonitorCapture {
    fn start(&mut self, request: AudioRequest) -> Result<AudioSession, AudioError> {
        if self.started {
            return Err(AudioError::InvalidState(
                "audio capture already started".to_string(),
            ));
        }
        let device = match default_monitor_source() {
            Some(d) => d,
            None => {
                return Err(AudioError::Backend(
                    "no Pulse/PipeWire monitor source (system audio) found".to_string(),
                ))
            }
        };
        self.format = request.format;
        self.stop.store(false, Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(16);
        let stop = Arc::clone(&self.stop);
        let child_slot = Arc::clone(&self.child);
        let format = self.format;
        let handle = thread::Builder::new()
            .name("gamecap-pulse-gst".to_string())
            .spawn(move || {
                if let Err(err) = run_pulse_capture(device, format, tx.clone(), stop, child_slot) {
                    let _ = tx.send(Event::Failed(err));
                }
            })
            .map_err(|e| AudioError::Backend(e.to_string()))?;
        self.worker = Some(handle);
        self.events = Some(rx);
        match self.events.as_ref() {
            Some(events) => match events.recv() {
                Ok(Event::Ready) => {
                    self.started = true;
                    Ok(AudioSession {
                        format: self.format,
                        tracks: vec![AudioTrackKind::System],
                    })
                }
                Ok(Event::Failed(err)) => Err(err),
                Ok(Event::Block(_)) => Err(AudioError::InvalidState(
                    "unexpected audio before ready".to_string(),
                )),
                Err(_) => Err(AudioError::Backend(
                    "audio thread ended before ready".to_string(),
                )),
            },
            None => Err(AudioError::InvalidState(
                "missing audio event channel".to_string(),
            )),
        }
    }

    fn poll_audio(&mut self, deadline: Instant) -> Result<Option<AudioBlock>, AudioError> {
        if !self.started {
            return Err(AudioError::InvalidState(
                "audio capture not started".to_string(),
            ));
        }
        let rx = match self.events.as_mut() {
            Some(rx) => rx,
            None => {
                return Err(AudioError::InvalidState(
                    "missing audio event channel".to_string(),
                ))
            }
        };
        let timeout = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(Event::Block(block)) => Ok(Some(block)),
            Ok(Event::Failed(err)) => Err(err),
            Ok(Event::Ready) => Ok(None),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => {
                Err(AudioError::DeviceLost("system audio capture ended".to_string()))
            }
        }
    }

    fn stop(&mut self) -> Result<(), AudioError> {
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

fn run_pulse_capture(
    device: String,
    format: AudioFormat,
    tx: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<Child>>>,
) -> Result<(), AudioError> {
    let channels = format.channels.get();
    let rate = format.sample_rate.hz();
    let tokens = vec![
        "pulsesrc".to_string(),
        format!("device={device}"),
        "!".to_string(),
        "audioconvert".to_string(),
        "!".to_string(),
        "audioresample".to_string(),
        "!".to_string(),
        format!("audio/x-raw,format=F32LE,channels={channels},rate={rate},layout=interleaved"),
        "!".to_string(),
        "fdsink".to_string(),
        "fd=$SINK".to_string(),
        "sync=false".to_string(),
    ];
    let pipe = spawn_gst_pipeline(&tokens).map_err(AudioError::Backend)?;
    let mut reader = pipe.reader;
    if let Ok(mut slot) = child_slot.lock() {
        *slot = Some(pipe.child);
    }
    if tx.send(Event::Ready).is_err() {
        return Ok(());
    }
    let frames = DEFAULT_BLOCK_FRAMES;
    let sample_bytes = (channels as usize).saturating_mul(4).saturating_mul(frames);
    let mut block_id = 0u64;
    let origin_samples = 0u64;
    while !stop.load(Ordering::Acquire) {
        let mut buf = vec![0u8; sample_bytes];
        match read_exact_timeout(&mut reader, &mut buf) {
            Ok(()) => {
                let mut samples = Vec::with_capacity(frames.saturating_mul(channels as usize));
                for chunk in buf.chunks_exact(4) {
                    let bytes = [chunk[0], chunk[1], chunk[2], chunk[3]];
                    samples.push(f32::from_le_bytes(bytes));
                }
                let capture_ts = media_pts_samples(
                    origin_samples.saturating_add(block_id.saturating_mul(frames as u64)),
                    INTERNAL_SAMPLE_RATE_HZ,
                );
                let block = AudioBlock {
                    block_id,
                    capture_ts,
                    track_id: AudioTrackKind::System.default_id(),
                    kind: AudioTrackKind::System,
                    format,
                    samples: std::sync::Arc::from(samples),
                };
                block_id = block_id.saturating_add(1);
                if tx.send(Event::Block(block)).is_err() {
                    break;
                }
            }
            Err(err) if err.kind() == ErrorKind::TimedOut || err.kind() == ErrorKind::WouldBlock => {
                continue;
            }
            Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
            Err(err) => {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                return Err(AudioError::Backend(format!("gstreamer audio read: {err}")));
            }
        }
    }
    if let Ok(mut slot) = child_slot.lock() {
        if let Some(mut child) = slot.take() {
            stop_child(&mut child);
        }
    }
    Ok(())
}

fn default_monitor_source() -> Option<String> {
    if let Some(sink) = pactl_output(&["get-default-sink"]) {
        let monitor = format!("{sink}.monitor");
        if source_exists(&monitor) {
            return Some(monitor);
        }
    }
    let listed = pactl_output(&["list", "short", "sources"]).unwrap_or_default();
    for line in listed.lines() {
        let name = line.split('\t').nth(1).unwrap_or("");
        if name.ends_with(".monitor") {
            return Some(name.to_string());
        }
    }
    None
}

fn source_exists(name: &str) -> bool {
    let listed = pactl_output(&["list", "short", "sources"]).unwrap_or_default();
    listed.lines().any(|line| line.split('\t').nth(1) == Some(name))
}

fn pactl_output(args: &[&str]) -> Option<String> {
    let out = Command::new("pactl").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
