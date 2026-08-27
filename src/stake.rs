use anyhow::{Context, Result};

pub const MIN_STAKE_MINOR: i64 = 100;
pub const HARD_MAX_STAKE_MINOR: i64 = 100_000;

pub fn parse_stake_minor(value: &str) -> Result<i64> {
    let amount = value
        .trim()
        .parse::<f64>()
        .context("stake must be a decimal USD amount")?;
    anyhow::ensure!(
        amount.is_finite() && (1.0..=1000.0).contains(&amount),
        "stake must be between 1.00 and 1000.00 USD"
    );
    let minor = (amount * 100.0).round();
    anyhow::ensure!(
        (amount * 100.0 - minor).abs() < 1e-9,
        "stake accepts at most two decimal places"
    );
    Ok(minor as i64)
}

pub fn live_loss_confirmation(stake_minor: i64) -> String {
    format!(
        "I_ACCEPT_I_CAN_LOSE_ALL_{:.2}_USD",
        stake_minor as f64 / 100.0
    )
}

pub fn live_warning(stake_minor: i64) -> String {
    format!("WARNING: LIVE TRADING USES REAL MONEY. THE FULL SELECTED STAKE OF ${:.2} MAY BE LOST. No profit or recovery is guaranteed.", stake_minor as f64 / 100.0)
}

pub fn validate_live_confirmation(stake_minor: i64, supplied: Option<&str>) -> Result<()> {
    let expected = live_loss_confirmation(stake_minor);
    anyhow::ensure!(supplied.map(str::trim) == Some(expected.as_str()), "live trading is blocked: after reviewing the warning, set DERIV_LIVE_STAKE_CONFIRMATION={expected}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stake_is_exact_and_bounded() {
        assert_eq!(parse_stake_minor("1").unwrap(), 100);
        assert_eq!(parse_stake_minor("1000.00").unwrap(), 100_000);
        assert!(parse_stake_minor("0.99").is_err());
        assert!(parse_stake_minor("1000.01").is_err());
        assert!(parse_stake_minor("1.001").is_err());
    }
    #[test]
    fn confirmation_is_bound_to_the_selected_stake() {
        let token = live_loss_confirmation(100_000);
        assert_eq!(token, "I_ACCEPT_I_CAN_LOSE_ALL_1000.00_USD");
        assert!(validate_live_confirmation(100_000, Some(&token)).is_ok());
        assert!(validate_live_confirmation(99_999, Some(&token)).is_err());
    }
}
