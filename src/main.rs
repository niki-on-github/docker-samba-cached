use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "ts", "m4v", "mks"];
const WATCH_DELAY_MS: u64 = 1000;

fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn is_file_still_open(path: &Path) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let proc_self_fd = PathBuf::from("/proc/self/fd");
    if let Ok(entries) = std::fs::read_dir(&proc_self_fd) {
        for entry in entries.flatten() {
            if let Ok(link) = std::fs::read_link(entry.path()) {
                if link == canonical {
                    return true;
                }
            }
        }
    }
    false
}

fn cache_file(path: PathBuf) {
    let path_clone = path.clone();

    thread::spawn(move || {
        thread::sleep(Duration::from_millis(WATCH_DELAY_MS));

        if !is_file_still_open(&path_clone) {
            debug!(
                "[CACHE] File no longer open, skipping: {}",
                path_clone.display()
            );
            return;
        }

        if !path_clone.exists() {
            debug!(
                "[CACHE] File no longer exists, skipping: {}",
                path_clone.display()
            );
            return;
        }

        info!("[CACHE] Loading: {}", path_clone.display());

        if let Err(e) = Command::new("vmtouch")
            .arg("-t")
            .arg(path_clone.as_os_str())
            .spawn()
        {
            warn!("[CACHE] Failed to spawn vmtouch: {}", e);
        }
    });
}

fn run_inotify(watch_dir: &Path) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    info!("[INOTIFY] Starting monitoring on: {}", watch_dir.display());

    let mut child = Command::new("inotifywait")
        .args([
            "-m",
            "-e",
            "open",
            "--format",
            "%w%f",
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
                    debug!("[INOTIFY] stderr: {}", line);
                }
            }
        }
    });

    let reader = std::io::BufReader::new(stdout);

    for line in reader.lines().map_while(Result::ok) {
        let path = PathBuf::from(&line);

        if !is_video_file(&path) {
            continue;
        }

        debug!("[INOTIFY] Video file read: {}", path.display());
        cache_file(path);
    }
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
