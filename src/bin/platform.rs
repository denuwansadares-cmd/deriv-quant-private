#[allow(dead_code)]
#[path = "../stake.rs"]
mod stake;
use stake::{
    live_loss_confirmation, validate_live_confirmation, HARD_MAX_STAKE_MINOR, MIN_STAKE_MINOR,
};

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
static AUDIT_LOCK: Mutex<()> = Mutex::new(());
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
fn fresh_heartbeat() -> bool {
    fs::metadata(risk_dir().join("runtime-heartbeat"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age < Duration::from_secs(20))
}
fn validated_header_i64(request: &str, name: &str, min: i64, max: i64) -> Option<i64> {
    header(request, name)?
        .parse::<i64>()
        .ok()
        .filter(|v| (min..=max).contains(v))
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
fn retained_audit_body(existing: &str, record: &str, cutoff: u64) -> String {
    let mut retained: Vec<&str> = existing
        .lines()
        .filter(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|value| value.get("at_unix")?.as_u64())
                .is_some_and(|timestamp| timestamp >= cutoff)
        })
        .collect();
    retained.push(record);
    let mut body = retained.join("\n");
    body.push('\n');
    body
}
fn audit(peer: &str, action: &str, accepted: bool) -> std::io::Result<()> {
    let _guard = AUDIT_LOCK.lock().unwrap_or_else(|lock| lock.into_inner());
    let path = risk_dir().join("dashboard-audit.jsonl");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let retention = std::env::var("DERIV_DASHBOARD_AUDIT_RETENTION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(604_800)
        .clamp(86_400, 2_592_000);
    let record = serde_json::json!({
        "at_unix": now,
        "peer": peer,
        "action": action,
        "accepted": accepted
    })
    .to_string();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    atomic(
        &path,
        retained_audit_body(&existing, &record, now.saturating_sub(retention)).as_bytes(),
    )
}
fn journal_path() -> PathBuf {
    std::env::var("DERIV_AUDIT_JOURNAL_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/audit"))
        .join("decision-journal.jsonl")
}

fn exposure_pending() -> bool {
    fs::read_to_string(risk_dir().join("open-exposure.json"))
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .and_then(|value| {
            value
                .get("contracts")
                .and_then(serde_json::Value::as_object)
                .map(|items| !items.is_empty())
        })
        .unwrap_or(false)
}

fn dashboard_telemetry() -> serde_json::Value {
    let path = journal_path();
    let Ok(body) = fs::read_to_string(path) else {
        return serde_json::json!({"running": false, "available": false, "trades": 0, "wins": 0, "losses": 0, "session_pnl_minor": 0, "recent": [], "signals": []});
    };
    let heartbeat_fresh = fs::metadata(risk_dir().join("runtime-heartbeat"))
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age <= Duration::from_secs(15));
    let mut mode = "unknown".to_string();
    let mut state = "Idle".to_string();
    let mut started_at_ms = 0_u64;
    let mut last_event_at_ms = 0_u64;
    let mut trades = 0_u64;
    let mut wins = 0_u64;
    let mut losses = 0_u64;
    let mut session_pnl_minor = 0_i64;
    let mut drawdown_minor = 0_i64;
    let mut next_stake_minor = 0_i64;
    let mut open_contract: Option<String> = None;
    let mut recent = Vec::new();
    let mut signals = Vec::new();
    let mut rise_fall_activation_at_ms: Option<u64> = None;
    for line in body.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = record
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let at = record
            .get("recorded_at_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let details = record
            .get("details")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if kind == "run_started" {
            mode = details
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            state = "Starting".to_string();
            started_at_ms = at;
            trades = 0;
            wins = 0;
            losses = 0;
            session_pnl_minor = 0;
            drawdown_minor = 0;
            next_stake_minor = 0;
            open_contract = None;
            recent.clear();
            signals.clear();
            rise_fall_activation_at_ms = None;
        }
        if started_at_ms == 0 {
            continue;
        }
        last_event_at_ms = last_event_at_ms.max(at);
        if kind == "rise_fall_activation_scheduled" {
            rise_fall_activation_at_ms = details
                .get("delay_secs")
                .and_then(serde_json::Value::as_u64)
                .map(|delay| at.saturating_add(delay.saturating_mul(1_000)));
        }
        match kind {
            "state_transition" => {
                if details.get("accepted").and_then(serde_json::Value::as_bool) == Some(true) {
                    state = details
                        .get("to")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(&state)
                        .to_string();
                }
            }
            "contract_strategy_decision" | "strategy_decision" => {
                let stage = if details.get("decision").and_then(serde_json::Value::as_str)
                    == Some("reject")
                {
                    "rejected_neutral"
                } else {
                    "analysed"
                };
                signals.push(serde_json::json!({"at_ms": at, "kind": kind, "stage": stage, "details": details}));
                if signals.len() > 40 {
                    signals.remove(0);
                }
            }
            "order_request"
                if details
                    .get("request_kind")
                    .and_then(serde_json::Value::as_str)
                    == Some("buy") =>
            {
                signals.push(serde_json::json!({"at_ms": at, "kind": kind, "stage": "risk_approved", "details": details}));
                if signals.len() > 40 {
                    signals.remove(0);
                }
            }
            "order_result" => match details
                .get("result_kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
            {
                "contract_opened" => {
                    open_contract = details
                        .get("normalized_contract_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    signals.push(serde_json::json!({"at_ms": at, "kind": kind, "stage": "executed", "details": details}));
                    if signals.len() > 40 {
                        signals.remove(0);
                    }
                }
                "contract_sold" => open_contract = None,
                _ => {}
            },
            "pnl_calculation" => {
                let trade_pnl = details
                    .get("trade_pnl_minor")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                trades += 1;
                if trade_pnl > 0 {
                    wins += 1
                } else {
                    losses += 1
                }
                session_pnl_minor = details
                    .get("session_pnl_minor")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(session_pnl_minor);
                drawdown_minor = details
                    .get("drawdown_minor")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(drawdown_minor);
                next_stake_minor = details
                    .get("next_stake_minor")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(next_stake_minor);
                recent.push(serde_json::json!({"at_ms": at, "pnl_minor": trade_pnl, "session_pnl_minor": session_pnl_minor}));
                if recent.len() > 12 {
                    recent.remove(0);
                }
                signals.push(serde_json::json!({"at_ms": at, "kind": kind, "stage": "settled", "details": details}));
                if signals.len() > 40 {
                    signals.remove(0);
                }
            }
            "risk_stop" => state = "Stopped".to_string(),
            _ => {}
        }
    }
    let pending_exposure = exposure_pending();
    if pending_exposure {
        state = "Exposure pending / review locked".to_string();
    }
    serde_json::json!({
        "available": started_at_ms > 0,
        "exposure_pending": pending_exposure,
        "running": heartbeat_fresh,
        "mode": mode,
        "state": state,
        "started_at_ms": started_at_ms,
        "last_event_at_ms": last_event_at_ms,
        "rise_fall_activation_at_ms": rise_fall_activation_at_ms,
        "trades": trades,
        "wins": wins,
        "losses": losses,
        "win_rate_percent": if trades > 0 { Some(wins as f64 * 100.0 / trades as f64) } else { None },
        "session_pnl_minor": session_pnl_minor,
        "drawdown_minor": drawdown_minor,
        "next_stake_minor": next_stake_minor,
        "open_contract": open_contract,
        "recent": recent,
        "signals": signals
    })
}

fn html_escape(value: &str) -> String {
    value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
}
fn money(value: i64) -> String {
    format!(
        "USD {}{:.2}",
        if value < 0 { "-" } else { "" },
        (value.abs() as f64) / 100.0
    )
}
fn dashboard_html() -> String {
    let telemetry = dashboard_telemetry();
    let running = telemetry
        .get("running")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let stopped = risk_dir().join("emergency.stop").exists()
        || risk_dir().join("stop-state.json").exists()
        || exposure_pending();
    let control = fs::read_to_string(risk_dir().join("control-status.json"))
        .ok()
        .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
        .and_then(|v| {
            v.get("state")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "idle".into());
    let configured = risk_dir().join("configured-stake-minor").exists()
        && risk_dir().join("configured-loss-limit-bps").exists();
    let trades = telemetry
        .get("trades")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let wins = telemetry
        .get("wins")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let losses = telemetry
        .get("losses")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let pnl = telemetry
        .get("session_pnl_minor")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let next = telemetry
        .get("next_stake_minor")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let mode = telemetry
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let replacements = [
        (
            "{{PLATFORM}}",
            if running {
                "OPERATIONAL".into()
            } else {
                "AVAILABLE".into()
            },
        ),
        (
            "{{CONNECTION}}",
            if running {
                "connected".into()
            } else {
                "idle".into()
            },
        ),
        ("{{MODE}}", html_escape(mode)),
        (
            "{{RISK}}",
            if stopped {
                "STOPPED".into()
            } else {
                "PROTECTED".into()
            },
        ),
        ("{{CONTROL}}", html_escape(&control)),
        (
            "{{SESSION_TITLE}}",
            if running {
                "Trading session".into()
            } else {
                "Last verified session".into()
            },
        ),
        (
            "{{BOT_STATE}}",
            if running {
                "BOT RUNNING".into()
            } else {
                "WAITING".into()
            },
        ),
        ("{{PNL}}", money(pnl)),
        ("{{TRADES}}", trades.to_string()),
        ("{{WINS}}", wins.to_string()),
        ("{{LOSSES}}", losses.to_string()),
        (
            "{{WIN_RATE}}",
            if trades == 0 {
                "—".into()
            } else {
                format!("{:.1}%", wins as f64 * 100.0 / trades as f64)
            },
        ),
        (
            "{{NEXT_STAKE}}",
            if next == 0 { "—".into() } else { money(next) },
        ),
        (
            "{{CONFIGURED}}",
            if configured {
                "Risk limits saved".into()
            } else {
                "Risk limits not configured".into()
            },
        ),
    ];
    replacements.into_iter().fold(
        include_str!("../../web/index.html").to_owned(),
        |body, (key, value)| body.replace(key, &value),
    )
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
            &dashboard_html(),
        ),
        "GET /style.css HTTP/1.1" => response(
            &mut s,
            "200 OK",
            "text/css; charset=utf-8",
            include_str!("../../web/style.css"),
        ),
        "GET /api/v1/status HTTP/1.1" => {
            let pending_exposure = exposure_pending();
            let stopped = risk_dir().join("emergency.stop").exists()
                || risk_dir().join("stop-state.json").exists()
                || pending_exposure;
            json(
                &mut s,
                "200 OK",
                &format!(
                    r#"{{"mode":"operator-controlled","live_trading":true,"stopped":{stopped},"exposure_pending":{pending_exposure},"bind":"{}","claims":"verified-tests-only"}}"#,
                    bind_address()
                ),
            )
        }
        "GET /api/v1/control/config HTTP/1.1" => {
            let configured = risk_dir().join("configured-stake-minor").exists()
                && risk_dir().join("configured-loss-limit-bps").exists();
            let stake_minor = fs::read_to_string(risk_dir().join("configured-stake-minor"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok())
                .unwrap_or(100);
            let loss_limit_bps = fs::read_to_string(risk_dir().join("configured-loss-limit-bps"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok())
                .unwrap_or(100);
            let control_state = fs::read_to_string(risk_dir().join("control-status.json"))
                .ok()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
                .and_then(|value| {
                    value
                        .get("state")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| "idle".to_string());
            let control_state = if exposure_pending() {
                "exposure_pending_review_locked".to_string()
            } else {
                control_state
            };
            json(
                &mut s,
                "200 OK",
                &serde_json::json!({
                    "stake_minor": stake_minor.clamp(MIN_STAKE_MINOR, HARD_MAX_STAKE_MINOR),
                    "loss_limit_bps": loss_limit_bps.clamp(1, 2_000),
                    "max_open_contracts": 1,
                    "hard_max_stake_minor": HARD_MAX_STAKE_MINOR,
                    "running": fresh_heartbeat(),
                "configured": configured,
                    "control_state": control_state
                })
                .to_string(),
            )
        }
        "GET /api/v1/telemetry HTTP/1.1" => {
            json(&mut s, "200 OK", &dashboard_telemetry().to_string())
        }
        "GET /api/v1/qualifications HTTP/1.1" => {
            let body = fs::read_to_string("config/strategy-qualifications.json")
                .unwrap_or_else(|_| r#"{"models":[]}"#.into());
            json(&mut s, "200 OK", &body)
        }
        "POST /api/v1/control/start-demo HTTP/1.1" => {
            if !authorized(&request) {
                audit(&peer, "start_demo", false)?;
                return json(
                    &mut s,
                    "401 Unauthorized",
                    r#"{"error":"operator_auth_required"}"#,
                );
            }
            if risk_dir().join("emergency.stop").exists()
                || risk_dir().join("stop-state.json").exists()
            {
                audit(&peer, "start_demo", false)?;
                return json(
                    &mut s,
                    "409 Conflict",
                    r#"{"error":"safety_stop_requires_review"}"#,
                );
            }
            atomic(&risk_dir().join("control-start-demo"), b"start demo")?;
            audit(&peer, "start_demo", true)?;
            json(
                &mut s,
                "202 Accepted",
                r#"{"accepted":true,"action":"start_demo"}"#,
            )
        }
        "POST /api/v1/control/configure-risk HTTP/1.1" => {
            if !authorized(&request) {
                audit(&peer, "configure_risk", false)?;
                return json(
                    &mut s,
                    "401 Unauthorized",
                    r#"{"error":"operator_auth_required"}"#,
                );
            }
            if fresh_heartbeat() {
                audit(&peer, "configure_risk", false)?;
                return json(
                    &mut s,
                    "409 Conflict",
                    r#"{"error":"stop_trading_before_changing_risk"}"#,
                );
            }
            let Some(stake_minor) = validated_header_i64(
                &request,
                "X-Stake-Minor",
                MIN_STAKE_MINOR,
                HARD_MAX_STAKE_MINOR,
            ) else {
                return json(
                    &mut s,
                    "400 Bad Request",
                    r#"{"error":"stake_must_be_between_1_and_1000_usd"}"#,
                );
            };
            let Some(loss_limit_bps) = validated_header_i64(&request, "X-Loss-Limit-Bps", 1, 2_000)
            else {
                return json(
                    &mut s,
                    "400 Bad Request",
                    r#"{"error":"loss_limit_must_be_between_0_01_and_20_percent"}"#,
                );
            };
            atomic(
                &risk_dir().join("configured-stake-minor"),
                stake_minor.to_string().as_bytes(),
            )?;
            atomic(
                &risk_dir().join("configured-loss-limit-bps"),
                loss_limit_bps.to_string().as_bytes(),
            )?;
            audit(&peer, "configure_risk", true)?;
            json(&mut s, "200 OK", &serde_json::json!({"accepted":true,"stake_minor":stake_minor,"loss_limit_bps":loss_limit_bps}).to_string())
        }
        "POST /api/v1/control/start-live HTTP/1.1" => {
            if !authorized(&request) {
                audit(&peer, "start_live", false)?;
                return json(
                    &mut s,
                    "401 Unauthorized",
                    r#"{"error":"operator_auth_required"}"#,
                );
            }
            let acknowledged = header(&request, "X-Live-Risk-Acknowledged") == Some("true");
            let confirmed = header(&request, "X-Live-Confirmation") == Some("LIVE");
            let stake_minor = fs::read_to_string(risk_dir().join("configured-stake-minor"))
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok());
            let stake_confirmed = stake_minor.is_some_and(|stake| {
                validate_live_confirmation(stake, header(&request, "X-Live-Stake-Confirmation"))
                    .is_ok()
            });
            if !acknowledged || !confirmed || !stake_confirmed {
                audit(&peer, "start_live", false)?;
                return json(
                    &mut s,
                    "400 Bad Request",
                    r#"{"error":"live_risk_acknowledgement_LIVE_and_amount_bound_confirmation_required"}"#,
                );
            }
            if fresh_heartbeat()
                || risk_dir().join("emergency.stop").exists()
                || risk_dir().join("stop-state.json").exists()
            {
                audit(&peer, "start_live", false)?;
                return json(
                    &mut s,
                    "409 Conflict",
                    r#"{"error":"trading_active_or_safety_stop_requires_review"}"#,
                );
            }
            if !risk_dir().join("configured-stake-minor").exists()
                || !risk_dir().join("configured-loss-limit-bps").exists()
            {
                return json(
                    &mut s,
                    "409 Conflict",
                    r#"{"error":"configure_risk_before_live_trading"}"#,
                );
            }
            let stake_minor = stake_minor.expect("validated configured stake");
            atomic(
                &risk_dir().join("live-stake-confirmation"),
                live_loss_confirmation(stake_minor).as_bytes(),
            )?;
            atomic(
                &risk_dir().join("control-start-live"),
                b"start live with explicit risk acknowledgement",
            )?;
            audit(&peer, "start_live", true)?;
            json(
                &mut s,
                "202 Accepted",
                r#"{"accepted":true,"action":"start_live"}"#,
            )
        }
        "POST /api/v1/control/stop-trading HTTP/1.1"
        | "POST /api/v1/control/stop-demo HTTP/1.1" => {
            if !authorized(&request) {
                audit(&peer, "stop_demo", false)?;
                return json(
                    &mut s,
                    "401 Unauthorized",
                    r#"{"error":"operator_auth_required"}"#,
                );
            }
            atomic(&risk_dir().join("control-stop-demo"), b"graceful stop")?;
            audit(&peer, "stop_demo", true)?;
            json(
                &mut s,
                "202 Accepted",
                r#"{"accepted":true,"action":"stop_demo"}"#,
            )
        }
        "POST /api/v1/emergency-stop HTTP/1.1" => {
            let peer_ip = s
                .peer_addr()
                .map(|value| value.ip())
                .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
            if !rate_allowed(attempts, peer_ip) {
                audit(&peer, "emergency_stop", false)?;
                return json(
                    &mut s,
                    "429 Too Many Requests",
                    r#"{"error":"rate_limited"}"#,
                );
            }
            if !authorized(&request) {
                audit(&peer, "emergency_stop", false)?;
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
            audit(&peer, "emergency_stop", true)?;
            json(&mut s, "202 Accepted", r#"{"stopped":true}"#)
        }
        _ => json(&mut s, "404 Not Found", r#"{"error":"not_found"}"#),
    }
}
fn main() -> std::io::Result<()> {
    let address = bind_address();
    println!("Trading control dashboard started (live mode requires explicit risk confirmation)");
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
    fn request_once(request: &str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempts: Attempts = Arc::new(Mutex::new(HashMap::new()));
        let server_attempts = Arc::clone(&attempts);
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle(stream, &server_attempts).unwrap();
        });
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        response
    }

    #[test]
    fn endpoint_level_read_only_and_authorization_policy_is_enforced() {
        for path in [
            "/",
            "/index.html",
            "/style.css",
            "/api/v1/status",
            "/api/v1/control/config",
            "/api/v1/telemetry",
            "/api/v1/qualifications",
        ] {
            let response = request_once(&format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n"));
            assert!(
                response.starts_with("HTTP/1.1 200 OK"),
                "GET {path}: {response}"
            );
        }
        for path in [
            "/api/v1/control/start-demo",
            "/api/v1/control/configure-risk",
            "/api/v1/control/start-live",
            "/api/v1/control/stop-trading",
            "/api/v1/control/stop-demo",
            "/api/v1/emergency-stop",
        ] {
            let response = request_once(&format!(
                "POST {path} HTTP/1.1\r\nHost: localhost\r\nX-Deriv-Role: viewer\r\n\r\n"
            ));
            assert!(
                response.starts_with("HTTP/1.1 401 Unauthorized"),
                "POST {path}: {response}"
            );
        }
        let response =
            request_once("PUT /api/v1/control/start-demo HTTP/1.1\r\nHost: localhost\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 404 Not Found"));
    }

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
    fn audit_retention_drops_expired_and_malformed_records() {
        let existing = concat!(
            "{\"at_unix\":99,\"accepted\":false}\n",
            "malformed\n",
            "{\"at_unix\":100,\"accepted\":true}\n",
            "{\"at_unix\":101,\"accepted\":false}\n"
        );
        let current = "{\"at_unix\":102,\"accepted\":true}";
        let body = retained_audit_body(existing, current, 100);
        assert!(!body.contains("malformed"));
        assert!(!body.contains("\"at_unix\":99"));
        assert!(body.contains("\"at_unix\":100"));
        assert!(body.ends_with("{\"at_unix\":102,\"accepted\":true}\n"));
    }
    #[test]
    fn csp_forbids_inline_exceptions() {
        assert!(!CSP.contains("unsafe-inline"));
        assert!(CSP.contains("object-src 'none'"));
    }
}
