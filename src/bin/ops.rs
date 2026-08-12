use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn run(program: &str, args: &[&str]) -> Result {
    println!("+ {program} {}", args.join(" "));
    let status = Command::new(program).args(args).status()?;
    ensure(status, program)
}
fn run_env(program: &str, args: &[&str], vars: &[(&str, &str)]) -> Result {
    println!("+ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(vars.iter().copied())
        .status()?;
    ensure(status, program)
}
fn ensure(status: ExitStatus, name: &str) -> Result {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{name} failed with {status}").into())
    }
}
fn exists(program: &str) -> bool {
    Command::new(program).arg("--version").output().is_ok()
}
fn root() -> Result<PathBuf> {
    let mut path = env::current_exe()?;
    while path.pop() {
        if path.join("Cargo.toml").is_file() {
            return Ok(path);
        }
    }
    env::current_dir().map_err(Into::into)
}
fn require(path: &Path) -> Result {
    if path.metadata().is_ok_and(|m| m.len() > 0) {
        Ok(())
    } else {
        Err(format!("missing or empty: {}", path.display()).into())
    }
}
fn copy_file(from: &Path, to: &Path) -> Result {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to)?;
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}
fn temporary(label: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "deriv-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}

fn reproducible_build(root: &Path) -> Result {
    let first = temporary("repro-a");
    let second = temporary("repro-b");
    fs::create_dir_all(&first)?;
    fs::create_dir_all(&second)?;
    let first_s = first.to_string_lossy();
    let second_s = second.to_string_lossy();
    env::set_current_dir(root)?;
    run_env(
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--no-default-features",
            "--bin",
            "deriv_platform",
        ],
        &[
            ("CARGO_INCREMENTAL", "0"),
            ("SOURCE_DATE_EPOCH", "0"),
            ("CARGO_TARGET_DIR", &first_s),
        ],
    )?;
    run_env(
        "cargo",
        &[
            "build",
            "--locked",
            "--release",
            "--no-default-features",
            "--bin",
            "deriv_platform",
        ],
        &[
            ("CARGO_INCREMENTAL", "0"),
            ("SOURCE_DATE_EPOCH", "0"),
            ("CARGO_TARGET_DIR", &second_s),
        ],
    )?;
    let a = first.join("release/deriv_platform");
    let b = second.join("release/deriv_platform");
    if fs::read(&a)? != fs::read(&b)? {
        return Err("release binaries are not reproducible".into());
    }
    run("sha256sum", &[a.to_str().ok_or("non-UTF-8 path")?])?;
    fs::remove_dir_all(first)?;
    fs::remove_dir_all(second)?;
    Ok(())
}
fn fixture(root: &Path) -> Result<PathBuf> {
    let directory = temporary("fixture");
    fs::create_dir_all(&directory)?;
    env::set_current_dir(root)?;
    run(
        "cargo",
        &[
            "run",
            "--quiet",
            "--locked",
            "--no-default-features",
            "--bin",
            "generate_synthetic_fixture",
            "--",
            directory.to_str().ok_or("non-UTF-8 path")?,
        ],
    )?;
    if fs::read(root.join("fixtures/synthetic/R_100.csv"))?
        != fs::read(directory.join("R_100.csv"))?
    {
        return Err("generated fixture differs".into());
    }
    run("sha256sum", &["-c", "fixtures/synthetic/R_100.csv.sha256"])?;
    Ok(directory)
}
fn release_readiness(root: &Path) -> Result {
    env::set_current_dir(root)?;
    println!("[1/5] Required buyer assets");
    for item in [
        "README.md",
        "BUYER_FULL_SETUP.md",
        "BUYER_FULL_SETUP_SI.md",
        "PRODUCT_API.md",
        "SECURITY_EVIDENCE.md",
        "CLAIMS.md",
        "THIRD_PARTY_LICENSES.md",
        "deriv_bot.cdx.json",
        "Dockerfile",
        "docker-compose.yml",
        "Caddyfile",
        "web/index.html",
        "web/style.css",
    ] {
        require(&root.join(item))?;
    }
    println!("[2/5] Secret-bearing paths are not tracked");
    for item in [".env", ".env.local", "data/api-records"] {
        let status = Command::new("git")
            .args(["ls-files", "--error-unmatch", item])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if status.success() {
            return Err(format!("tracked runtime/secret path: {item}").into());
        }
    }
    println!("[3/5] Rust regression and live-build gate");
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run("cargo", &["check", "--locked", "--all-targets"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--locked",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run("cargo", &["test", "--locked", "--all-targets"])?;
    run(
        "cargo",
        &[
            "check",
            "--locked",
            "--features",
            "live-trading",
            "--bin",
            "deriv_bot",
        ],
    )?;
    println!("[4/5] Deterministic fixture");
    let directory = fixture(root)?;
    fs::remove_dir_all(directory)?;
    println!("[5/5] Integration");
    if exists("docker") {
        run("docker", &["compose", "config", "--quiet"])?;
    } else {
        println!("SKIP: docker unavailable");
    }
    println!("PASS: commercial release preflight completed");
    Ok(())
}
fn buyer_evaluate(root: &Path) -> Result {
    let source = root.join("source");
    env::set_current_dir(&source)?;
    if source.join(".env").exists() || source.join("data/api-records").exists() {
        return Err("secret/runtime paths present in buyer source".into());
    }
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run("cargo", &["test", "--locked", "--no-default-features"])?;
    let directory = fixture(&source)?;
    let audit = directory.join("audit");
    let audit_s = audit.to_string_lossy();
    run_env(
        "cargo",
        &[
            "run",
            "--locked",
            "--no-default-features",
            "--bin",
            "deriv_bot",
            "--",
            "--mode",
            "simulation",
            "--symbols",
            "R_100",
            "--history",
            "fixtures/synthetic",
        ],
        &[
            ("DERIV_AUDIT_JOURNAL_DIR", &audit_s),
            ("DERIV_RANDOM_SEED", "20260803"),
            ("SIM_EXPECT_REPLAY_HASH", "44effca3edc5f553"),
        ],
    )?;
    run(
        "cargo",
        &[
            "check",
            "--locked",
            "--features",
            "live-trading",
            "--bin",
            "deriv_bot",
        ],
    )?;
    if exists("cargo-audit") {
        run("cargo", &["audit"])?;
    } else {
        println!("SKIP cargo-audit: install cargo-audit");
    }
    reproducible_build(&source)?;
    run("sha256sum", &["Cargo.lock"])?;
    fs::remove_dir_all(directory)?;
    println!("PASS: engineering evaluation complete; no profitability/live approval inferred");
    Ok(())
}
fn package(root: &Path) -> Result {
    env::set_current_dir(root)?;
    let licence = env::var("FINAL_BUYER_LICENCE")
        .map(PathBuf::from)
        .map_err(|_| "FINAL_BUYER_LICENCE is required")?;
    require(&licence)?;
    if env::var("FINAL_BUYER_LICENCE_REVIEWED").as_deref() != Ok("YES") {
        return Err("FINAL_BUYER_LICENCE_REVIEWED=YES is required".into());
    }
    if fs::canonicalize(&licence)? == fs::canonicalize(root.join("LICENSE"))? {
        return Err("superseded root LICENSE is excluded".into());
    }
    let text = fs::read_to_string(&licence)?.to_ascii_uppercase();
    if ["DRAFT", "NOT SIGNED", "SUPERSEDED"]
        .iter()
        .any(|word| text.contains(word))
    {
        return Err("buyer licence appears draft or superseded".into());
    }
    run("cargo", &["cyclonedx", "--format", "json"])?;
    let stage = temporary("data-room");
    for dir in ["source", "evidence", "legal", "media"] {
        fs::create_dir_all(stage.join(dir))?;
    }
    for item in [
        "Cargo.toml",
        "Cargo.lock",
        "Dockerfile",
        "docker-compose.yml",
        "Caddyfile",
        "README.md",
        "PRODUCT_API.md",
        "BACKTESTING.md",
        "CLAIMS.md",
        "BUYER_ONBOARDING.md",
        "BUYER_FULL_SETUP.md",
        "BUYER_FULL_SETUP_SI.md",
    ] {
        copy_file(&root.join(item), &stage.join("source").join(item))?;
    }
    for item in ["src", "config", "schemas", "web", "fixtures"] {
        copy_tree(&root.join(item), &stage.join("source").join(item))?;
    }
    for item in [
        "SECURITY_EVIDENCE.md",
        "THIRD_PARTY_LICENSES.md",
        "THIRD_PARTY_LICENSES.tsv",
    ] {
        copy_file(&root.join(item), &stage.join("evidence").join(item))?;
    }
    copy_file(
        &root.join("deriv_bot.cdx.json"),
        &stage.join("evidence/SBOM.cdx.json"),
    )?;
    copy_file(
        &licence,
        &stage.join("legal/EXCLUSIVE_PROPRIETARY_USAGE_LICENCE.md"),
    )?;
    for item in ["COPYRIGHT", "AUTHORSHIP_DECLARATION.md"] {
        copy_file(&root.join(item), &stage.join("legal").join(item))?;
    }
    copy_file(
        &root.join("AUDIT.md"),
        &stage.join("legal/DERIV_API_TERMS_ENGINEERING_MEMO.md"),
    )?;
    for entry in fs::read_dir(root.join("data-room"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|v| v == "md") {
            copy_file(&entry.path(), &stage.join(entry.file_name()))?;
        }
    }
    fs::write(stage.join("media/STATUS.txt"), "Demo video: MISSING - record from a clean demo build.\nProduct screenshots: MISSING - capture from the buyer-verifiable dashboard build.\n")?;
    let out = root.join("buyer-data-room");
    fs::create_dir_all(&out)?;
    let archive = out.join("deriv-bot-data-room-1.0.0.tar.gz");
    let stage_s = stage.to_string_lossy();
    let archive_s = archive.to_string_lossy();
    run("tar", &["-C", &stage_s, "-czf", &archive_s, "."])?;
    run("sha256sum", &[&archive_s])?;
    fs::remove_dir_all(stage)?;
    println!("{}", archive.display());
    Ok(())
}
fn usage() {
    eprintln!(
        "usage: deriv_ops <release-readiness|reproducible-build|buyer-evaluate|package-data-room>"
    );
}
fn main() -> Result {
    let command = env::args().nth(1).unwrap_or_default();
    let root = root()?;
    match command.as_str() {
        "release-readiness" => release_readiness(&root),
        "reproducible-build" => reproducible_build(&root),
        "buyer-evaluate" => buyer_evaluate(&root),
        "package-data-room" => package(&root),
        _ => {
            usage();
            Err(io::Error::other("unknown command").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_asset_rejects_missing_and_empty_files() {
        let path = temporary("ops-require-test");
        let _ = fs::remove_file(&path);
        assert!(require(&path).is_err());
        fs::write(&path, []).unwrap();
        assert!(require(&path).is_err());
        fs::write(&path, b"ok").unwrap();
        assert!(require(&path).is_ok());
        fs::remove_file(path).unwrap();
    }
}
