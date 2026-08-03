use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER_BYTES: usize = 16 * 1024;
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
type Attempts = Arc<Mutex<HashMap<std::net::IpAddr, Vec<Instant>>>>;
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
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing parent"))?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".dashboard.tmp-{}-{nonce}", std::process::id()));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&temp, path)?;
        fs::File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn response(
    s: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    write!(s,"HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Security-Policy: {CSP}\r\nX-Frame-Options: DENY\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nPermissions-Policy: camera=(), microphone=(), geolocation=()\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len())
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
    let mut diff = a.len() ^ b.len();
    for index in 0..a.len().max(b.len()) {
        diff |= usize::from(
            *a.as_bytes().get(index).unwrap_or(&0) ^ *b.as_bytes().get(index).unwrap_or(&0),
        );
    }
    diff == 0
}
fn authorized(request: &str) -> bool {
    let Some(expected) = std::env::var("DERIV_DASHBOARD_TOKEN")
        .ok()
        .filter(|v| v.len() >= 32)
    else {
        return false;
    };
    let Some(expected_csrf) = std::env::var("DERIV_DASHBOARD_CSRF_TOKEN")
        .ok()
        .filter(|value| value.len() >= 32 && !constant_time_eq(value, &expected))
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
    constant_time_eq(bearer, &expected)
        && constant_time_eq(csrf, &expected_csrf)
        && role == "operator"
}
fn rate_allowed(attempts: &Attempts, peer: std::net::IpAddr) -> bool {
    let now = Instant::now();
    let mut all = attempts.lock().unwrap_or_else(|lock| lock.into_inner());
    let peer_attempts = all.entry(peer).or_default();
    peer_attempts.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
    if peer_attempts.len() >= 5 {
        return false;
    }
    peer_attempts.push(now);
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
fn handle(mut s: TcpStream, attempts: &Attempts) -> std::io::Result<()> {
    let peer = s
        .peer_addr()
        .map(|v| v.ip().to_string())
        .unwrap_or_else(|_| "unknown".into());
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    let mut request_bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let count = s.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        request_bytes.extend_from_slice(&buffer[..count]);
        if request_bytes.len() > MAX_HEADER_BYTES {
            return json(
                &mut s,
                "431 Request Header Fields Too Large",
                r#"{"error":"headers_too_large"}"#,
            );
        }
        if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8(request_bytes)
        .map_err(|_| std::io::Error::other("request is not UTF-8"))?;
    match request.lines().next().unwrap_or("") {
        "GET / HTTP/1.1" | "GET /index.html HTTP/1.1" => response(
            &mut s,
            "200 OK",
            "text/html; charset=utf-8",
            include_str!("../../web/index.html"),
        ),
        "GET /style.css HTTP/1.1" => response(
            &mut s,
            "200 OK",
            "text/css; charset=utf-8",
            include_str!("../../web/style.css"),
        ),
        "GET /app.js HTTP/1.1" => response(
            &mut s,
            "200 OK",
            "application/javascript; charset=utf-8",
            include_str!("../../web/app.js"),
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
            let peer_ip = s
                .peer_addr()
                .map(|value| value.ip())
                .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
            if !rate_allowed(attempts, peer_ip) {
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
    println!("Safe demo dashboard started (live trading unavailable)");
    let attempts: Attempts = Arc::new(Mutex::new(HashMap::new()));
    let active = Arc::new(Mutex::new(0_usize));
    for stream in TcpListener::bind(address)?.incoming().flatten() {
        let mut count = active.lock().unwrap_or_else(|lock| lock.into_inner());
        if *count >= MAX_CONNECTIONS {
            drop(count);
            let mut stream = stream;
            let _ = json(
                &mut stream,
                "503 Service Unavailable",
                r#"{"error":"busy"}"#,
            );
            continue;
        }
        *count += 1;
        drop(count);
        let attempts = Arc::clone(&attempts);
        let active = Arc::clone(&active);
        thread::spawn(move || {
            let _ = handle(stream, &attempts);
            let mut count = active.lock().unwrap_or_else(|lock| lock.into_inner());
            *count = count.saturating_sub(1);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn constant_time_comparison_rejects_length_and_content_changes() {
        assert!(constant_time_eq("same", "same"));
        assert!(!constant_time_eq("same", "sam"));
        assert!(!constant_time_eq("same", "sage"));
    }
    #[test]
    fn rate_limit_is_isolated_by_peer() {
        let attempts: Attempts = Arc::new(Mutex::new(HashMap::new()));
        let first = std::net::IpAddr::from([127, 0, 0, 1]);
        let second = std::net::IpAddr::from([127, 0, 0, 2]);
        for _ in 0..5 {
            assert!(rate_allowed(&attempts, first));
        }
        assert!(!rate_allowed(&attempts, first));
        assert!(rate_allowed(&attempts, second));
    }
    #[test]
    fn csp_forbids_inline_exceptions() {
        assert!(!CSP.contains("unsafe-inline"));
        assert!(CSP.contains("object-src 'none'"));
    }
}
