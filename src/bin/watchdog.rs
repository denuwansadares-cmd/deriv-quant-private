use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime},
};
const HEARTBEAT_NAME: &str = "runtime-heartbeat";
const STOP_NAME: &str = "emergency.stop";
const STATE_NAME: &str = "stop-state.json";
const LOCK_NAME: &str = "runtime.lock";
fn dir() -> PathBuf {
    std::env::var("DERIV_RISK_STATE_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".risk"))
}
fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".watchdog.tmp-{}", std::process::id()));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(temp, path)?;
    fs::File::open(parent)?.sync_all()
}
fn terminate_runtime(pid: u32) {
    let value = pid.to_string();
    let _ = Command::new("kill").args(["-TERM", &value]).status();
    thread::sleep(Duration::from_secs(2));
    if Path::new(&format!("/proc/{pid}")).exists() {
        let _ = Command::new("kill").args(["-KILL", &value]).status();
    }
}
fn main() -> std::io::Result<()> {
    let stale_secs = std::env::var("DERIV_WATCHDOG_STALE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10)
        .max(2);
    fs::create_dir_all(dir())?;
    loop {
        let heartbeat = dir().join(HEARTBEAT_NAME);
        let stale = fs::metadata(&heartbeat)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_none_or(|age| age >= Duration::from_secs(stale_secs));
        if stale {
            let reason = format!("watchdog: runtime heartbeat stale for {stale_secs}s");
            atomic(&dir().join(STOP_NAME), reason.as_bytes())?;
            atomic(
                &dir().join(STATE_NAME),
                format!(r#"{{"stopped":true,"reason":"{reason}"}}"#).as_bytes(),
            )?;
            if let Ok(pid) = fs::read_to_string(dir().join(LOCK_NAME))
                .unwrap_or_default()
                .trim()
                .parse::<u32>()
            {
                if pid > 1 {
                    terminate_runtime(pid)
                }
            }
            eprintln!("watchdog latched stop and terminated trading runtime");
            return Ok(());
        }
        if dir().join(STOP_NAME).exists() {
            if let Ok(pid) = fs::read_to_string(dir().join(LOCK_NAME))
                .unwrap_or_default()
                .trim()
                .parse::<u32>()
            {
                if pid > 1 {
                    terminate_runtime(pid)
                }
            }
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}
