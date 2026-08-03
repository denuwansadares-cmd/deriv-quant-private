use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct RuntimeIdentity {
    pid: u32,
    process_start_ticks: u64,
    boot_id: String,
}

fn dir() -> PathBuf {
    std::env::var("DERIV_RISK_STATE_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".risk"))
}
fn process_start_ticks(pid: u32) -> std::io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let tail = stat
        .rsplit_once(')')
        .map(|(_, tail)| tail.trim())
        .ok_or_else(|| std::io::Error::other("invalid proc stat"))?;
    tail.split_whitespace()
        .nth(19)
        .ok_or_else(|| std::io::Error::other("missing proc start time"))?
        .parse()
        .map_err(|_| std::io::Error::other("invalid proc start time"))
}
fn identity_is_active(identity: &RuntimeIdentity) -> bool {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .is_some_and(|value| value.trim() == identity.boot_id)
        && process_start_ticks(identity.pid).ok() == Some(identity.process_start_ticks)
}
fn read_identity() -> Option<RuntimeIdentity> {
    fs::read_to_string(dir().join(LOCK_NAME))
        .ok()
        .and_then(|value| serde_json::from_str(&value).ok())
}
fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".watchdog.tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn terminate_runtime(identity: &RuntimeIdentity) {
    if !identity_is_active(identity) {
        return;
    }
    let value = identity.pid.to_string();
    let _ = Command::new("kill").args(["-TERM", &value]).status();
    thread::sleep(Duration::from_secs(2));
    if identity_is_active(identity) {
        let _ = Command::new("kill").args(["-KILL", &value]).status();
    }
}
fn latch_stop(reason: &str) -> std::io::Result<()> {
    atomic(&dir().join(STOP_NAME), reason.as_bytes())?;
    atomic(
        &dir().join(STATE_NAME),
        serde_json::json!({"stopped":true,"reason":reason})
            .to_string()
            .as_bytes(),
    )
}
fn main() -> std::io::Result<()> {
    let stale_secs = std::env::var("DERIV_WATCHDOG_STALE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10)
        .max(2);
    fs::create_dir_all(dir())?;
    loop {
        let heartbeat = dir().join(HEARTBEAT_NAME);
        let stale = fs::metadata(&heartbeat)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_none_or(|age| age >= Duration::from_secs(stale_secs));
        if stale {
            let reason = format!("watchdog: runtime heartbeat stale for {stale_secs}s");
            latch_stop(&reason)?;
            if let Some(identity) = read_identity() {
                terminate_runtime(&identity);
            }
            eprintln!("watchdog latched stop after stale runtime heartbeat");
            return Ok(());
        }
        if dir().join(STOP_NAME).exists() {
            if let Some(identity) = read_identity() {
                terminate_runtime(&identity);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_identity_matches_but_forged_identity_does_not() {
        let pid = std::process::id();
        let identity = RuntimeIdentity {
            pid,
            process_start_ticks: process_start_ticks(pid).unwrap(),
            boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
                .to_string(),
        };
        assert!(identity_is_active(&identity));
        let mut forged = identity;
        forged.process_start_ticks = forged.process_start_ticks.saturating_add(1);
        assert!(!identity_is_active(&forged));
    }
}
