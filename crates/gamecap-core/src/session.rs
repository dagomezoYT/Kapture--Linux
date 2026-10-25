//! Recorder session: threads, bounded queues, safety governor, state machine.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gamecap_audio::{AudioCaptureBackend, AudioEncoder, AudioRequest, NullAudioEncoder};
use gamecap_capture::{CaptureRequest, VideoCaptureBackend};
use gamecap_config::SessionConfig;
use gamecap_encoder::{EncoderConfig, VideoEncoder};
use gamecap_hardware::{assert_user_write_path, resolve_probe_path, OsStorageProbe, OsTelemetry};
use gamecap_metrics::{MetricsCollector, SessionMetrics, SessionSummary};
use gamecap_mux::{ContainerKind, Muxer, MuxerConfig, MuxerOutput};
use gamecap_types::{
    AudioFormat, AudioTrackKind, CodecId, DegradedReason, Diagnostics, DropReason, EncodedPacket,
    GpuFrameRef, HostTelemetry, QualityProfile, ResourceSnapshot, SafetyAction, SafetyStopReason,
    SessionId, SessionState, StorageProbe,
};

use crate::clock::MasterClock;
use crate::error::CoreError;
use crate::governor::{evaluate, SaturationStreak};
use crate::memory::{audio_block_bytes, video_frame_bytes, MemoryTracker};
use crate::policy::{offer_from_capture, OfferOutcome};
use crate::queue::{bounded, BoundedSender, RecvTimeoutError, TrySendError};
use crate::state_machine::StateMachine;

enum PipelinePacket {
    Video(EncodedPacket),
    Audio {
        track: gamecap_types::AudioTrackId,
        packet: EncodedPacket,
    },
}

struct Running {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<Result<(), CoreError>>>,
    mux_output: Arc<Mutex<Option<MuxerOutput>>>,
    safety_reason: Arc<Mutex<Option<SafetyStopReason>>>,
    degraded: Arc<Mutex<Option<DegradedReason>>>,
}

/// Orchestrates capture → encode → mux without owning OS types.
pub struct RecorderSession {
    config: SessionConfig,
    clock: MasterClock,
    state: StateMachine,
    metrics: Arc<MetricsCollector>,
    video: Option<Box<dyn VideoCaptureBackend>>,
    audio: Option<Box<dyn AudioCaptureBackend>>,
    encoder: Option<Box<dyn VideoEncoder>>,
    audio_encoder: Option<Box<dyn AudioEncoder>>,
    muxer: Option<Box<dyn Muxer>>,
    video_request: Option<CaptureRequest>,
    audio_request: Option<AudioRequest>,
    storage: Arc<dyn StorageProbe>,
    telemetry: Arc<dyn HostTelemetry>,
    running: Option<Running>,
}

#[derive(Clone, Debug)]
pub struct SessionOutcome {
    pub state: SessionState,
    pub metrics: SessionMetrics,
    pub summary: SessionSummary,
    pub mux: MuxerOutput,
    pub sidecar_path: Option<PathBuf>,
    pub safety_stop: Option<SafetyStopReason>,
    pub degraded: Option<DegradedReason>,
}

impl RecorderSession {
    pub fn builder(config: SessionConfig) -> RecorderBuilder {
        RecorderBuilder {
            config,
            video: None,
            audio: None,
            encoder: None,
            audio_encoder: None,
            muxer: None,
            video_request: None,
            audio_request: None,
            storage: None,
            telemetry: None,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state.current()
    }

    pub fn session_id(&self) -> SessionId {
        self.config.session_id
    }

    pub fn metrics_snapshot(&self) -> SessionMetrics {
        self.metrics.snapshot()
    }

    pub fn start(&mut self) -> Result<(), CoreError> {
        match self.state.current() {
            SessionState::Idle => {
                self.state.transition(SessionState::Probing)?;
                self.preflight_storage()?;
                self.state.transition(SessionState::Ready)?;
            }
            SessionState::Ready => {
                self.preflight_storage()?;
            }
            other => {
                return Err(CoreError::InvalidTransition {
                    from: other.to_string(),
                    to: SessionState::Starting.to_string(),
                });
            }
        }
        self.state.transition(SessionState::Starting)?;
        self.spawn_workers()?;
        self.state.transition(SessionState::Recording)?;
        Ok(())
    }

    /// Blocks until synthetic backends complete, the governor stops, or an error fails the session.
    pub fn wait(&mut self) -> Result<SessionOutcome, CoreError> {
        self.join_workers(false)
    }

    pub fn stop(&mut self) -> Result<SessionOutcome, CoreError> {
        if let Some(running) = &self.running {
            running.stop.store(true, Ordering::Release);
        }
        if self.state.current().is_capturing()
            || matches!(self.state.current(), SessionState::Starting)
        {
            let _ = self.state.transition(SessionState::Stopping);
        }
        self.join_workers(true)
    }

    fn watch_path(&self) -> PathBuf {
        if let Some(dir) = &self.config.output_dir {
            return dir.clone();
        }
        if let Some(dir) = &self.config.sidecar_dir {
            return dir.clone();
        }
        std::env::temp_dir()
    }

    fn preflight_storage(&self) -> Result<(), CoreError> {
        let path = resolve_probe_path(Some(&self.watch_path()));
        match self.storage.free_bytes(&path) {
            Ok(free) => {
                self.metrics.set_disk_free_bytes(Some(free));
                if free < self.config.limits.min_free_bytes {
                    return Err(CoreError::Safety(format!(
                        "insufficient free space: {free} bytes (minimum {})",
                        self.config.limits.min_free_bytes
                    )));
                }
                Ok(())
            }
            Err(err) => {
                if self.config.limits.require_disk_probe {
                    Err(err.into())
                } else {
                    self.metrics.set_disk_free_bytes(None);
                    Ok(())
                }
            }
        }
    }

    fn spawn_workers(&mut self) -> Result<(), CoreError> {
        let video = self
            .video
            .take()
            .ok_or(CoreError::MissingBackend("video_capture"))?;
        let audio = self
            .audio
            .take()
            .ok_or(CoreError::MissingBackend("audio_capture"))?;
        let encoder = self
            .encoder
            .take()
            .ok_or(CoreError::MissingBackend("video_encoder"))?;
        let audio_encoder: Box<dyn AudioEncoder> = match self.audio_encoder.take() {
            Some(enc) => enc,
            None => Box::new(NullAudioEncoder::new()),
        };
        let muxer = self
            .muxer
            .take()
            .ok_or(CoreError::MissingBackend("muxer"))?;

        let video_request = match self.video_request.clone() {
            Some(r) => r,
            None => return Err(CoreError::Config("missing CaptureRequest".to_string())),
        };
        let audio_request = match self.audio_request.clone() {
            Some(r) => r,
            None => return Err(CoreError::Config("missing AudioRequest".to_string())),
        };

        let (video_tx, video_rx) = bounded(self.config.queues.video);
        let (audio_tx, audio_rx) = bounded(self.config.queues.audio);
        let (pkt_tx, pkt_rx) = bounded(self.config.queues.packets);

        let stop = Arc::new(AtomicBool::new(false));
        let capture_ready = Arc::new(AtomicBool::new(false));
        let mux_output = Arc::new(Mutex::new(None));
        let metrics = Arc::clone(&self.metrics);
        let retries = self.config.backpressure.encode_full_retries;
        let packet_timeout = self.config.backpressure.packet_send_timeout;
        let quality = self.config.quality.clone();
        let session_id = self.config.session_id;
        let sidecar = self.sidecar_path();
        let memory = Arc::new(MemoryTracker::new(self.config.limits.max_pipeline_bytes));
        let safety_reason = Arc::new(Mutex::new(None));
        let degraded = Arc::new(Mutex::new(None));
        let limits = self.config.limits;
        let video_cap = self.config.queues.video as u64;
        let packet_cap = self.config.queues.packets as u64;

        let output_path = self
            .config
            .output_dir
            .as_ref()
            .map(|dir| dir.join(format!("gamecap-{session_id}.mkv")));
        let mux_cfg = MuxerConfig {
            session_id,
            container: ContainerKind::Mkv,
            output_path,
            sidecar_path: sidecar.clone(),
            video_width: Some(video_request.format.width),
            video_height: Some(video_request.format.height),
            audio_sample_rate_hz: Some(audio_request.format.sample_rate.hz()),
            audio_channels: Some(audio_request.format.channels.get()),
            default_frame_duration_ns: video_request
                .format
                .frame_rate
                .and_then(|r| r.period_nanos().ok()),
        };

        let mut threads = Vec::new();

        {
            let mux_output = Arc::clone(&mux_output);
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            threads.push(spawn_named("gamecap-mux", move || {
                mux_loop(
                    muxer,
                    mux_cfg,
                    pkt_rx,
                    mux_output,
                    metrics,
                    stop,
                    limits.max_output_bytes,
                )
            })?);
        }

        {
            let pkt_tx = pkt_tx.clone();
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            let format = video_request.format.clone();
            let memory = Arc::clone(&memory);
            threads.push(spawn_named("gamecap-encode", move || {
                encode_loop(
                    encoder,
                    format,
                    quality,
                    video_rx,
                    pkt_tx,
                    metrics,
                    stop,
                    packet_timeout,
                    memory,
                )
            })?);
        }

        {
            let pkt_tx = pkt_tx.clone();
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            let memory = Arc::clone(&memory);
            threads.push(spawn_named("gamecap-audio-enc", move || {
                audio_encode_loop(
                    audio_encoder,
                    audio_rx,
                    pkt_tx,
                    metrics,
                    stop,
                    packet_timeout,
                    memory,
                )
            })?);
        }
        drop(pkt_tx);

        {
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            let memory = Arc::clone(&memory);
            let duration = self.config.duration;
            let capture_ready = Arc::clone(&capture_ready);
            let clock = self.clock.clone();
            threads.push(spawn_named("gamecap-capture", move || {
                capture_loop(
                    video,
                    video_request,
                    video_tx,
                    metrics,
                    stop,
                    retries,
                    memory,
                    duration,
                    capture_ready,
                    clock,
                )
            })?);
        }

        {
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            let memory = Arc::clone(&memory);
            let duration = None;
            let capture_ready = Arc::clone(&capture_ready);
            threads.push(spawn_named("gamecap-audio", move || {
                audio_loop(
                    audio,
                    audio_request,
                    audio_tx,
                    metrics,
                    stop,
                    retries,
                    memory,
                    duration,
                    capture_ready,
                )
            })?);
        }

        {
            let metrics = Arc::clone(&metrics);
            let stop = Arc::clone(&stop);
            let storage = Arc::clone(&self.storage);
            let telemetry = Arc::clone(&self.telemetry);
            let watch = self.watch_path();
            let memory = Arc::clone(&memory);
            let safety_reason = Arc::clone(&safety_reason);
            let degraded = Arc::clone(&degraded);
            threads.push(spawn_named("gamecap-governor", move || {
                governor_loop(
                    stop,
                    metrics,
                    storage,
                    telemetry,
                    watch,
                    memory,
                    limits,
                    video_cap,
                    packet_cap,
                    safety_reason,
                    degraded,
                )
            })?);
        }

        self.running = Some(Running {
            stop,
            threads,
            mux_output,
            safety_reason,
            degraded,
        });
        Ok(())
    }

    fn join_workers(&mut self, _explicit_stop: bool) -> Result<SessionOutcome, CoreError> {
        let running = match self.running.take() {
            Some(r) => r,
            None => {
                return Err(CoreError::InvalidTransition {
                    from: self.state.current().to_string(),
                    to: "join".to_string(),
                });
            }
        };

        let mut first_err: Option<CoreError> = None;
        for handle in running.threads {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
                Err(_) => {
                    if first_err.is_none() {
                        first_err = Some(CoreError::Join("worker panicked".to_string()));
                    }
                }
            }
        }

        self.metrics.mark_end(self.clock.now());
        let mux = match running.mux_output.lock() {
            Ok(guard) => guard.clone().unwrap_or_default(),
            Err(_) => MuxerOutput::default(),
        };

        if matches!(
            self.state.current(),
            SessionState::Recording | SessionState::Degraded { .. } | SessionState::Reconfiguring
        ) {
            let _ = self.state.transition(SessionState::Stopping);
        }
        if matches!(self.state.current(), SessionState::Stopping) {
            let _ = self.state.transition(SessionState::Finalizing);
        }

        if let Some(err) = first_err {
            self.state.fail(
                err.to_string(),
                Diagnostics {
                    last_error: Some(err.to_string()),
                    ..Diagnostics::default()
                },
            );
            return Err(err);
        }

        if matches!(self.state.current(), SessionState::Finalizing) {
            self.state.transition(SessionState::Completed)?;
        }

        let safety_stop = match running.safety_reason.lock() {
            Ok(guard) => *guard,
            Err(_) => None,
        };
        let degraded = match running.degraded.lock() {
            Ok(guard) => guard.clone(),
            Err(_) => None,
        };

        let metrics = self.metrics.snapshot();
        let summary = SessionSummary::new(metrics.clone());
        let sidecar_path = self.write_sidecar(&summary)?;
        Ok(SessionOutcome {
            safety_stop,
            degraded,
            state: self.state.current(),
            metrics,
            summary,
            mux,
            sidecar_path,
        })
    }

    fn sidecar_path(&self) -> Option<PathBuf> {
        if !self.config.write_sidecar {
            return None;
        }
        let dir = match &self.config.sidecar_dir {
            Some(dir) => dir.clone(),
            None => std::env::temp_dir(),
        };
        Some(dir.join(format!("gamecap-session-{}.json", self.config.session_id)))
    }

    fn write_sidecar(&self, summary: &SessionSummary) -> Result<Option<PathBuf>, CoreError> {
        let path = match self.sidecar_path() {
            Some(p) => p,
            None => return Ok(None),
        };
        assert_user_write_path(&path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| CoreError::Config(e.to_string()))?;
        }
        fs::write(&path, summary.to_json()).map_err(|e| CoreError::Config(e.to_string()))?;
        Ok(Some(path))
    }
}

impl Drop for RecorderSession {
    fn drop(&mut self) {
        if self.running.is_some() {
            let _ = self.stop();
        }
    }
}

pub struct RecorderBuilder {
    config: SessionConfig,
    video: Option<Box<dyn VideoCaptureBackend>>,
    audio: Option<Box<dyn AudioCaptureBackend>>,
    encoder: Option<Box<dyn VideoEncoder>>,
    audio_encoder: Option<Box<dyn AudioEncoder>>,
    muxer: Option<Box<dyn Muxer>>,
    video_request: Option<CaptureRequest>,
    audio_request: Option<AudioRequest>,
    storage: Option<Arc<dyn StorageProbe>>,
    telemetry: Option<Arc<dyn HostTelemetry>>,
}

impl RecorderBuilder {
    pub fn video_capture(mut self, backend: impl VideoCaptureBackend + 'static) -> Self {
        self.video = Some(Box::new(backend));
        self
    }

    pub fn audio_capture(mut self, backend: impl AudioCaptureBackend + 'static) -> Self {
        self.audio = Some(Box::new(backend));
        self
    }

    pub fn video_encoder(mut self, encoder: impl VideoEncoder + 'static) -> Self {
        self.encoder = Some(Box::new(encoder));
        self
    }

    pub fn audio_encoder(mut self, encoder: impl AudioEncoder + 'static) -> Self {
        self.audio_encoder = Some(Box::new(encoder));
        self
    }

    pub fn muxer(mut self, muxer: impl Muxer + 'static) -> Self {
        self.muxer = Some(Box::new(muxer));
        self
    }

    pub fn audio_capture_box(mut self, backend: Box<dyn AudioCaptureBackend>) -> Self {
        self.audio = Some(backend);
        self
    }

    pub fn video_encoder_box(mut self, encoder: Box<dyn VideoEncoder>) -> Self {
        self.encoder = Some(encoder);
        self
    }

    pub fn capture_request(mut self, request: CaptureRequest) -> Self {
        self.video_request = Some(request);
        self
    }

    pub fn audio_request(mut self, request: AudioRequest) -> Self {
        self.audio_request = Some(request);
        self
    }

    pub fn storage_probe(mut self, probe: impl StorageProbe + 'static) -> Self {
        self.storage = Some(Arc::new(probe));
        self
    }

    pub fn telemetry(mut self, telemetry: impl HostTelemetry + 'static) -> Self {
        self.telemetry = Some(Arc::new(telemetry));
        self
    }

    pub fn build(self) -> RecorderSession {
        let metrics = Arc::new(MetricsCollector::new(self.config.session_id));
        let storage: Arc<dyn StorageProbe> = match self.storage {
            Some(p) => p,
            None => Arc::new(OsStorageProbe),
        };
        let telemetry: Arc<dyn HostTelemetry> = match self.telemetry {
            Some(t) => t,
            None => Arc::new(OsTelemetry),
        };
        RecorderSession {
            config: self.config,
            clock: MasterClock::new(),
            state: StateMachine::new(),
            metrics,
            video: self.video,
            audio: self.audio,
            encoder: self.encoder,
            audio_encoder: self.audio_encoder,
            muxer: self.muxer,
            video_request: self.video_request,
            audio_request: self.audio_request,
            storage,
            telemetry,
            running: None,
        }
    }
}

fn spawn_named(
    name: &str,
    f: impl FnOnce() -> Result<(), CoreError> + Send + 'static,
) -> Result<JoinHandle<Result<(), CoreError>>, CoreError> {
    thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .map_err(|e| CoreError::Join(e.to_string()))
}

#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn capture_loop(
    mut backend: Box<dyn VideoCaptureBackend>,
    request: CaptureRequest,
    tx: BoundedSender<gamecap_types::VideoFrame>,
    metrics: Arc<MetricsCollector>,
    stop: Arc<AtomicBool>,
    retries: u32,
    memory: Arc<MemoryTracker>,
    duration: Option<Duration>,
    capture_ready: Arc<AtomicBool>,
    clock: MasterClock,
) -> Result<(), CoreError> {
    if let Err(err) = backend.start(request) {
        stop.store(true, Ordering::Release);
        return Err(err.into());
    }
    metrics.mark_start(clock.now());
    capture_ready.store(true, Ordering::Release);
    let started_at = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        if backend.is_complete() {
            break;
        }
        if let Some(limit) = duration {
            if started_at.elapsed() >= limit {
                stop.store(true, Ordering::Release);
                break;
            }
        }
        match backend.poll_frame(Instant::now() + Duration::from_millis(5)) {
            Ok(Some(frame)) => {
                metrics.inc_frames_seen();
                metrics.inc_frames_captured();
                metrics.record_capture_interval(frame.capture_ts);
                metrics.record_zero_copy(frame.storage.is_gpu());
                metrics.set_capture_queue_depth(tx.len() as u64);
                let bytes = video_frame_bytes(&frame);
                if !memory.try_reserve(bytes) {
                    metrics.record_drop(DropReason::MemoryLimit);
                    continue;
                }
                match offer_from_capture(
                    |item| tx.try_send(item),
                    frame,
                    retries,
                    DropReason::EncodeBackpressure,
                ) {
                    OfferOutcome::Queued => {}
                    OfferOutcome::Dropped(reason) => {
                        memory.release(bytes);
                        metrics.record_drop(reason);
                    }
                    OfferOutcome::Closed => {
                        memory.release(bytes);
                        break;
                    }
                }
            }
            Ok(None) => {
                if backend.is_complete() {
                    break;
                }
            }
            Err(e) => {
                let _ = backend.stop();
                stop.store(true, Ordering::Release);
                return Err(e.into());
            }
        }
    }
    backend.stop()?;
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::result_large_err)]
fn audio_loop(
    mut backend: Box<dyn AudioCaptureBackend>,
    request: AudioRequest,
    tx: BoundedSender<gamecap_types::AudioBlock>,
    metrics: Arc<MetricsCollector>,
    stop: Arc<AtomicBool>,
    retries: u32,
    memory: Arc<MemoryTracker>,
    duration: Option<Duration>,
    capture_ready: Arc<AtomicBool>,
) -> Result<(), CoreError> {
    while !capture_ready.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(5));
    }
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    if let Err(err) = backend.start(request) {
        stop.store(true, Ordering::Release);
        return Err(err.into());
    }
    let started_at = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        if backend.is_complete() {
            break;
        }
        if let Some(limit) = duration {
            if started_at.elapsed() >= limit {
                stop.store(true, Ordering::Release);
                break;
            }
        }
        match backend.poll_audio(Instant::now() + Duration::from_millis(5)) {
            Ok(Some(block)) => {
                metrics.inc_audio_blocks();
                let bytes = audio_block_bytes(&block);
                if !memory.try_reserve(bytes) {
                    metrics.record_drop(DropReason::MemoryLimit);
                    continue;
                }
                match offer_from_capture(
                    |item| tx.try_send(item),
                    block,
                    retries,
                    DropReason::EncodeBackpressure,
                ) {
                    OfferOutcome::Queued => {}
                    OfferOutcome::Dropped(reason) => {
                        memory.release(bytes);
                        metrics.record_drop(reason);
                    }
                    OfferOutcome::Closed => {
                        memory.release(bytes);
                        break;
                    }
                }
            }
            Ok(None) => {
                if backend.is_complete() {
                    break;
                }
            }
            Err(e) => {
                let _ = backend.stop();
                stop.store(true, Ordering::Release);
                return Err(e.into());
            }
        }
    }
    backend.stop()?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_loop(
    mut encoder: Box<dyn VideoEncoder>,
    format: gamecap_types::VideoFormat,
    quality: QualityProfile,
    rx: crate::queue::BoundedReceiver<gamecap_types::VideoFrame>,
    tx: BoundedSender<PipelinePacket>,
    metrics: Arc<MetricsCollector>,
    stop: Arc<AtomicBool>,
    packet_timeout: Duration,
    memory: Arc<MemoryTracker>,
) -> Result<(), CoreError> {
    let bit_depth = format.pixel_format.bit_depth();
    let caps = encoder.capabilities();
    let codec = caps.codecs.first().copied().unwrap_or(CodecId::Null);
    encoder.configure(EncoderConfig {
        format,
        codec,
        quality,
        bit_depth,
    })?;
    loop {
        metrics.set_encoder_queue_depth(rx.len() as u64);
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(frame) => {
                let bytes = video_frame_bytes(&frame);
                encoder.submit(&GpuFrameRef::new(&frame))?;
                metrics.inc_frames_submitted();
                let packets = encoder.receive()?;
                memory.release(bytes);
                for packet in packets {
                    metrics.inc_frames_encoded();
                    metrics.add_bytes_written(packet.data.len() as u64);
                    match tx.send_timeout(PipelinePacket::Video(packet), packet_timeout) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            metrics.record_drop(DropReason::MuxBackpressure);
                        }
                        Err(TrySendError::Closed(_)) => return Ok(()),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Acquire) && rx.is_empty() {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for packet in encoder.drain()? {
        metrics.inc_frames_encoded();
        let _ = tx.send_timeout(PipelinePacket::Video(packet), packet_timeout);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn audio_encode_loop(
    mut encoder: Box<dyn AudioEncoder>,
    rx: crate::queue::BoundedReceiver<gamecap_types::AudioBlock>,
    tx: BoundedSender<PipelinePacket>,
    metrics: Arc<MetricsCollector>,
    stop: Arc<AtomicBool>,
    packet_timeout: Duration,
    memory: Arc<MemoryTracker>,
) -> Result<(), CoreError> {
    encoder.configure(AudioFormat::pcm_f32_48k_stereo_interleaved())?;
    loop {
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(block) => {
                let bytes = audio_block_bytes(&block);
                encoder.submit(&block)?;
                memory.release(bytes);
                for packet in encoder.receive()? {
                    let track = match packet.audio_track {
                        Some(t) => t,
                        None => AudioTrackKind::System.default_id(),
                    };
                    match tx.send_timeout(PipelinePacket::Audio { track, packet }, packet_timeout) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            metrics.record_drop(DropReason::MuxBackpressure);
                        }
                        Err(TrySendError::Closed(_)) => return Ok(()),
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Acquire) && rx.is_empty() {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

fn mux_loop(
    mut muxer: Box<dyn Muxer>,
    cfg: MuxerConfig,
    rx: crate::queue::BoundedReceiver<PipelinePacket>,
    output: Arc<Mutex<Option<MuxerOutput>>>,
    metrics: Arc<MetricsCollector>,
    stop: Arc<AtomicBool>,
    max_output_bytes: u64,
) -> Result<(), CoreError> {
    muxer.open(cfg)?;
    loop {
        if stop.load(Ordering::Acquire) && rx.is_empty() {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(PipelinePacket::Video(packet)) => {
                metrics.set_packet_queue_depth(rx.len() as u64);
                if output_limit_reached(metrics.bytes_written(), max_output_bytes) {
                    metrics.record_drop(DropReason::DiskBackpressure);
                    stop.store(true, Ordering::Release);
                    break;
                }
                muxer.write_video(packet)?;
            }
            Ok(PipelinePacket::Audio { track, packet }) => {
                if output_limit_reached(metrics.bytes_written(), max_output_bytes) {
                    metrics.record_drop(DropReason::DiskBackpressure);
                    stop.store(true, Ordering::Release);
                    break;
                }
                muxer.write_audio(track, packet)?;
            }
            Err(RecvTimeoutError::Timeout) => {
                if stop.load(Ordering::Acquire) && rx.is_empty() {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    while let Ok(packet) = rx.try_recv() {
        if output_limit_reached(metrics.bytes_written(), max_output_bytes) {
            metrics.record_drop(DropReason::DiskBackpressure);
            break;
        }
        match packet {
            PipelinePacket::Video(p) => muxer.write_video(p)?,
            PipelinePacket::Audio { track, packet } => muxer.write_audio(track, packet)?,
        }
    }
    let finalized = muxer.finalize()?;
    if let Ok(mut guard) = output.lock() {
        *guard = Some(finalized);
    }
    stop.store(true, Ordering::Release);
    Ok(())
}

fn output_limit_reached(written: u64, max_output_bytes: u64) -> bool {
    max_output_bytes > 0 && written >= max_output_bytes
}

#[allow(clippy::too_many_arguments)]
fn governor_loop(
    stop: Arc<AtomicBool>,
    metrics: Arc<MetricsCollector>,
    storage: Arc<dyn StorageProbe>,
    telemetry: Arc<dyn HostTelemetry>,
    watch: PathBuf,
    memory: Arc<MemoryTracker>,
    limits: gamecap_config::ResourceLimits,
    video_cap: u64,
    packet_cap: u64,
    safety_reason: Arc<Mutex<Option<SafetyStopReason>>>,
    degraded: Arc<Mutex<Option<DegradedReason>>>,
) -> Result<(), CoreError> {
    let mut streak = SaturationStreak::default();
    let mut last_written = 0u64;
    let path = resolve_probe_path(Some(&watch));
    while !stop.load(Ordering::Acquire) {
        let mut host = telemetry.sample();
        let disk_free = storage.free_bytes(&path).ok();
        host.disk_free_bytes = disk_free;
        metrics.record_host_sample(host.clone());
        metrics.set_disk_free_bytes(disk_free);
        metrics.set_pipeline_bytes(memory.current());

        let snap_metrics = metrics.snapshot();
        let written = metrics.bytes_written();
        let snap = ResourceSnapshot {
            disk_free_bytes: disk_free,
            pipeline_bytes: memory.current(),
            bytes_written: written,
            video_queue_depth: snap_metrics.capture_queue_depth,
            video_queue_capacity: video_cap,
            packet_queue_depth: snap_metrics.packet_queue_depth,
            packet_queue_capacity: packet_cap,
            encode_backpressure_drops: snap_metrics.drops.encode_backpressure,
            mux_backpressure_drops: snap_metrics.drops.mux_backpressure,
            disk_backpressure_drops: snap_metrics.drops.disk_backpressure,
            gpu_busy_percent: host.gpu_busy_percent,
            bytes_written_unchanged: written == last_written,
        };
        last_written = written;

        let (action, next) = evaluate(&limits, &snap, streak);
        streak = next;
        match action {
            SafetyAction::Continue => {}
            SafetyAction::Degrade(reason) => {
                if let Ok(mut guard) = degraded.lock() {
                    *guard = Some(DegradedReason {
                        component: reason.as_str().to_string(),
                        detail: "resource saturation; dropping rather than blocking the host"
                            .to_string(),
                    });
                }
            }
            SafetyAction::Stop(reason) => {
                metrics.record_safety_stop(reason);
                if let Ok(mut guard) = safety_reason.lock() {
                    *guard = Some(reason);
                }
                stop.store(true, Ordering::Release);
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}
