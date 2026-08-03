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
        .unwrap_or_else(|_| "~/deriv_tick_downloader/data".to_string());

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
    std::env::var("DERIV_ACCESS_TOKEN")
        .or_else(|_| std::env::var("DERIV_PAT"))
        .or_else(|_| std::env::var("DERIV_PAT_TOKEN"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
