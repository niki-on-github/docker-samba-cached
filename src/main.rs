use parking_lot::Mutex;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

const VIDEO_EXTENSIONS: &[&str] = &["mkv", "mp4", "avi", "ts", "m4v", "mks"];
const COOLDOWN_SECS: u64 = 30;
const WATCH_DELAY_MS: u64 = 1000;

struct CooldownEntry {
    last_triggered: Instant,
}

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
                "File no longer open, skipping cache: {}",
                path_clone.display()
            );
            return;
        }

        if !path_clone.exists() {
            debug!(
                "File no longer exists, skipping cache: {}",
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
            warn!("Failed to spawn vmtouch: {}", e);
        }
    });
}

fn run_inotify(watch_dir: &Path, cooldown_map: Mutex<HashMap<PathBuf, CooldownEntry>>) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    info!("Starting inotify monitoring on: {}", watch_dir.display());

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
        .expect("Failed to start inotifywait");

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take();

    let stderr_handle = thread::spawn(move || {
        if let Some(stderr) = stderr {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if !line.is_empty() {
                    debug!("inotifywait: {}", line);
                }
            }
        }
    });

    let reader = std::io::BufReader::new(stdout);

    for line in reader.lines().map_while(Result::ok) {
        let path = PathBuf::from(&line);

        debug!("[INOTIFY] Open event: {}", path.display());

        if !is_video_file(&path) {
            debug!("[INOTIFY] Skipping non-video file: {}", path.display());
            continue;
        }

        let mut cooldown = cooldown_map.lock();
        if let Some(entry) = cooldown.get(&path) {
            if entry.last_triggered.elapsed() < Duration::from_secs(COOLDOWN_SECS) {
                debug!("[INOTIFY] In cooldown, skipping: {}", path.display());
                continue;
            }
        }

        cooldown.insert(
            path.clone(),
            CooldownEntry {
                last_triggered: Instant::now(),
            },
        );

        if cooldown.len() > 100 {
            cooldown.retain(|_, v| v.last_triggered.elapsed() < Duration::from_secs(60));
        }

        let file_to_cache = path.clone();
        drop(cooldown);

        info!("[INOTIFY] Video file opened: {}", file_to_cache.display());
        cache_file(file_to_cache);
    }

    let _ = stderr_handle.join();
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
            info!("CACHE_WORK_DIR: {}", dir);
            PathBuf::from(dir)
        }
        Err(_) => {
            error!("CACHE_WORK_DIR environment variable not set");
            thread::sleep(Duration::from_secs(60));
            std::process::exit(1);
        }
    };

    if !watch_dir.exists() {
        error!("CACHE_WORK_DIR does not exist: {}", watch_dir.display());
        thread::sleep(Duration::from_secs(60));
        std::process::exit(1);
    }

    info!("Starting Media RAM Cacher");
    info!("Watching: {}", watch_dir.display());

    let cooldown_map: Mutex<HashMap<PathBuf, CooldownEntry>> = Mutex::new(HashMap::new());

    run_inotify(&watch_dir, cooldown_map);
}
