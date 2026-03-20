use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "ts", "m4v", "mks"];
const OPEN_TIMEOUT_MS: u64 = 500;

struct OpenTracker {
    pending: HashMap<PathBuf, Instant>,
    timeout: Duration,
}

impl OpenTracker {
    fn new(timeout_ms: u64) -> Self {
        Self {
            pending: HashMap::new(),
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    fn on_open(&mut self, path: PathBuf) {
        if !path.exists() {
            info!(
                "[TRACKER] Ignoring open for non-existent file: {}",
                path.display()
            );
            return;
        }

        self.pending.insert(path.clone(), Instant::now());
        debug!("[TRACKER] File opened (timer started): {}", path.display());
    }

    fn on_close(&mut self, path: &Path) {
        if self.pending.remove(path).is_some() {
            info!(
                "[TRACKER] File closed before timeout, dropped: {}",
                path.display()
            );
        }
    }

    fn check_and_cache_timed_out(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, started)| now.duration_since(**started) >= self.timeout)
            .map(|(path, _)| path.clone())
            .collect();

        for path in &timed_out {
            self.pending.remove(path);
            info!("[TRACKER] File timed out, caching: {}", path.display());
            cache_file(path.clone());
        }
    }
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn cache_file(path: PathBuf) {
    let path_clone = path.clone();

    thread::spawn(move || {
        if !path_clone.exists() {
            info!(
                "[CACHE] File no longer exists, skipping: {}",
                path_clone.display()
            );
            return;
        }

        info!("[CACHE] Loading: {}", path_clone.display());

        match Command::new("vmtouch")
            .arg("-t")
            .arg(path_clone.as_os_str())
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    info!(
                        "[CACHE] vmtouch: {}",
                        String::from_utf8_lossy(&output.stdout).trim()
                    );
                } else {
                    warn!(
                        "[CACHE] vmtouch failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
            }
            Err(e) => warn!("[CACHE] Failed to run vmtouch: {}", e),
        }
    });
}

fn run_inotify(watch_dir: &Path) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    info!("[INOTIFY] Starting monitoring on: {}", watch_dir.display());

    let tracker = Arc::new(Mutex::new(OpenTracker::new(OPEN_TIMEOUT_MS)));

    let mut child = Command::new("inotifywait")
        .args([
            "-m",
            "-e",
            "open",
            "-e",
            "close",
            "--format",
            "%w%f%n%e",
            "-r",
            watch_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("[INOTIFY] Failed to start inotifywait");

    let stdout = child
        .stdout
        .take()
        .expect("[INOTIFY] Failed to capture stdout");
    let stderr = child.stderr.take();

    let _stderr_handle = thread::spawn(move || {
        if let Some(stderr) = stderr {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if !line.is_empty() {
                    info!("[INOTIFY] stderr: {}", line);
                }
            }
        }
    });

    let tracker_clone = Arc::clone(&tracker);
    let check_handle = thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(100));
        let mut tracker = tracker_clone.lock().unwrap();
        tracker.check_and_cache_timed_out();
    });

    let reader = std::io::BufReader::new(stdout);

    for line in reader.lines().map_while(Result::ok) {
        info!("[INOTIFY] event: {}", line);

        if let Some(null_pos) = line.find('\0') {
            let path = PathBuf::from(line[..null_pos].trim_end_matches('/'));
            let event = &line[null_pos + 1..];

            if !is_video_file(&path) {
                continue;
            }

            let mut tracker = tracker.lock().unwrap();
            tracker.check_and_cache_timed_out();

            match event {
                "OPEN" => {
                    info!("[INOTIFY] File opened: {}", path.display());
                    tracker.on_open(path);
                }
                "CLOSE_NOWRITE" | "CLOSE_WRITE" => {
                    tracker.on_close(&path);
                }
                _ => {}
            }
        }
    }

    check_handle.join().ok();
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::DEBUG.into()),
        )
        .init();

    let watch_dir = match env::var("CACHE_WORK_DIR") {
        Ok(dir) => {
            info!("[CONFIG] CACHE_WORK_DIR: {}", dir);
            PathBuf::from(dir)
        }
        Err(_) => {
            error!("[CONFIG] CACHE_WORK_DIR environment variable not set");
            thread::sleep(Duration::from_secs(60));
            std::process::exit(1);
        }
    };

    if !watch_dir.exists() {
        error!(
            "[CONFIG] CACHE_WORK_DIR does not exist: {}",
            watch_dir.display()
        );
        thread::sleep(Duration::from_secs(60));
        std::process::exit(1);
    }

    info!("[MAIN] Starting Media RAM Cacher");
    info!("[MAIN] Watching: {}", watch_dir.display());

    run_inotify(&watch_dir);
}
