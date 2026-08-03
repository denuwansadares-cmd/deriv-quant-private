use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static ATTEMPTS: OnceLock<Mutex<Vec<Instant>>> = OnceLock::new();
fn risk_dir() -> PathBuf {
    std::env::var("DERIV_RISK_STATE_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".risk"))
}
fn bind_address() -> String {
    std::env::var("DERIV_DASHBOARD_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into())
}
fn atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".dashboard.tmp-{}", std::process::id()));
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
fn response(
    s: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    write!(s,"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; frame-ancestors 'none'\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len())
}
fn json(s: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    response(s, status, "application/json", body)
}
fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().skip(1).find_map(|line| {
        let (k, v) = line.split_once(':')?;
        (k.eq_ignore_ascii_case(name)).then(|| v.trim())
    })
}
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |diff, (x, y)| diff | (x ^ y))
        == 0
}
fn authorized(request: &str) -> bool {
    let Some(expected) = std::env::var("DERIV_DASHBOARD_TOKEN")
        .ok()
        .filter(|v| v.len() >= 32)
    else {
        return false;
    };
    let expiry = std::env::var("DERIV_DASHBOARD_TOKEN_EXPIRES_UNIX")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if expiry <= now {
        return false;
    }
    let bearer = header(request, "Authorization")
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let csrf = header(request, "X-CSRF-Token").unwrap_or("");
    let role = header(request, "X-Deriv-Role").unwrap_or("");
    constant_time_eq(bearer, &expected) && constant_time_eq(csrf, &expected) && role == "operator"
}
fn rate_allowed() -> bool {
    let now = Instant::now();
    let mut attempts = ATTEMPTS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap();
    attempts.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
    if attempts.len() >= 5 {
        return false;
    }
    attempts.push(now);
    true
}
fn audit(peer: &str, action: &str, accepted: bool) {
    let path = risk_dir().join("dashboard-audit.jsonl");
    let record = format!(
        r#"{{"at_unix":{},"peer":"{}","action":"{}","accepted":{}}}"#,
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        peer.replace('"', ""),
        action,
        accepted
    );
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{record}");
        let _ = f.sync_data();
    }
}
fn handle(mut s: TcpStream) -> std::io::Result<()> {
    let peer = s
        .peer_addr()
        .map(|v| v.ip().to_string())
        .unwrap_or_else(|_| "unknown".into());
    let mut b = [0_u8; 8192];
    let n = s.read(&mut b)?;
    let request = String::from_utf8_lossy(&b[..n]);
    match request.lines().next().unwrap_or("") {
        "GET / HTTP/1.1" | "GET /index.html HTTP/1.1" => response(
            &mut s,
            "200 OK",
            "text/html; charset=utf-8",
            include_str!("../../web/index.html"),
        ),
        "GET /api/v1/status HTTP/1.1" => {
            let stopped = risk_dir().join("emergency.stop").exists()
                || risk_dir().join("stop-state.json").exists();
            json(
                &mut s,
                "200 OK",
                &format!(
                    r#"{{"mode":"demo","live_trading":false,"stopped":{stopped},"bind":"{}","claims":"verified-tests-only"}}"#,
                    bind_address()
                ),
            )
        }
        "GET /api/v1/qualifications HTTP/1.1" => {
            let body = fs::read_to_string("config/strategy-qualifications.json")
                .unwrap_or_else(|_| r#"{"models":[]}"#.into());
            json(&mut s, "200 OK", &body)
        }
        "POST /api/v1/emergency-stop HTTP/1.1" => {
            if !rate_allowed() {
                audit(&peer, "emergency_stop", false);
                return json(
                    &mut s,
                    "429 Too Many Requests",
                    r#"{"error":"rate_limited"}"#,
                );
            }
            if !authorized(&request) {
                audit(&peer, "emergency_stop", false);
                return json(
                    &mut s,
                    "401 Unauthorized",
                    r#"{"error":"operator_auth_csrf_or_expiry"}"#,
                );
            }
            let reason = format!("dashboard emergency stop by {peer}");
            atomic(&risk_dir().join("emergency.stop"), reason.as_bytes())?;
            atomic(
                &risk_dir().join("stop-state.json"),
                format!(r#"{{"stopped":true,"reason":"{}"}}"#, reason).as_bytes(),
            )?;
            audit(&peer, "emergency_stop", true);
            json(&mut s, "202 Accepted", r#"{"stopped":true}"#)
        }
        _ => json(&mut s, "404 Not Found", r#"{"error":"not_found"}"#),
    }
}
fn main() -> std::io::Result<()> {
    let address = bind_address();
    println!("Safe demo dashboard: http://{address} (live trading unavailable)");
    for stream in TcpListener::bind(address)?.incoming() {
        if let Ok(stream) = stream {
            let _ = handle(stream);
        }
    }
    Ok(())
}
