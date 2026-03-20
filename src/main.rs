use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "ts", "m4v", "mks"];
const OPEN_TIMEOUT_MS: u64 = 500;
const CACHE_COOLDOWN_MS: u64 = 30000;

struct OpenTracker {
    pending: HashMap<PathBuf, Instant>,
    recently_cached: HashMap<PathBuf, Instant>,
    timeout: Duration,
}

impl OpenTracker {
    fn new(timeout_ms: u64) -> Self {
        Self {
            pending: HashMap::new(),
            recently_cached: HashMap::new(),
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    fn cleanup_stale_cache_entries(&mut self) {
        let cutoff = Instant::now() - Duration::from_millis(CACHE_COOLDOWN_MS);
        self.recently_cached
            .retain(|_, cached_at| *cached_at > cutoff);
    }

    fn on_open(&mut self, path: PathBuf) {
        self.cleanup_stale_cache_entries();

        if let Some(cached_at) = self.recently_cached.get(&path) {
            let elapsed = Instant::now().duration_since(*cached_at);
            if elapsed < Duration::from_millis(CACHE_COOLDOWN_MS) {
                return;
            }
        }

        self.pending.insert(path, Instant::now());
    }

    fn on_close(&mut self, path: &Path) {
        self.pending.remove(path);
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
            self.recently_cached.insert(path.clone(), Instant::now());
            cache_file(path);
        }
    }
}

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn cache_file(path: &Path) {
    info!("[CACHE] Loading: {}", path.display());

    match Command::new("vmtouch")
        .arg("-t")
        .arg(path.as_os_str())
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
            "close_write",
            "-e",
            "close_nowrite",
            "--format",
            "%w%f|%e",
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
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (path, event) = match trimmed.find('|') {
            Some(pipe_pos) => {
                let path = PathBuf::from(trimmed[..pipe_pos].trim_end_matches('/'));
                let event = trimmed[pipe_pos + 1..].trim();
                (path, event)
            }
            None => {
                warn!("[INOTIFY] malformed line (no | separator): {}", trimmed);
                continue;
            }
        };

        if !is_video_file(&path) {
            continue;
        }

        let mut tracker = tracker.lock().unwrap();

        let primary_event = event.split(',').next().unwrap_or(event);
        match primary_event {
            "OPEN" => {
                //info!("[INOTIFY] File opened: {}", path.display());
                tracker.on_open(path);
            }
            "CLOSE_NOWRITE" | "CLOSE_WRITE" => {
                //info!("[INOTIFY] File closed: {}", path.display());
                tracker.on_close(&path);
            }
            _ => {
                warn!("[INOTIFY] Unknown event: {}", event);
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
