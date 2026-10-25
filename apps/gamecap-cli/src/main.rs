//! GameCap CLI: synthetic pipeline (M0) and Linux screen recording (M1).

#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gamecap_audio::{AudioRequest, FakeAudioBackend, FakeAudioConfig, PcmPassthroughEncoder};
use gamecap_capture::{CaptureRequest, FakeCaptureBackend, FakeCaptureConfig};
use gamecap_config::{BackpressurePolicy, QueueConfig, ResourceLimits, SessionConfig};
use gamecap_core::RecorderSession;
use gamecap_encoder::NullEncoder;
use gamecap_hardware::assert_user_write_path;
use gamecap_mux::NullMuxer;
use gamecap_types::{
    AudioFormat, AudioTrackKind, CaptureTarget, CursorMode, FrameRateMode, HdrPolicy, PixelFormat,
    QualityProfile, Rational, VideoFormat,
};

#[cfg(target_os = "linux")]
use gamecap_audio::SilenceAudioBackend;
#[cfg(target_os = "linux")]
use gamecap_audio_linux::PulseMonitorCapture;

const KEYINT_DEFAULT: f64 = 1.5;
const KEYINT_MIN: f64 = 1.0;
const KEYINT_MAX: f64 = 2.0;

fn normalize_quality(quality: &str) -> &str {
    match quality {
        "medium" => "medium",
        "high" | "very_high" | "ultra" => "high",
        _ => "high",
    }
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("gamecap: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let first = args.first().map(String::as_str);
    match first {
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some("gui") | None => run_gui(),
        Some("toggle") => run_toggle(),
        Some("run-fake") => run_fake(&args[1..]),
        Some("record") => run_record(&args[1..]),
        Some(flag) if flag.starts_with('-') => run_record(&args),
        Some(other) => Err(format!("unknown command '{other}'. Use `gamecap help`.")),
    }
}

fn print_help() {
    println!(
        "GameCap — grabadora de juegos\n\n\
         Sin argumentos abre la interfaz. Desde la CLI:\n\n\
         Uso:\n  \
         gamecap gui\n  \
         gamecap toggle\n  \
         gamecap record [--until-stop] [--duration-secs N] [--output-dir DIR]\n            \
         [--monitor] [--region WxH+X+Y] [--now] [--portal]\n            \
         [--status-file PATH] [--stop-file PATH] [--output-name FILE]\n  \
         gamecap run-fake --fps 144 --duration-secs 10 [--width 128 --height 72]\n"
    );
}

fn run_fake(args: &[String]) -> Result<(), String> {
    let fps = parse_u32_flag(args, "--fps")?.unwrap_or(144);
    let duration_secs = parse_u32_flag(args, "--duration-secs")?.unwrap_or(10);
    let width = parse_u32_flag(args, "--width")?.unwrap_or(128);
    let height = parse_u32_flag(args, "--height")?.unwrap_or(72);

    let fps_r = Rational::new(fps, 1).map_err(|e| e.to_string())?;
    let duration = Duration::from_secs(u64::from(duration_secs));
    let format =
        VideoFormat::sdr_rec709(width, height, PixelFormat::Bgra8).map_err(|e| e.to_string())?;

    let queues = QueueConfig::new(256, 256, 512).map_err(|e| e.to_string())?;
    let config = SessionConfig::builder()
        .queues(queues)
        .backpressure(BackpressurePolicy {
            encode_full_retries: 50_000,
            packet_send_timeout: Duration::from_millis(100),
        })
        .quality(QualityProfile::HighQuality)
        .frame_rate_mode(FrameRateMode::Custom { fps: fps_r })
        .duration(duration)
        .write_sidecar(true)
        .build();

    let capture = FakeCaptureBackend::new(FakeCaptureConfig::new(format.clone(), fps_r, duration));
    let audio = FakeAudioBackend::new(FakeAudioConfig::new(duration));

    let mut session = RecorderSession::builder(config)
        .video_capture(capture)
        .audio_capture(audio)
        .video_encoder(NullEncoder::new())
        .muxer(NullMuxer::new())
        .capture_request(CaptureRequest::synthetic(
            format,
            FrameRateMode::Custom { fps: fps_r },
        ))
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System, AudioTrackKind::Microphone],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .build();

    session.start().map_err(|e| e.to_string())?;
    let outcome = session.wait().map_err(|e| e.to_string())?;
    print!("{}", outcome.summary);
    if let Some(path) = outcome.sidecar_path {
        println!("Sidecar: {}", path.display());
    }
    Ok(())
}

fn run_gui() -> Result<(), String> {
    let script = desktop_script("ui.py").ok_or_else(|| {
        "no se encontró apps/gamecap-desktop/ui.py".to_string()
    })?;
    let status = Command::new("python3")
        .arg(&script)
        .status()
        .map_err(|e| format!("no se pudo abrir la interfaz: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("la interfaz salió con {status}"))
    }
}

fn run_toggle() -> Result<(), String> {
    let script = desktop_script("toggle.py").ok_or_else(|| {
        "no se encontró apps/gamecap-desktop/toggle.py".to_string()
    })?;
    let status = Command::new("python3")
        .arg(&script)
        .status()
        .map_err(|e| format!("no se pudo conmutar la grabación: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("toggle salió con {status}"))
    }
}

fn desktop_script(name: &str) -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("../apps/gamecap-desktop").join(name));
            cands.push(dir.join(name));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        cands.push(cwd.join("apps/gamecap-desktop").join(name));
    }
    cands.into_iter().find(|p| p.is_file())
}

fn run_record(args: &[String]) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        return Err(
            "record is implemented for Linux (portal + PipeWire) in this milestone".to_string(),
        );
    }
    #[cfg(target_os = "linux")]
    {
        linux_record(args)
    }
}

#[cfg(target_os = "linux")]
fn linux_record(args: &[String]) -> Result<(), String> {
    use gamecap_capture_linux::{
        detect_display_mode, gst_nvh264enc_available, gst_vaav1enc_available,
        PortalPipeWireCapture,
    };
    use gamecap_mux::MkvMuxer;

    let until_stop = args.iter().any(|a| a == "--until-stop");
    let duration_secs = parse_u32_flag(args, "--duration-secs")?
        .unwrap_or(if until_stop { 0 } else { 10 });
    let duration = Duration::from_secs(u64::from(duration_secs.max(1)));
    let output_dir = match parse_string_flag(args, "--output-dir") {
        Some(dir) => PathBuf::from(dir),
        None => default_output_dir(),
    };
    fs::create_dir_all(&output_dir).map_err(|e| e.to_string())?;
    assert_user_write_path(&output_dir.join("gamecap.mkv")).map_err(|e| e.to_string())?;

    let mode = detect_display_mode();
    let hz = mode.refresh_hz;
    let force_portal = args.iter().any(|a| a == "--portal");
    if !force_portal {
        match record_with_gsr(GsrOptions {
            duration,
            duration_secs,
            until_stop,
            output_dir: &output_dir,
            output_name: parse_string_flag(args, "--output-name"),
            mode: &mode,
            whole_monitor: args.iter().any(|a| a == "--monitor"),
            wait_for_game: !args.iter().any(|a| a == "--now"),
            region: parse_string_flag(args, "--region"),
            status_file: parse_string_flag(args, "--status-file").map(PathBuf::from),
            stop_file: parse_string_flag(args, "--stop-file").map(PathBuf::from),
            fps_override: parse_u32_flag(args, "--fps")?,
            codec: parse_string_flag(args, "--codec").unwrap_or("av1"),
            container: parse_string_flag(args, "--container").unwrap_or("mkv"),
            audio_sources: parse_all_string_flags(args, "--audio"),
            audio_bitrate: parse_u32_flag(args, "--audio-bitrate")?.unwrap_or(192),
            monitor_name: parse_string_flag(args, "--monitor-name"),
            device: parse_string_flag(args, "--device"),
            quality: parse_string_flag(args, "--quality").unwrap_or("high"),
            cursor: parse_string_flag(args, "--cursor").unwrap_or("yes"),
            frame_mode: parse_string_flag(args, "--frame-mode").unwrap_or("vfr"),
            tune: parse_string_flag(args, "--tune").unwrap_or("performance"),
            keyint: parse_f64_flag(args, "--keyint")?.unwrap_or(KEYINT_DEFAULT),
            scale: parse_string_flag(args, "--scale"),
            replay_secs: parse_u32_flag(args, "--replay-secs")?.unwrap_or(0),
            ipc: parse_string_flag(args, "--ipc"),
        }) {
            Ok(()) => return Ok(()),
            Err(err) => {
                eprintln!("Aviso: captura KMS no disponible ({err}). Se usa el portal.");
            }
        }
    }
    println!("Comprobando encoder de vídeo...");
    let va_av1 = gst_vaav1enc_available();
    let gst_nvenc = !va_av1 && gst_nvh264enc_available();
    let (encoder, encoder_name) = select_linux_encoder(hz, va_av1, gst_nvenc);
    let capture = if va_av1 {
        PortalPipeWireCapture::new().with_gst_va_av1()
    } else if gst_nvenc {
        PortalPipeWireCapture::new().with_gst_nvenc()
    } else {
        PortalPipeWireCapture::new()
    };
    println!("Encoder: {encoder_name}");
    println!(
        "Pantalla: {}×{} @ {hz} Hz — captura y encode se adaptan a este modo.",
        mode.width, mode.height
    );
    println!("GameCap va a pedir permiso para compartir pantalla.");
    println!("Elige un monitor o ventana. La cuenta de {duration_secs} s empieza al aceptar.");
    println!("Salida: {}", output_dir.display());

    let mut format = VideoFormat::sdr_rec709(mode.width, mode.height, PixelFormat::Bgra8)
        .map_err(|e| e.to_string())?;
    let fps = Rational::from_integer(hz);
    format.frame_rate = Some(fps);
    let queues = QueueConfig::new(64, 32, 128).map_err(|e| e.to_string())?;
    let mut limits = ResourceLimits::default();
    limits.max_pipeline_bytes = 384 * 1024 * 1024;
    let config = SessionConfig::builder()
        .queues(queues)
        .backpressure(BackpressurePolicy {
            encode_full_retries: 16,
            packet_send_timeout: Duration::from_millis(50),
        })
        .quality(QualityProfile::HighQuality)
        .frame_rate_mode(FrameRateMode::DisplaySync { refresh: fps })
        .duration(duration)
        .write_sidecar(true)
        .sidecar_dir(output_dir.clone())
        .output_dir(output_dir.clone())
        .limits(limits)
        .build();

    let mut session = RecorderSession::builder(config)
        .video_capture(capture)
        .audio_capture(FallbackAudio::new())
        .video_encoder_box(encoder)
        .audio_encoder(PcmPassthroughEncoder::new())
        .muxer(MkvMuxer::new())
        .capture_request(CaptureRequest {
            target: CaptureTarget::Monitor {
                id: gamecap_types::MonitorId::from_raw(0),
            },
            format,
            frame_rate_mode: FrameRateMode::DisplaySync { refresh: fps },
            cursor: CursorMode::Embedded,
            hdr_policy: HdrPolicy::ForceSdr,
        })
        .audio_request(AudioRequest {
            tracks: vec![AudioTrackKind::System],
            format: AudioFormat::pcm_f32_48k_stereo_interleaved(),
            duration: Some(duration),
        })
        .build();

    session.start().map_err(|e| e.to_string())?;
    let outcome = session.wait().map_err(|e| e.to_string())?;
    print!("{}", outcome.summary);
    if let Some(path) = outcome.mux.path.as_ref() {
        println!("Archivo: {}", path.display());
    }
    if let Some(path) = outcome.sidecar_path {
        println!("Sidecar: {}", path.display());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct GsrOptions<'a> {
    duration: Duration,
    duration_secs: u32,
    until_stop: bool,
    output_dir: &'a Path,
    output_name: Option<&'a str>,
    mode: &'a gamecap_capture_linux::DisplayMode,
    whole_monitor: bool,
    wait_for_game: bool,
    region: Option<&'a str>,
    status_file: Option<PathBuf>,
    stop_file: Option<PathBuf>,
    fps_override: Option<u32>,
    codec: &'a str,
    container: &'a str,
    audio_sources: Vec<&'a str>,
    audio_bitrate: u32,
    monitor_name: Option<&'a str>,
    device: Option<&'a str>,
    quality: &'a str,
    cursor: &'a str,
    frame_mode: &'a str,
    tune: &'a str,
    keyint: f64,
    scale: Option<&'a str>,
    replay_secs: u32,
    ipc: Option<&'a str>,
}

#[cfg(target_os = "linux")]
fn record_with_gsr(opts: GsrOptions<'_>) -> Result<(), String> {
    let gsr = which_bin("gpu-screen-recorder")?;
    if !Path::new("/usr/bin/gsr-kms-server").is_file() {
        return Err("gsr-kms-server no está instalado".to_string());
    }
    let monitor = opts
        .monitor_name
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(gsr_monitor_name);
    let target = if let Some(dev) = opts.device.filter(|d| !d.is_empty()) {
        GsrTarget::Device(dev.to_string())
    } else {
        pick_gsr_target(&monitor, opts.whole_monitor, opts.region)
    };
    let out_name = opts.output_name.unwrap_or("gamecap-1.mkv");
    let out = opts.output_dir.join(out_name);
    assert_user_write_path(&out).map_err(|e| e.to_string())?;
    let hz = opts
        .fps_override
        .unwrap_or(opts.mode.refresh_hz)
        .clamp(24, 360);
    let codec = match opts.codec {
        "h264" | "hevc" | "av1" | "hevc_hdr" | "av1_hdr" => opts.codec,
        _ => "av1",
    };
    let container = match opts.container {
        "mp4" | "mkv" | "webm" => opts.container,
        _ => "mkv",
    };
    let quality = normalize_quality(opts.quality);
    let frame_mode = match opts.frame_mode {
        // KMS only supports vfr|cfr; content is X11/portal. Prefer vfr over cfr
        // to avoid encoder death spiral when game FPS dips.
        "content" | "vfr" | "cfr" => opts.frame_mode,
        _ => "vfr",
    };
    let tune = match opts.tune {
        "performance" | "quality" => opts.tune,
        _ => "performance",
    };
    let keyint = opts.keyint.clamp(KEYINT_MIN, KEYINT_MAX);
    let keyint_s = if (keyint - keyint.round()).abs() < 0.05 {
        format!("{}", keyint.round() as i32)
    } else {
        format!("{keyint:.1}")
    };
    let cursor = match opts.cursor {
        "no" | "off" | "0" => "no",
        _ => "yes",
    };
    let hz_s = hz.to_string();
    let out_s = out.to_string_lossy().into_owned();
    println!("Comprobando encoder de vídeo...");
    println!(
        "Encoder: gpu-screen-recorder KMS ({codec}, rango completo, misma GPU que el panel)"
    );
    match &target {
        GsrTarget::Monitor(name) => println!(
            "Pantalla: {}×{} @ {hz} Hz — monitor {name} completo.",
            opts.mode.width, opts.mode.height
        ),
        GsrTarget::Region { spec } => {
            println!("Pantalla: región {spec} @ {hz} Hz — recorte KMS del monitor.");
        }
        GsrTarget::Device(path) => {
            println!("Dispositivo de captura: {path} @ {hz} Hz.");
        }
    }
    if opts.wait_for_game && io::stdin().is_terminal() {
        arm_game_focus();
    }
    if opts.until_stop {
        println!("Grabando hasta Detener (SIGINT o stop-file).");
    } else {
        println!("La cuenta de {} s empieza ahora.", opts.duration_secs);
    }
    println!("Salida: {}", opts.output_dir.display());
    println!("Modo de fotogramas: {frame_mode} · tune: {tune} · calidad: {quality} · keyint: {keyint_s}s");

    let mut args: Vec<String> = vec![
        "-w".into(),
        target.source(),
        "-f".into(),
        hz_s,
        "-k".into(),
        codec.into(),
        "-c".into(),
        container.into(),
        "-cr".into(),
        "full".into(),
        "-fm".into(),
        frame_mode.into(),
        "-q".into(),
        quality.into(),
        "-bm".into(),
        "qp".into(),
        "-tune".into(),
        tune.into(),
        "-keyint".into(),
        keyint_s,
        "-cursor".into(),
        cursor.into(),
    ];
    if let Some(scale) = opts.scale.filter(|s| !s.is_empty() && *s != "0x0" && *s != "native") {
        args.push("-s".into());
        args.push(scale.to_string());
        println!("Resolución límite: {scale}");
    }
    if opts.replay_secs > 0 {
        args.push("-r".into());
        args.push(opts.replay_secs.to_string());
        args.push("-o".into());
        args.push(opts.output_dir.to_string_lossy().into_owned());
        args.push("-restart-replay-on-save".into());
        args.push("yes".into());
    } else {
        args.push("-o".into());
        args.push(out_s);
    }
    let audio_sources: Vec<&str> = if opts.audio_sources.is_empty() {
        vec!["default_output"]
    } else if opts.audio_sources.iter().any(|s| *s == "none" || s.is_empty())
        && opts.audio_sources.len() == 1
    {
        Vec::new()
    } else {
        opts.audio_sources
            .iter()
            .copied()
            .filter(|s| *s != "none" && !s.is_empty())
            .collect()
    };
    if !audio_sources.is_empty() {
        for src in &audio_sources {
            args.push("-a".into());
            args.push((*src).to_string());
        }
        args.push("-ac".into());
        args.push("opus".into());
        let audio_bitrate = editor_audio_bitrate(opts.audio_bitrate);
        if audio_bitrate > 0 {
            args.push("-ab".into());
            args.push(audio_bitrate.to_string());
        }
    }
    if let GsrTarget::Region { spec } = &target {
        args.push("-region".into());
        args.push(spec.clone());
    }
    if let Some(ipc) = opts.ipc.filter(|p| !p.is_empty()) {
        args.push("-ipc".into());
        args.push(ipc.to_string());
    }
    let mut child = Command::new(&gsr)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("no se pudo arrancar gpu-screen-recorder: {e}"))?;
    let live_fps = Arc::new(Mutex::new(0.0_f64));
    if let Some(stderr) = child.stderr.take() {
        let live_fps = Arc::clone(&live_fps);
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                eprintln!("{line}");
                if let Some(fps) = parse_gsr_fps(&line) {
                    if let Ok(mut slot) = live_fps.lock() {
                        *slot = fps;
                    }
                }
            }
        });
    }
    thread::sleep(Duration::from_millis(400));
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        return Err(format!("gpu-screen-recorder salió de inmediato ({status})"));
    }
    let gsr_pid = child.id();
    let started = Instant::now();
    let mut paused_total = Duration::ZERO;
    let mut pause_since: Option<Instant> = None;
    // Updated every loop tick; always assigned before the loop exits.
    let mut active_elapsed;
    loop {
        let paused_now = paused_flag();
        match (paused_now, pause_since) {
            (true, None) => pause_since = Some(Instant::now()),
            (false, Some(since)) => {
                paused_total += since.elapsed();
                pause_since = None;
            }
            _ => {}
        }
        let mut elapsed = started.elapsed().saturating_sub(paused_total);
        if let Some(since) = pause_since {
            elapsed = elapsed.saturating_sub(since.elapsed());
        }
        active_elapsed = elapsed;
        let fps_now = live_fps.lock().map(|v| *v).unwrap_or(0.0);
        write_status(
            opts.status_file.as_deref(),
            true,
            active_elapsed.as_secs_f64(),
            fps_now,
            &out,
            gsr_pid,
        );
        if let Some(path) = opts.stop_file.as_ref() {
            if path.is_file() {
                break;
            }
        }
        if let Some(_status) = child.try_wait().map_err(|e| e.to_string())? {
            break;
        }
        if !opts.until_stop && active_elapsed >= opts.duration {
            break;
        }
        thread::sleep(Duration::from_millis(400));
    }
    let pid = gsr_pid.to_string();
    let _ = Command::new("kill").args(["-INT", &pid]).status();
    let stop_wait = Instant::now();
    loop {
        if let Some(_status) = child.try_wait().map_err(|e| e.to_string())? {
            break;
        }
        if stop_wait.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    write_status(
        opts.status_file.as_deref(),
        false,
        active_elapsed.as_secs_f64(),
        live_fps.lock().map(|v| *v).unwrap_or(0.0),
        &out,
        0,
    );
    if !out.is_file() || fs::metadata(&out).map(|m| m.len()).unwrap_or(0) < 1024 {
        return Err("gpu-screen-recorder no escribió el MKV".to_string());
    }
    let meta = ffprobe_video(&out, hz);
    let frames = meta.as_ref().map(|m| m.frames).unwrap_or(0);
    let fps = meta.as_ref().map(|m| m.fps).unwrap_or(0.0);
    let span = meta.as_ref().map(|m| m.span_secs).unwrap_or(0.0);
    let dropped = meta.as_ref().map(|m| m.dropped).unwrap_or(0);
    println!("Duration: {span:.3} s");
    println!("Captured: {frames}");
    println!("Encoded: {frames}");
    println!("Dropped: {dropped}");
    println!("Frame rate: {fps:.2} fps (objetivo {hz} Hz)");
    println!("Archivo: {}", out.display());
    let sidecar = opts.output_dir.join("gamecap-session-1.json");
    let json = format!(
        "{{\n  \"session_id\": 1,\n  \"backend\": \"gpu-screen-recorder-kms\",\n  \"frames_encoded\": {frames},\n  \"frames_dropped\": {dropped},\n  \"duration_s\": {span:.6},\n  \"fps\": {fps:.4},\n  \"width\": {},\n  \"height\": {},\n  \"refresh_hz\": {hz}\n}}\n",
        opts.mode.width, opts.mode.height
    );
    let _ = fs::write(&sidecar, json);
    println!("Sidecar: {}", sidecar.display());
    Ok(())
}

#[cfg(target_os = "linux")]
fn parse_gsr_fps(line: &str) -> Option<f64> {
    let rest = line.trim().strip_prefix("gsr info: update fps: ")?;
    let token = rest.split(',').next()?.trim();
    token.parse().ok()
}

#[cfg(target_os = "linux")]
fn paused_sidecar() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cache/gamecap/paused")
}

#[cfg(target_os = "linux")]
fn paused_flag() -> bool {
    fs::read_to_string(paused_sidecar())
        .ok()
        .map(|s| {
            let t = s.trim();
            t == "1" || t.eq_ignore_ascii_case("true")
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn write_status(
    path: Option<&Path>,
    recording: bool,
    elapsed_s: f64,
    fps: f64,
    out: &Path,
    gsr_pid: u32,
) {
    let Some(path) = path else {
        return;
    };
    let file_mb = fs::metadata(out)
        .map(|m| m.len() as f64 / 1_048_576.0)
        .unwrap_or(0.0);
    let free_gb = df_free_gb(out.parent().unwrap_or(out));
    let rec = if recording { "true" } else { "false" };
    // Prefer ~/.cache/gamecap/paused sidecar (survives status rewrites).
    let paused = paused_flag()
        || fs::read_to_string(path)
            .ok()
            .map(|s| s.contains("\"paused\":true"))
            .unwrap_or(false);
    let paused_s = if paused { "true" } else { "false" };
    let json = format!(
        "{{\"recording\":{rec},\"paused\":{paused_s},\"elapsed_s\":{elapsed_s:.3},\"fps\":{fps:.2},\"file_mb\":{file_mb:.1},\"free_gb\":{free_gb:.1},\"gsr_pid\":{gsr_pid},\"path\":{}}}\n",
        json_string(&out.to_string_lossy()),
    );
    let _ = fs::write(path, json);
}

#[cfg(target_os = "linux")]
fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(target_os = "linux")]
fn df_free_gb(dir: &Path) -> f64 {
    let path = dir.to_string_lossy();
    let Ok(out) = Command::new("df")
        .args(["-B1", "--output=avail", path.as_ref()])
        .output()
    else {
        return 0.0;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .and_then(|line| line.trim().parse::<f64>().ok())
        .map(|bytes| bytes / 1_073_741_824.0)
        .unwrap_or(0.0)
}

#[cfg(target_os = "linux")]
enum GsrTarget {
    Monitor(String),
    Region { spec: String },
    Device(String),
}

#[cfg(target_os = "linux")]
impl GsrTarget {
    fn source(&self) -> String {
        match self {
            Self::Monitor(name) => name.clone(),
            Self::Region { .. } => "region".to_string(),
            Self::Device(path) => path.clone(),
        }
    }
}

#[cfg(target_os = "linux")]
fn pick_gsr_target(monitor: &str, whole_monitor: bool, preset: Option<&str>) -> GsrTarget {
    if let Some(spec) = preset.and_then(normalize_region) {
        return GsrTarget::Region { spec };
    }
    if whole_monitor || !io::stdin().is_terminal() {
        return GsrTarget::Monitor(monitor.to_string());
    }
    println!("Selecciona la ventana: dibuja un recuadro sobre ella (clic y arrastra).");
    println!("Esc cancela y graba el monitor entero ({monitor}).");
    match gnome_select_area()
        .or_else(gtk_select_area)
        .or_else(slurp_region)
    {
        Some(spec) => {
            println!("Región: {spec}");
            GsrTarget::Region { spec }
        }
        None => {
            println!("Sin recorte: se graba {monitor} completo.");
            GsrTarget::Monitor(monitor.to_string())
        }
    }
}

#[cfg(target_os = "linux")]
fn arm_game_focus() {
    println!();
    println!("Vuelve al juego y haz clic dentro: Minecraft (y otros) se congelan sin foco.");
    println!("Pantalla completa exclusiva también puede congelar el fotograma; usa ventana o sin bordes.");
    for i in (1..=3).rev() {
        println!("Grabación en {i}…");
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(target_os = "linux")]
fn gnome_select_area() -> Option<String> {
    let dests = [
        "org.gnome.Shell.Screenshot",
        "org.gnome.Shell",
    ];
    for dest in dests {
        let Ok(out) = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                dest,
                "--object-path",
                "/org/gnome/Shell/Screenshot",
                "--method",
                "org.gnome.Shell.Screenshot.SelectArea",
            ])
            .stdin(Stdio::null())
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if !err.is_empty() {
                eprintln!("Aviso: selector de GNOME ({dest}): {err}");
            }
            continue;
        }
        let raw = String::from_utf8_lossy(&out.stdout);
        if let Some(spec) = parse_select_area(&raw) {
            return Some(spec);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn gtk_select_area() -> Option<String> {
    let script = region_picker_script()?;
    let out = Command::new("python3")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    normalize_region(&raw)
}

#[cfg(target_os = "linux")]
fn region_picker_script() -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            cands.push(dir.join("pick-region.py"));
            cands.push(dir.join("../scripts/pick-region.py"));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        cands.push(cwd.join("scripts/pick-region.py"));
    }
    cands.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "linux")]
fn parse_select_area(stdout: &str) -> Option<String> {
    let nums: Vec<i32> = stdout
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let x = nums[0];
    let y = nums[1];
    let w = nums[2];
    let h = nums[3];
    if w < 2 || h < 2 {
        return None;
    }
    normalize_region(&format!("{w}x{h}+{x}+{y}"))
}

#[cfg(target_os = "linux")]
fn slurp_region() -> Option<String> {
    if desktop_is_gnome() {
        return None;
    }
    let slurp = which_bin("slurp").ok()?;
    let out = Command::new(slurp)
        .args(["-d", "-f", "%wx%h+%x+%y"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    normalize_region(&raw)
}

#[cfg(target_os = "linux")]
fn desktop_is_gnome() -> bool {
    env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .split(':')
        .any(|part| part == "gnome")
}

#[cfg(target_os = "linux")]
fn normalize_region(raw: &str) -> Option<String> {
    let (size, pos) = raw.split_once('+')?;
    let (w, h) = size.split_once('x')?;
    let (x, y) = pos.split_once('+')?;
    let mut w: u32 = w.parse().ok()?;
    let mut h: u32 = h.parse().ok()?;
    let mut x: i32 = x.parse().ok()?;
    let mut y: i32 = y.parse().ok()?;
    x &= !1;
    y &= !1;
    w &= !1;
    h &= !1;
    if w < 2 {
        w = 2;
    }
    if h < 2 {
        h = 2;
    }
    Some(format!("{w}x{h}+{x}+{y}"))
}

#[cfg(target_os = "linux")]
struct ProbeVideo {
    frames: u64,
    fps: f64,
    span_secs: f64,
    dropped: u64,
}

/// Opus under 160 kbps is what made the editor tracks sound hissy and crushed.
#[cfg(target_os = "linux")]
fn editor_audio_bitrate(raw: u32) -> u32 {
    if raw == 0 || raw < 160 {
        192
    } else {
        raw
    }
}

#[cfg(target_os = "linux")]
fn ffprobe_video(path: &Path, hz: u32) -> Option<ProbeVideo> {
    let out = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
            path.to_str()?,
        ])
        .output()
        .ok()?;
    let step = if hz == 0 { 0.0 } else { 1.0 / f64::from(hz) };
    let mut count = 0u64;
    let mut first = 0.0;
    let mut prev = 0.0;
    let mut last = 0.0;
    let mut dropped = 0u64;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(t) = line.split(',').next()?.parse::<f64>() else {
            continue;
        };
        if count == 0 {
            first = t;
        } else if step > 0.0 {
            let delta = t - prev;
            if delta > step * 1.75 {
                let missed = (delta / step).round() as i64 - 1;
                if missed > 0 {
                    dropped += missed as u64;
                }
            }
        }
        prev = t;
        last = t;
        count += 1;
    }
    if count < 2 {
        return None;
    }
    let span = last - first;
    if span <= 0.0 {
        return None;
    }
    Some(ProbeVideo {
        frames: count,
        fps: (count as f64 - 1.0) / span,
        span_secs: span,
        dropped,
    })
}

#[cfg(target_os = "linux")]
fn gsr_monitor_name() -> String {
    let Ok(out) = Command::new("gpu-screen-recorder")
        .arg("--list-monitors")
        .output()
    else {
        return "screen".to_string();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|line| line.split('|').next())
        .filter(|name| !name.is_empty())
        .unwrap_or("screen")
        .to_string()
}

#[cfg(target_os = "linux")]
fn which_bin(bin: &str) -> Result<String, String> {
    if let Ok(path) = env::var("PATH") {
        for dir in path.split(':') {
            let candidate = Path::new(dir).join(bin);
            if candidate.is_file() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }
    let fallback = Path::new("/usr/bin").join(bin);
    if fallback.is_file() {
        return Ok(fallback.to_string_lossy().into_owned());
    }
    Err(format!("{bin} no está en PATH"))
}

#[cfg(target_os = "linux")]
fn select_linux_encoder(
    hz: u32,
    va_av1: bool,
    gst_nvenc: bool,
) -> (Box<dyn gamecap_encoder::VideoEncoder>, &'static str) {
    use gamecap_encoder_ffmpeg::{AnnexBPassthroughEncoder, FfmpegH264Encoder, OpenH264Encoder};

    if va_av1 {
        return (
            Box::new(AnnexBPassthroughEncoder::av1()),
            "vaav1enc AMD (DMA-BUF, misma GPU que el monitor)",
        );
    }
    if gst_nvenc {
        return (
            Box::new(AnnexBPassthroughEncoder::h264()),
            "nvh264enc GPU (GStreamer, junto a PipeWire)",
        );
    }
    if FfmpegH264Encoder::nvenc_available() {
        let mut encoder = FfmpegH264Encoder::nvenc();
        encoder.set_target_fps(hz);
        let yuv444 = FfmpegH264Encoder::nvenc_444_available();
        encoder.set_yuv444(yuv444);
        let name = if yuv444 {
            "h264_nvenc 4:4:4 (ffmpeg)"
        } else {
            "h264_nvenc (ffmpeg)"
        };
        return (Box::new(encoder), name);
    }
    if FfmpegH264Encoder::vaapi_available() {
        let mut encoder = FfmpegH264Encoder::vaapi();
        encoder.set_target_fps(hz);
        return (Box::new(encoder), "h264_vaapi (ffmpeg)");
    }
    (Box::new(OpenH264Encoder::new()), "OpenH264 (software)")
}

#[cfg(target_os = "linux")]
struct FallbackAudio {
    pulse: PulseMonitorCapture,
    silence: SilenceAudioBackend,
    use_silence: bool,
}

#[cfg(target_os = "linux")]
impl FallbackAudio {
    fn new() -> Self {
        Self {
            pulse: PulseMonitorCapture::new(),
            silence: SilenceAudioBackend::new(),
            use_silence: false,
        }
    }
}

#[cfg(target_os = "linux")]
impl gamecap_audio::AudioCaptureBackend for FallbackAudio {
    fn start(
        &mut self,
        request: AudioRequest,
    ) -> Result<gamecap_audio::AudioSession, gamecap_types::AudioError> {
        match self.pulse.start(request.clone()) {
            Ok(session) => Ok(session),
            Err(err) => {
                eprintln!("Aviso: audio de sistema no disponible ({err}). Se grabará silencio.");
                self.use_silence = true;
                self.silence.start(request)
            }
        }
    }

    fn poll_audio(
        &mut self,
        deadline: std::time::Instant,
    ) -> Result<Option<gamecap_types::AudioBlock>, gamecap_types::AudioError> {
        if self.use_silence {
            self.silence.poll_audio(deadline)
        } else {
            self.pulse.poll_audio(deadline)
        }
    }

    fn stop(&mut self) -> Result<(), gamecap_types::AudioError> {
        if self.use_silence {
            self.silence.stop()
        } else {
            self.pulse.stop()
        }
    }
}

fn default_output_dir() -> PathBuf {
    let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join("Videos").join("GameCap")
}

fn parse_f64_flag(args: &[String], flag: &str) -> Result<Option<f64>, String> {
    for pair in args.windows(2) {
        if pair[0] == flag {
            return pair[1]
                .parse::<f64>()
                .map(Some)
                .map_err(|_| format!("invalid number for {flag}: {}", pair[1]));
        }
    }
    Ok(None)
}

fn parse_u32_flag(args: &[String], flag: &str) -> Result<Option<u32>, String> {
    for pair in args.windows(2) {
        if pair[0] == flag {
            return pair[1]
                .parse::<u32>()
                .map(Some)
                .map_err(|_| format!("invalid integer for {flag}: {}", pair[1]));
        }
    }
    Ok(None)
}

fn parse_string_flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn parse_all_string_flags<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .collect()
}
