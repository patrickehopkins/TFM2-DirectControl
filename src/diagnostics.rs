//! Automatic, bounded support diagnostics for ordinary Workshop installs.
//!
//! The stable host logger is available in init; render-time evidence is written to one
//! companion file beside the game's log.log. No special launch flags or key combos.
//! Only Harbinger state and its own shortcut attempts are recorded.
use std::{
    env, fs::{self, File, OpenOptions}, io::{self, Write}, path::PathBuf,
    sync::{atomic::{AtomicU64, Ordering}, Mutex, OnceLock}, time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 1_048_576;
static FILE: OnceLock<Mutex<File>> = OnceLock::new();
static LAST_HEARTBEAT_SECOND: AtomicU64 = AtomicU64::new(0);

fn timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

pub fn initialize() -> Result<PathBuf, String> {
    let appdata = env::var_os("APPDATA").ok_or("APPDATA environment variable not set")?;
    let folder = PathBuf::from(appdata).join("TeamSamoyed").join("TeamfightManager2").join("data");
    fs::create_dir_all(&folder).map_err(|error| format!("create diagnostic folder: {error}"))?;
    let path = folder.join("harbinger-diagnostics.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() >= MAX_LOG_BYTES) {
        let previous = folder.join("harbinger-diagnostics.previous.log");
        let _ = fs::remove_file(&previous);
        // Failure to rotate must not disable logging or gameplay.
        let _ = fs::rename(&path, &previous);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)
        .map_err(|error| format!("open diagnostic file: {error}"))?;
    writeln!(file, "\n=== Harbinger v{} process started at unix={} ===", env!("CARGO_PKG_VERSION"), timestamp())
        .map_err(|error| format!("write diagnostic session header: {error}"))?;
    file.flush().map_err(|error| format!("flush diagnostic session header: {error}"))?;
    FILE.set(Mutex::new(file)).map_err(|_| "diagnostics already initialized".to_owned())?;
    Ok(path)
}

pub fn event(message: &str) {
    let Some(file) = FILE.get() else { return; };
    let Ok(mut file) = file.lock() else { return; };
    let _ = writeln!(file, "[{}] {}", timestamp(), message);
    // Preserve the last event even if the game hangs or exits unexpectedly.
    let _ = io::Write::flush(&mut *file);
}

pub fn heartbeat_if_due(message: impl FnOnce() -> String) {
    let now = timestamp();
    let last = LAST_HEARTBEAT_SECOND.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 15 { return; }
    if LAST_HEARTBEAT_SECOND.compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
        event(&message());
    }
}
