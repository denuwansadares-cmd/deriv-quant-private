use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    process::{Child, Command},
    signal::unix::{signal, SignalKind},
    time::sleep,
};

#[derive(Serialize)]
struct Status<'a> {
    state: &'a str,
    at_unix: u64,
}
fn dir() -> PathBuf {
    std::env::var_os("DERIV_RISK_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| ".risk".into())
}
fn atomic(path: &Path, body: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".supervisor-{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(body)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn status(dir: &Path, state: &str) -> std::io::Result<()> {
    let value = Status {
        state,
        at_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    atomic(
        &dir.join("control-status.json"),
        serde_json::to_string(&value)?.as_bytes(),
    )
}
fn limit(dir: &Path, name: &str, default: Option<&str>) -> std::io::Result<String> {
    match fs::read_to_string(dir.join(name)) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().into()),
        _ if default.is_some() => Ok(default.unwrap_or_default().into()),
        Err(e) => Err(e),
        _ => Err(std::io::Error::other("empty risk limit")),
    }
}
fn blocked(dir: &Path) -> bool {
    dir.join("emergency.stop").exists() || dir.join("stop-state.json").exists()
}
fn launch(dir: &Path, mode: &'static str) -> std::io::Result<Child> {
    let stake = limit(
        dir,
        "configured-stake-minor",
        (mode == "demo").then_some("100"),
    )?;
    let loss = limit(
        dir,
        "configured-loss-limit-bps",
        (mode == "demo").then_some("100"),
    )?;
    let history = std::env::var("HISTORY_PATH").unwrap_or_else(|_| "/history/normalized".into());
    let mut command = Command::new("deriv_bot");
    command
        .args(["--mode", mode, "--history", &history])
        .env("DERIV_MAX_STAKE_MINOR", stake)
        .env("DERIV_SESSION_LOSS_LIMIT_BPS", loss)
        .stdin(Stdio::null());
    if mode == "live" {
        command.env("DERIV_ENABLE_LIVE_TRADING", "I_UNDERSTAND_REAL_MONEY");
    }
    command.spawn()
}
async fn stop(
    child: &mut Option<Child>,
    mode: &mut Option<&'static str>,
    dir: &Path,
    archive_demo: bool,
) {
    if let Some(process) = child.as_mut() {
        let _ = Command::new("kill")
            .args(["-INT", &process.id().unwrap_or_default().to_string()])
            .status()
            .await;
        let _ = process.wait().await;
    }
    *child = None;
    if archive_demo && *mode == Some("demo") {
        let old = dir.join("stop-state.json");
        if old.exists() {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let _ = fs::rename(old, dir.join(format!("stop-state.graceful-{stamp}.json")));
        }
    }
    *mode = None;
    let _ = status(dir, "idle");
}
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    let desired = dir.join("desired-demo-mode");
    let mut child = None;
    let mut mode = None;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    status(&dir, "idle")?;
    loop {
        tokio::select! { _ = interrupt.recv() => { stop(&mut child, &mut mode, &dir, false).await; break; }, _ = terminate.recv() => { stop(&mut child, &mut mode, &dir, false).await; break; }, _ = sleep(Duration::from_secs(1)) => {} }
        if dir.join("control-stop-demo").exists() {
            let _ = fs::remove_file(dir.join("control-stop-demo"));
            let _ = fs::remove_file(&desired);
            stop(&mut child, &mut mode, &dir, true).await;
        }
        if child
            .as_mut()
            .is_some_and(|p: &mut Child| p.try_wait().ok().flatten().is_some())
        {
            child = None;
            mode = None;
            status(&dir, "idle")?;
        }
        if dir.join("control-start-demo").exists() {
            let _ = fs::remove_file(dir.join("control-start-demo"));
            atomic(&desired, b"demo\n")?;
        }
        if desired.exists() && child.is_none() {
            if blocked(&dir) {
                status(&dir, "blocked_by_safety_stop")?;
            } else {
                child = Some(launch(&dir, "demo")?);
                mode = Some("demo");
                status(&dir, "starting")?;
            }
        }
        if dir.join("control-start-live").exists() {
            let _ = fs::remove_file(dir.join("control-start-live"));
            if child.is_some() {
                status(&dir, "running")?;
            } else if blocked(&dir) {
                status(&dir, "blocked_by_safety_stop")?;
            } else {
                child = Some(launch(&dir, "live")?);
                mode = Some("live");
                status(&dir, "starting_live")?;
            }
        }
        if child.is_some() {
            status(&dir, "running")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_limit_wins_and_demo_default_is_bounded() {
        let path =
            std::env::temp_dir().join(format!("deriv-supervisor-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        assert_eq!(
            limit(&path, "configured-stake-minor", Some("100")).unwrap(),
            "100"
        );
        fs::write(path.join("configured-stake-minor"), "250\n").unwrap();
        assert_eq!(
            limit(&path, "configured-stake-minor", Some("100")).unwrap(),
            "250"
        );
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn safety_stop_is_fail_closed() {
        let path =
            std::env::temp_dir().join(format!("deriv-supervisor-stop-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        assert!(!blocked(&path));
        fs::write(path.join("emergency.stop"), "stop").unwrap();
        assert!(blocked(&path));
        fs::remove_dir_all(path).unwrap();
    }
}
