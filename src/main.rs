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

fn try_fanotify(watch_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use nix::sys::fanotify::{EventFFlags, Fanotify, InitFlags, MarkFlags, MaskFlags};

    let watch_dir_fd =
        std::fs::File::open(watch_dir).map_err(|e| format!("Failed to open watch dir: {}", e))?;

    let fanotify_fd = Fanotify::init(
        InitFlags::FAN_NONBLOCK | InitFlags::FAN_CLOEXEC,
        EventFFlags::empty(),
    )
    .map_err(|e| format!("Failed to initialize fanotify: {}", e))?;

    fanotify_fd
        .mark(
            MarkFlags::FAN_MARK_MOUNT,
            MaskFlags::FAN_OPEN,
            &watch_dir_fd,
            None::<&Path>,
        )
        .map_err(|e| format!("Failed to mark fanotify: {}", e))?;

    info!("Using fanotify for file monitoring");
    Ok(())
}

fn run_fanotify_loop(
    watch_dir: PathBuf,
    cooldown_map: Mutex<HashMap<PathBuf, CooldownEntry>>,
) -> Result<(), Box<dyn std::error::Error>> {
    use nix::sys::fanotify::{EventFFlags, Fanotify, InitFlags, MaskFlags};
    use std::os::fd::AsRawFd;

    let watch_dir_fd =
        std::fs::File::open(&watch_dir).map_err(|e| format!("Failed to open watch dir: {}", e))?;

    let fanotify_fd = Fanotify::init(
        InitFlags::FAN_NONBLOCK | InitFlags::FAN_CLOEXEC,
        EventFFlags::empty(),
    )?;

    fanotify_fd.mark(
        nix::sys::fanotify::MarkFlags::FAN_MARK_MOUNT,
        MaskFlags::FAN_OPEN,
        &watch_dir_fd,
        None::<&Path>,
    )?;

    info!("Fanotify monitoring active");

    loop {
        let events = match fanotify_fd.read_events() {
            Ok(e) => e,
            Err(e) => {
                warn!("Error reading fanotify event: {}", e);
                continue;
            }
        };

        for event in events {
            let fd = match event.fd() {
                Some(fd) => fd,
                None => continue,
            };

            let path = match std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd())) {
                Ok(p) => p,
                Err(_) => continue,
            };

            debug!("Open event: {}", path.display());

            if !is_video_file(&path) {
                continue;
            }

            let mut cooldown = cooldown_map.lock();
            if let Some(entry) = cooldown.get(&path) {
                if entry.last_triggered.elapsed() < Duration::from_secs(COOLDOWN_SECS) {
                    debug!("In cooldown, skipping: {}", path.display());
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

            drop(cooldown);

            cache_file(path);
        }
    }
}

fn run_inotify(watch_dir: &Path, cooldown_map: Mutex<HashMap<PathBuf, CooldownEntry>>) {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    info!("Using inotify for file monitoring");

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
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start inotifywait");

    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let reader = std::io::BufReader::new(stdout);

    for line in reader.lines().map_while(Result::ok) {
        let path = PathBuf::from(&line);

        if !is_video_file(&path) {
            continue;
        }

        let mut cooldown = cooldown_map.lock();
        if let Some(entry) = cooldown.get(&path) {
            if entry.last_triggered.elapsed() < Duration::from_secs(COOLDOWN_SECS) {
                debug!("In cooldown, skipping: {}", path.display());
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

        drop(cooldown);

        cache_file(path);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
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

    match try_fanotify(&watch_dir) {
        Ok(_) => {
            if let Err(e) = run_fanotify_loop(watch_dir, cooldown_map) {
                error!("Fanotify loop error: {}", e);
            }
        }
        Err(e) => {
            warn!("Fanotify not available ({}), falling back to inotify", e);
            run_inotify(&watch_dir, cooldown_map);
        }
    }
}
