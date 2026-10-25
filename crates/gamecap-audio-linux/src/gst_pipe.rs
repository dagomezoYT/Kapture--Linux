//! Spawn `gst-launch-1.0` with a dedicated UnixStream for raw PCM.

use std::io::{self, Read};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use rustix::io::{fcntl_setfd, FdFlags};

pub struct GstRawPipe {
    pub child: Child,
    pub reader: UnixStream,
}

pub fn spawn_gst_pipeline(tokens: &[String]) -> Result<GstRawPipe, String> {
    let gst = which("gst-launch-1.0")?;
    let (reader, writer) =
        UnixStream::pair().map_err(|e| format!("unix socket pair: {e}"))?;
    fcntl_setfd(writer.as_fd(), FdFlags::empty()).map_err(|e| format!("fcntl: {e}"))?;
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
        .spawn()
        .map_err(|e| format!("failed to start gst-launch-1.0: {e}"))?;
    drop(writer);
    std::thread::sleep(Duration::from_millis(80));
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        return Err(format!(
            "gst-launch-1.0 exited immediately ({status}): {err}"
        ));
    }
    let _ = reader.set_read_timeout(Some(Duration::from_millis(200)));
    Ok(GstRawPipe { child, reader })
}

pub fn read_exact_timeout(reader: &mut UnixStream, buf: &mut [u8]) -> io::Result<()> {
    reader.read_exact(buf)
}

pub fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
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
