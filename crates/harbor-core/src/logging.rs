use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::io::AsyncReadExt;
use tracing::{info, warn};
use tracing_subscriber::fmt::writer::MakeWriter;

/// 单个活跃日志文件上限（2 MiB）；滚动只保留当前文件与单份 `.old`，
/// 运行期总占用恒定不超过 4 MiB，彻底杜绝长期常驻撑满闪存或 tmpfs。
pub const MAX_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;

static ACTIVE_LOG_WRITER: OnceLock<Arc<BoundedLogWriter>> = OnceLock::new();

pub fn set_active_log_writer(writer: Arc<BoundedLogWriter>) {
    let _ = ACTIVE_LOG_WRITER.set(writer);
}

pub fn active_log_writer() -> Option<Arc<BoundedLogWriter>> {
    ACTIVE_LOG_WRITER.get().cloned()
}

pub fn rotated_log_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".old");
    path.with_file_name(name)
}

pub fn rotate_oversized_log(path: &Path) {
    rotate_oversized_log_with_limit(path, MAX_LOG_FILE_BYTES);
}

fn rotate_oversized_log_with_limit(path: &Path, max_bytes: u64) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() <= max_bytes {
        return;
    }
    let rotated = rotated_log_path(path);
    match fs::rename(path, &rotated) {
        Ok(()) => info!(log = ?path, rotated_to = ?rotated, "Rotated oversized log file"),
        Err(err) => warn!(log = ?path, error = %err, "Failed to rotate oversized log file"),
    }
}

struct BoundedState {
    path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    current_len: u64,
    mirror_tty: bool,
}

pub struct BoundedLogWriter {
    state: Mutex<BoundedState>,
}

impl BoundedLogWriter {
    pub fn open(path: PathBuf) -> io::Result<Arc<Self>> {
        Self::open_with_limit(path, MAX_LOG_FILE_BYTES, io::stdout().is_terminal())
    }

    pub fn open_with_limit(
        path: PathBuf,
        max_bytes: u64,
        mirror_tty: bool,
    ) -> io::Result<Arc<Self>> {
        let max_bytes = max_bytes.max(1024);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        rotate_oversized_log_with_limit(&path, max_bytes);
        let file = open_private_append(&path)?;
        let current_len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Arc::new(Self {
            state: Mutex::new(BoundedState {
                path,
                max_bytes,
                file: Some(file),
                current_len,
                mirror_tty,
            }),
        }))
    }

    pub fn write_bytes(&self, buf: &[u8]) {
        if buf.is_empty() {
            return;
        }
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.mirror_tty {
            let _ = io::stdout().write_all(buf);
        }
        let max_bytes = state.max_bytes as usize;
        let slice = if buf.len() > max_bytes {
            &buf[buf.len() - max_bytes..]
        } else {
            buf
        };
        if state.current_len > 0
            && state.current_len.saturating_add(slice.len() as u64) > state.max_bytes
        {
            rotate_in_place(&mut state);
        }
        if state.file.is_none() {
            if let Some(parent) = state.path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(file) = open_private_append(&state.path) {
                state.current_len = file.metadata().map(|m| m.len()).unwrap_or(0);
                state.file = Some(file);
            }
        }
        if let Some(ref mut file) = state.file {
            match file.write_all(slice) {
                Ok(()) => {
                    state.current_len = state.current_len.saturating_add(slice.len() as u64);
                }
                Err(_) => {
                    state.file = None;
                }
            }
        }
    }

    pub fn flush_file(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if let Some(ref mut file) = state.file {
            let _ = file.flush();
        }
    }
}

fn open_private_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

fn rotate_in_place(state: &mut BoundedState) {
    if let Some(mut old_file) = state.file.take() {
        let _ = old_file.flush();
    }
    let rotated = rotated_log_path(&state.path);
    let _ = fs::rename(&state.path, &rotated);
    state.current_len = 0;
}

#[derive(Clone)]
pub struct BoundedWriterHandle(pub Arc<BoundedLogWriter>);

impl Write for BoundedWriterHandle {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_bytes(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush_file();
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for BoundedWriterHandle {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        Self(self.0.clone())
    }
}

pub fn spawn_child_log_drains(child: &mut tokio::process::Child, writer: Arc<BoundedLogWriter>) {
    if let Some(stdout) = child.stdout.take() {
        let writer = writer.clone();
        tokio::spawn(async move {
            drain_stream(stdout, writer).await;
        });
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            drain_stream(stderr, writer).await;
        });
    }
}

async fn drain_stream<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    writer: Arc<BoundedLogWriter>,
) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => writer.write_bytes(&buf[..n]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_writer_rotates_live_and_caps_total_disk_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("harbor.log");
        let rotated_path = rotated_log_path(&log_path);
        let limit = 2048_u64;

        let writer = BoundedLogWriter::open_with_limit(log_path.clone(), limit, false).unwrap();
        let chunk = vec![b'a'; 600];
        // Write 15 * 600 = 9000 bytes (> 4x limit) across multiple rotations
        for _ in 0..15 {
            writer.write_bytes(&chunk);
        }
        writer.flush_file();

        let active_len = fs::metadata(&log_path).unwrap().len();
        let rotated_len = fs::metadata(&rotated_path).unwrap().len();
        assert!(active_len <= limit, "active {active_len} > {limit}");
        assert!(rotated_len <= limit, "rotated {rotated_len} > {limit}");
        assert!(active_len > 0);
        assert!(rotated_len > 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&log_path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn child_log_drains_pipe_into_bounded_writer_and_rotate() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("child.log");
        let rotated_path = rotated_log_path(&log_path);
        let limit = 1024_u64;

        let writer = BoundedLogWriter::open_with_limit(log_path.clone(), limit, false).unwrap();
        #[cfg(unix)]
        let mut child = tokio::process::Command::new("sh")
            .args([
                "-c",
                "i=0; while [ $i -lt 80 ]; do echo '0123456789abcdef0123456789abcdef'; i=$((i+1)); done",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let mut child = tokio::process::Command::new("cmd.exe")
            .args([
                "/C",
                "for /L %i in (1,1,80) do @echo 0123456789abcdef0123456789abcdef",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        spawn_child_log_drains(&mut child, writer.clone());
        let status = child.wait().await.unwrap();
        assert!(status.success());
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert!(fs::metadata(&log_path).unwrap().len() <= limit);
        assert!(fs::metadata(&rotated_path).unwrap().len() <= limit);
    }
}
