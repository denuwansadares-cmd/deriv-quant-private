/// Load environment variables from the local `.env` file when available.
fn load_env() {
    if let Err(error) = dotenvy::dotenv() {
        log::debug!("No .env file loaded: {error}");
    }
}

/// Parse supported command-line arguments.
///
/// Supported forms:
///   --mode demo
///   --mode live
///   --mode simulation
///   --mode=demo
///   --symbols R_100,R_50
///   --symbols=R_100,R_50
///   --history data/history.json
///   --history=data/history.json
fn parse_args() -> (String, Vec<String>, String) {
    let mut mode = std::env::var("DERIV_MODE").unwrap_or_else(|_| "demo".to_string());

    let mut symbols = std::env::var("DERIV_SYMBOLS")
        .unwrap_or_else(|_| AVAILABLE_INDICES.join(","))
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>();

    let mut history_path = std::env::var("HISTORY_PATH")
        .unwrap_or_default();

    let args = std::env::args().skip(1).collect::<Vec<String>>();
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if let Some(value) = arg.strip_prefix("--mode=") {
            mode = value.trim().to_string();
        } else if arg == "--mode" {
            if let Some(value) = args.get(index + 1) {
                mode = value.trim().to_string();
                index += 1;
            }
        } else if let Some(value) = arg
            .strip_prefix("--symbols=")
            .or_else(|| arg.strip_prefix("--symbol="))
        {
            symbols = parse_symbol_list(value);
        } else if arg == "--symbols" || arg == "--symbol" {
            if let Some(value) = args.get(index + 1) {
                symbols = parse_symbol_list(value);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--history=") {
            history_path = value.trim().to_string();
        } else if arg == "--history" {
            if let Some(value) = args.get(index + 1) {
                history_path = value.trim().to_string();
                index += 1;
            }
        }

        index += 1;
    }

    if symbols.is_empty() {
        symbols.extend(AVAILABLE_INDICES.iter().map(|symbol| (*symbol).to_string()));
    }

    (mode, symbols, history_path)
}

fn parse_symbol_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Read the Deriv application ID without exposing it in source code.
fn get_app_id() -> Option<String> {
    std::env::var("DERIV_APP_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Read a buyer-owned Deriv OAuth or PAT bearer access token without exposing it in source code.
fn get_access_token() -> Option<String> {
    if let Some(token) = std::env::var("DERIV_ACCESS_TOKEN")
        .ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
        return Some(token);
    }
    let fallback = std::env::var("DERIV_PAT").or_else(|_| std::env::var("DERIV_PAT_TOKEN"))
        .ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    if fallback.is_some() {
        log::warn!("Using legacy DERIV_PAT fallback; migrate locally to DERIV_ACCESS_TOKEN");
    }
    fallback
}

fn parse_demo_duration(mode: &str, raw: Option<&str>) -> Result<Option<Duration>> {
    if mode != "demo" { return Ok(None); }
    let raw = raw.context("DEMO_DURATION_SECS must be set explicitly in demo mode")?;
    let seconds = raw.trim().parse::<u64>()
        .context("DEMO_DURATION_SECS must be a positive integer")?;
    anyhow::ensure!(seconds > 0, "DEMO_DURATION_SECS must be greater than zero");
    Ok(Some(Duration::from_secs(seconds)))
}

fn configured_demo_duration(mode: &str) -> Result<Option<Duration>> {
    let raw = std::env::var("DEMO_DURATION_SECS").ok();
    parse_demo_duration(mode, raw.as_deref())
}

async fn wait_for_demo_deadline(duration: Option<Duration>) {
    match duration {
        Some(duration) => sleep(duration).await,
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod runtime_duration_tests {
    use super::*;

    #[test]
    fn demo_duration_is_explicit_and_positive() {
        assert!(parse_demo_duration("demo", None).is_err());
        assert!(parse_demo_duration("demo", Some("0")).is_err());
        assert!(parse_demo_duration("demo", Some("invalid")).is_err());
        assert_eq!(parse_demo_duration("demo", Some("259200")).unwrap(), Some(Duration::from_secs(259_200)));
        assert_eq!(parse_demo_duration("simulation", None).unwrap(), None);
    }

    #[tokio::test(start_paused = true)]
    async fn demo_deadline_fires_at_the_exact_configured_duration() {
        let task = tokio::spawn(wait_for_demo_deadline(Some(Duration::from_secs(259_200))));
        tokio::time::advance(Duration::from_secs(259_199)).await;
        assert!(!task.is_finished());
        tokio::time::advance(Duration::from_secs(1)).await;
        task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn absent_deadline_never_fires() {
        let task = tokio::spawn(wait_for_demo_deadline(None));
        tokio::time::advance(Duration::from_secs(1_000_000)).await;
        assert!(!task.is_finished());
        task.abort();
    }
}

#[derive(Debug, Clone, Copy)]
struct RuntimeRiskConfig {
    max_stake_minor: i64,
    session_loss_limit_bps: i64,
    max_open_exposure_bps: i64,
    max_open_contracts: usize,
}
impl RuntimeRiskConfig {
    fn compiled_defaults() -> Self {
        Self {
            max_stake_minor: HARD_MAX_STAKE_MINOR,
            session_loss_limit_bps: SESSION_LOSS_LIMIT_BPS,
            max_open_exposure_bps: 200,
            max_open_contracts: 1,
        }
    }
    fn parse(mode: &str, value: impl Fn(&str) -> Option<String>) -> Result<Self> {
        if mode == "simulation" { return Ok(Self::compiled_defaults()); }
        let parse_i64 = |name: &str| -> Result<i64> {
            value(name).with_context(|| format!("{name} must be set explicitly"))?
                .trim().parse().with_context(|| format!("{name} must be an integer"))
        };
        let parse_usize = |name: &str| -> Result<usize> {
            value(name).with_context(|| format!("{name} must be set explicitly"))?
                .trim().parse().with_context(|| format!("{name} must be an integer"))
        };
        let config = Self {
            max_stake_minor: parse_i64("DERIV_MAX_STAKE_MINOR")?,
            session_loss_limit_bps: parse_i64("DERIV_SESSION_LOSS_LIMIT_BPS")?,
            max_open_exposure_bps: parse_i64("DERIV_MAX_OPEN_EXPOSURE_BPS")?,
            max_open_contracts: parse_usize("DERIV_MAX_OPEN_CONTRACTS")?,
        };
        anyhow::ensure!((MIN_STAKE_MINOR..=HARD_MAX_STAKE_MINOR).contains(&config.max_stake_minor), "DERIV_MAX_STAKE_MINOR is outside the reviewed absolute range");
        anyhow::ensure!((1..=SESSION_LOSS_LIMIT_BPS).contains(&config.session_loss_limit_bps), "DERIV_SESSION_LOSS_LIMIT_BPS is outside the reviewed absolute range");
        anyhow::ensure!((1..=200).contains(&config.max_open_exposure_bps), "DERIV_MAX_OPEN_EXPOSURE_BPS is outside the reviewed absolute range");
        anyhow::ensure!(config.max_open_contracts == 1, "DERIV_MAX_OPEN_CONTRACTS must equal 1");
        Ok(config)
    }
    fn load(mode: &str) -> Result<Self> {
        Self::parse(mode, |name| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod runtime_risk_config_tests {
    use super::*;
    #[test]
    fn explicit_one_contract_minimum_stake_config_passes() {
        let config = RuntimeRiskConfig::parse("demo", |name| Some(match name {
            "DERIV_MAX_STAKE_MINOR" => "100",
            "DERIV_SESSION_LOSS_LIMIT_BPS" => "100",
            "DERIV_MAX_OPEN_EXPOSURE_BPS" => "100",
            "DERIV_MAX_OPEN_CONTRACTS" => "1",
            _ => return None,
        }.to_string())).unwrap();
        assert_eq!(config.max_stake_minor, MIN_STAKE_MINOR);
        assert_eq!(config.max_open_contracts, 1);
    }
    #[test]
    fn missing_or_multi_contract_config_fails_closed() {
        assert!(RuntimeRiskConfig::parse("demo", |_| None).is_err());
        assert!(RuntimeRiskConfig::parse("demo", |name| Some(if name == "DERIV_MAX_OPEN_CONTRACTS" { "2" } else { "100" }.to_string())).is_err());
    }
}
