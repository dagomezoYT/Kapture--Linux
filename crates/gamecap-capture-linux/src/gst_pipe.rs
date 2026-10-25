//! Spawn `gst-launch-1.0` with inherited fds and a dedicated UnixStream for raw samples.

use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustix::io::{fcntl_setfd, FdFlags};

/// Large enough for a 4K BGRA frame so fdsink does not stall PipeWire.
const UNIX_BUF_BYTES: usize = 16 * 1024 * 1024;

pub struct GstRawPipe {
    pub child: Child,
    pub reader: UnixStream,
    pub prefix: Vec<u8>,
    pub(crate) stderr: Arc<Mutex<String>>,
}

pub fn spawn_gst_pipeline(
    tokens: &[String],
    inherit: &[BorrowedFd<'_>],
) -> Result<GstRawPipe, String> {
    let gst = which("gst-launch-1.0")?;
    let (reader, writer) = UnixStream::pair().map_err(|e| format!("unix socket pair: {e}"))?;
    enlarge_unix_buffers(&reader, &writer);
    clear_cloexec(writer.as_fd())?;
    for fd in inherit {
        clear_cloexec(*fd)?;
    }
    let sink_fd = writer.as_raw_fd();
    let mut args = vec!["-q".to_string()];
    args.extend(tokens.iter().cloned());
    for arg in &mut args {
        if arg.contains("$SINK") {
            *arg = arg.replace("$SINK", &sink_fd.to_string());
        }
    }
    let mut child = Command::new(&gst)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("GST_DEBUG", "0")
        .env("GST_PLUGIN_SYSTEM_PATH_1_0", "/usr/lib64/gstreamer-1.0")
        .env("PIPEWIRE_QUANTUM", "128/48000")
        .spawn()
        .map_err(|e| format!("failed to start gst-launch-1.0: {e}"))?;
    drop(writer);
    let stderr = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = child.stderr.take() {
        let buf = Arc::clone(&stderr);
        thread::spawn(move || {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text);
            if let Ok(mut slot) = buf.lock() {
                *slot = text;
            }
        });
    }
    // Caps negotiation can take longer than a single short sleep.
    for _ in 0..8 {
        thread::sleep(Duration::from_millis(50));
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(gst_exit_message(status, &stderr));
        }
    }
    let _ = reader.set_read_timeout(Some(Duration::from_millis(20)));
    Ok(GstRawPipe {
        child,
        reader,
        prefix: Vec::new(),
        stderr,
    })
}

/// Spawn and require at least one byte (or keep the process if it stays healthy).
pub fn spawn_gst_pipeline_ready(
    tokens: &[String],
    inherit: &[BorrowedFd<'_>],
    wait: Duration,
) -> Result<GstRawPipe, String> {
    let mut pipe = spawn_gst_pipeline(tokens, inherit)?;
    let deadline = Instant::now() + wait;
    let mut first = [0u8; 1];
    loop {
        if let Some(status) = pipe.child.try_wait().map_err(|e| e.to_string())? {
            return Err(gst_exit_message(status, &pipe.stderr));
        }
        match pipe.reader.read(&mut first) {
            Ok(0) => {
                let _ = pipe.child.try_wait();
                return Err(gst_exit_message_or(
                    &pipe.stderr,
                    "gstreamer socket closed before the first frame",
                ));
            }
            Ok(_) => {
                pipe.prefix.push(first[0]);
                return Ok(pipe);
            }
            Err(err)
                if err.kind() == io::ErrorKind::TimedOut
                    || err.kind() == io::ErrorKind::WouldBlock =>
            {
                if Instant::now() >= deadline {
                    stop_child(&mut pipe.child);
                    return Err(gst_exit_message_or(
                        &pipe.stderr,
                        "gstreamer produced no data",
                    ));
                }
            }
            Err(err) => {
                stop_child(&mut pipe.child);
                return Err(format!("gstreamer read: {err}"));
            }
        }
    }
}

fn gst_exit_message(status: std::process::ExitStatus, stderr: &Arc<Mutex<String>>) -> String {
    let text = stderr
        .lock()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(sin stderr)".to_string());
    format!("gst-launch-1.0 exited ({status}): {text}")
}

fn gst_exit_message_or(stderr: &Arc<Mutex<String>>, fallback: &str) -> String {
    let text = stderr
        .lock()
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    match text {
        Some(text) => format!("{fallback}: {text}"),
        None => fallback.to_string(),
    }
}

/// Fill `buf` even if individual reads time out mid-frame (avoids stream desync).
pub fn read_exact_timeout(reader: &mut UnixStream, buf: &mut [u8]) -> io::Result<()> {
    let mut off = 0usize;
    while off < buf.len() {
        match reader.read(&mut buf[off..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "gstreamer socket closed",
                ));
            }
            Ok(n) => off += n,
            Err(err)
                if err.kind() == io::ErrorKind::TimedOut
                    || err.kind() == io::ErrorKind::WouldBlock =>
            {
                if off == 0 {
                    return Err(err);
                }
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

pub fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn enlarge_unix_buffers(reader: &UnixStream, writer: &UnixStream) {
    let _ = rustix::net::sockopt::set_socket_recv_buffer_size(reader, UNIX_BUF_BYTES);
    let _ = rustix::net::sockopt::set_socket_send_buffer_size(reader, UNIX_BUF_BYTES);
    let _ = rustix::net::sockopt::set_socket_recv_buffer_size(writer, UNIX_BUF_BYTES);
    let _ = rustix::net::sockopt::set_socket_send_buffer_size(writer, UNIX_BUF_BYTES);
}

fn which(bin: &str) -> Result<String, String> {
    if let Ok(p) = std::env::var("PATH") {
        for dir in p.split(':') {
            let candidate = std::path::Path::new(dir).join(bin);
            if candidate.is_file() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }
    let fallback = std::path::PathBuf::from("/usr/bin").join(bin);
    if fallback.is_file() {
        return Ok(fallback.to_string_lossy().into_owned());
    }
    Err(format!("{bin} not found in PATH"))
}

fn clear_cloexec(fd: impl AsFd) -> Result<(), String> {
    fcntl_setfd(fd, FdFlags::empty()).map_err(|e| format!("fcntl: {e}"))
}

/// True when GStreamer can encode AV1 on the AMD display GPU.
pub fn gst_vaav1enc_available() -> bool {
    gst_element_available("vaav1enc", "VA-API")
}

/// True when GStreamer can load NVIDIA NVENC (`nvh264enc`).
pub fn gst_nvh264enc_available() -> bool {
    gst_element_available("nvh264enc", "NVENC")
}

fn gst_element_available(element: &str, needle: &str) -> bool {
    let inspect = match which("gst-inspect-1.0") {
        Ok(p) => p,
        Err(_) => return false,
    };
    let out = Command::new(inspect)
        .arg(element)
        .env("GST_DEBUG", "0")
        .env("GST_PLUGIN_SYSTEM_PATH_1_0", "/usr/lib64/gstreamer-1.0")
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains(element) && text.contains(needle)
        }
        _ => false,
    }
}
