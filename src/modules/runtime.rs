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
///   --stake 3.00
///   --stake=3.00
fn parse_args_from(args: &[String]) -> Result<(String, Vec<String>, String, Option<i64>)> {
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
    let mut stake_minor = None;
    let mut index = 0usize;

    while index < args.len() {
        let arg = &args[index];

        if let Some(value) = arg.strip_prefix("--mode=") {
            mode = value.trim().to_string();
        } else if arg == "--mode" {
            let value = args.get(index + 1).context("--mode requires a value")?;
            mode = value.trim().to_string();
            index += 1;
        } else if let Some(value) = arg
            .strip_prefix("--symbols=")
            .or_else(|| arg.strip_prefix("--symbol="))
        {
            symbols = parse_symbol_list(value);
        } else if arg == "--symbols" || arg == "--symbol" {
            let value = args.get(index + 1).context("--symbols requires a value")?;
            symbols = parse_symbol_list(value);
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--history=") {
            history_path = value.trim().to_string();
        } else if arg == "--history" {
            let value = args.get(index + 1).context("--history requires a value")?;
            history_path = value.trim().to_string();
            index += 1;
        } else if let Some(value) = arg.strip_prefix("--stake=") {
            stake_minor = Some(crate::stake::parse_stake_minor(value)?);
        } else if arg == "--stake" {
            let value = args.get(index + 1).context("--stake requires a USD amount")?;
            stake_minor = Some(crate::stake::parse_stake_minor(value)?);
            index += 1;
        } else {
            anyhow::bail!("Unknown argument: {arg}");
        }

        index += 1;
    }

    if symbols.is_empty() {
        symbols.extend(AVAILABLE_INDICES.iter().map(|symbol| (*symbol).to_string()));
    }

    Ok((mode, symbols, history_path, stake_minor))
}

fn parse_args() -> Result<(String, Vec<String>, String, Option<i64>)> {
    parse_args_from(&std::env::args().skip(1).collect::<Vec<_>>())
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
    let Some(raw) = raw else { return Ok(None); };
    let seconds = raw.trim().parse::<u64>()
        .context("DEMO_DURATION_SECS must be a non-negative integer")?;
    Ok((seconds > 0).then(|| Duration::from_secs(seconds)))
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
    fn demo_duration_is_optional_for_continuous_operation() {
        assert_eq!(parse_demo_duration("demo", None).unwrap(), None);
        assert_eq!(parse_demo_duration("demo", Some("0")).unwrap(), None);
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

#[cfg(test)]
mod runtime_cli_tests {

    #[test]
    fn stake_parser_is_exact_and_bounded() {
        assert_eq!(crate::stake::parse_stake_minor("1").unwrap(), 100);
        assert_eq!(crate::stake::parse_stake_minor("3.5").unwrap(), 350);
        assert_eq!(crate::stake::parse_stake_minor("1000.00").unwrap(), 100_000);
        assert!(crate::stake::parse_stake_minor("0.99").is_err());
        assert!(crate::stake::parse_stake_minor("1.001").is_err());
        assert!(crate::stake::parse_stake_minor("1000.01").is_err());
    }
}

#[derive(Debug, Clone, Copy)]
struct RuntimeRiskConfig {
    max_stake_minor: i64,
    session_loss_limit_bps: i64,
    max_open_exposure_bps: i64,
    max_open_contracts: usize,
    max_consecutive_losses: u32,
    loss_cooldown_base_secs: u64,
}
impl RuntimeRiskConfig {
    fn compiled_defaults() -> Self {
        Self {
            max_stake_minor: HARD_MAX_STAKE_MINOR,
            session_loss_limit_bps: SESSION_LOSS_LIMIT_BPS,
            max_open_exposure_bps: BALANCE_RISK_BPS,
            max_open_contracts: 1,
            max_consecutive_losses: 3,
            loss_cooldown_base_secs: 60,
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
        let parse_u32 = |name: &str| -> Result<u32> {
            value(name).with_context(|| format!("{name} must be set explicitly"))?
                .trim().parse().with_context(|| format!("{name} must be an integer"))
        };
        let parse_u64 = |name: &str| -> Result<u64> {
            value(name).with_context(|| format!("{name} must be set explicitly"))?
                .trim().parse().with_context(|| format!("{name} must be an integer"))
        };
        let config = Self {
            max_stake_minor: parse_i64("DERIV_MAX_STAKE_MINOR")?,
            session_loss_limit_bps: parse_i64("DERIV_SESSION_LOSS_LIMIT_BPS")?,
            max_open_exposure_bps: parse_i64("DERIV_MAX_OPEN_EXPOSURE_BPS")?,
            max_open_contracts: parse_usize("DERIV_MAX_OPEN_CONTRACTS")?,
            max_consecutive_losses: parse_u32("DERIV_MAX_CONSECUTIVE_LOSSES")?,
            loss_cooldown_base_secs: parse_u64("DERIV_LOSS_COOLDOWN_BASE_SECS")?,
        };
        anyhow::ensure!((MIN_STAKE_MINOR..=HARD_MAX_STAKE_MINOR).contains(&config.max_stake_minor), "DERIV_MAX_STAKE_MINOR is outside the reviewed absolute range");
        anyhow::ensure!((1..=SESSION_LOSS_LIMIT_BPS).contains(&config.session_loss_limit_bps), "DERIV_SESSION_LOSS_LIMIT_BPS is outside the reviewed absolute range");
        anyhow::ensure!((1..=BALANCE_RISK_BPS).contains(&config.max_open_exposure_bps), "DERIV_MAX_OPEN_EXPOSURE_BPS is outside the reviewed absolute range");
        anyhow::ensure!(config.max_open_contracts == 1, "DERIV_MAX_OPEN_CONTRACTS must equal 1");
        anyhow::ensure!((1..=10).contains(&config.max_consecutive_losses), "DERIV_MAX_CONSECUTIVE_LOSSES must be between 1 and 10");
        anyhow::ensure!((10..=3_600).contains(&config.loss_cooldown_base_secs), "DERIV_LOSS_COOLDOWN_BASE_SECS must be between 10 and 3600");
        Ok(config)
    }
    fn load(mode: &str) -> Result<Self> {
        Self::parse(mode, |name| std::env::var(name).ok())
    }
    fn with_cli_stake(mut self, stake_minor: Option<i64>) -> Self {
        if let Some(stake_minor) = stake_minor { self.max_stake_minor = stake_minor; }
        self
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
            "DERIV_MAX_CONSECUTIVE_LOSSES" => "3",
            "DERIV_LOSS_COOLDOWN_BASE_SECS" => "60",
            _ => return None,
        }.to_string())).unwrap();
        assert_eq!(config.max_stake_minor, MIN_STAKE_MINOR);
        assert_eq!(config.max_open_contracts, 1);
        assert_eq!(config.max_consecutive_losses, 3);
    }
    #[test]
    fn missing_or_multi_contract_config_fails_closed() {
        assert!(RuntimeRiskConfig::parse("demo", |_| None).is_err());
        assert!(RuntimeRiskConfig::parse("demo", |name| Some(if name == "DERIV_MAX_OPEN_CONTRACTS" { "2" } else { "100" }.to_string())).is_err());
    }
}
