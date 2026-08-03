// API-derived records managed by this application are ephemeral. This module
// deliberately does not touch HISTORY_PATH, which may point at separately
// licensed, user-owned data.
const API_RECORD_RETENTION_SECS: u64 = 24 * 60 * 60;
const DEFAULT_API_RECORDS_DIR: &str = "data/api-records";

fn api_records_dir() -> PathBuf {
    std::env::var("DERIV_API_RECORDS_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_API_RECORDS_DIR))
}

fn api_record_is_expired(
    modified: std::time::SystemTime,
    now: std::time::SystemTime,
) -> bool {
    now.duration_since(modified)
        .map(|age| age >= Duration::from_secs(API_RECORD_RETENTION_SECS))
        .unwrap_or(false)
}

/// Delete expired regular files from the application-managed API record cache.
///
/// The scan is intentionally non-recursive and ignores directories and
/// symlinks. A missing cache directory is treated as an empty cache.
fn enforce_api_record_retention() -> Result<usize> {
    let directory = api_records_dir();
    if !directory.exists() {
        return Ok(0);
    }
    if !directory.is_dir() {
        anyhow::bail!(
            "DERIV_API_RECORDS_DIR must be a directory: {}",
            directory.display()
        );
    }

    let now = std::time::SystemTime::now();
    let mut removed = 0usize;
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("Failed to scan API record cache: {}", directory.display()))?
    {
        let entry = entry.with_context(|| {
            format!("Failed to read an entry in API record cache: {}", directory.display())
        })?;
        let metadata = fs::symlink_metadata(entry.path()).with_context(|| {
            format!("Failed to inspect API record: {}", entry.path().display())
        })?;
        if metadata.file_type().is_file()
            && api_record_is_expired(metadata.modified()?, now)
        {
            fs::remove_file(entry.path()).with_context(|| {
                format!("Failed to expire API record: {}", entry.path().display())
            })?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn records_expire_at_twenty_four_hours() {
        let now = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(100_000);
        assert!(!api_record_is_expired(
            now - Duration::from_secs(API_RECORD_RETENTION_SECS - 1),
            now
        ));
        assert!(api_record_is_expired(
            now - Duration::from_secs(API_RECORD_RETENTION_SECS),
            now
        ));
    }
}
