use std::{
    collections::{HashMap, HashSet},
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

fn stable_component_token(value: &str, label: &str) -> Result<String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(format!("invalid SBOM {label}: {value:?}").into());
    }
    Ok(value.to_owned())
}
fn replace_exact_strings(value: &mut serde_json::Value, replacements: &HashMap<String, String>) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(replacement) = replacements.get(text) {
                *text = replacement.clone();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                replace_exact_strings(value, replacements);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                replace_exact_strings(value, replacements);
            }
        }
        _ => {}
    }
}
fn collect_bom_refs(value: &serde_json::Value, refs: &mut Vec<String>) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                collect_bom_refs(value, refs);
            }
        }
        serde_json::Value::Object(values) => {
            if let Some(reference) = values.get("bom-ref").and_then(|value| value.as_str()) {
                refs.push(reference.to_owned());
            }
            for value in values.values() {
                collect_bom_refs(value, refs);
            }
        }
        _ => {}
    }
}
fn contains_local_or_absolute_path(value: &serde_json::Value, forbidden: &[String]) -> bool {
    match value {
        serde_json::Value::String(text) => {
            let windows_absolute = text.as_bytes().get(1) == Some(&b':')
                && matches!(text.as_bytes().get(2), Some(b'\\' | b'/'));
            text.starts_with('/')
                || windows_absolute
                || text.contains("path+file://")
                || text.contains("file:///")
                || forbidden
                    .iter()
                    .any(|fragment| !fragment.is_empty() && text.contains(fragment))
        }
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| contains_local_or_absolute_path(value, forbidden)),
        serde_json::Value::Object(values) => values
            .values()
            .any(|value| contains_local_or_absolute_path(value, forbidden)),
        _ => false,
    }
}
fn normalize_sbom(value: &mut serde_json::Value, root: &Path) -> Result<HashMap<String, String>> {
    let application = value
        .pointer_mut("/metadata/component")
        .and_then(|value| value.as_object_mut())
        .ok_or("SBOM metadata.component is missing")?;
    let package_name = stable_component_token(
        application
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("SBOM application name is missing")?,
        "application name",
    )?;
    let package_version = stable_component_token(
        application
            .get("version")
            .and_then(|value| value.as_str())
            .ok_or("SBOM application version is missing")?,
        "application version",
    )?;
    let old_root_ref = application
        .get("bom-ref")
        .and_then(|value| value.as_str())
        .ok_or("SBOM application bom-ref is missing")?
        .to_owned();
    let new_root_ref = format!("urn:cdx:cargo:{package_name}:{package_version}:application");
    let mut replacements = HashMap::from([(old_root_ref, new_root_ref)]);
    let mut stable_refs = HashSet::new();
    if !stable_refs.insert(replacements.values().next().unwrap().clone()) {
        return Err("duplicate normalized SBOM application reference".into());
    }
    let binaries = application
        .get_mut("components")
        .and_then(|value| value.as_array_mut())
        .ok_or("SBOM application binary components are missing")?;
    for binary in binaries {
        let binary = binary
            .as_object_mut()
            .ok_or("SBOM binary component is malformed")?;
        if binary.get("type").and_then(|value| value.as_str()) != Some("application") {
            return Err("unexpected non-application workspace component".into());
        }
        let binary_name = stable_component_token(
            binary
                .get("name")
                .and_then(|value| value.as_str())
                .ok_or("SBOM binary name is missing")?,
            "binary name",
        )?;
        let binary_version = binary
            .get("version")
            .and_then(|value| value.as_str())
            .ok_or("SBOM binary version is missing")?;
        if binary_version != package_version {
            return Err("SBOM binary version differs from package version".into());
        }
        let old_ref = binary
            .get("bom-ref")
            .and_then(|value| value.as_str())
            .ok_or("SBOM binary bom-ref is missing")?
            .to_owned();
        let new_ref =
            format!("urn:cdx:cargo:{package_name}:{package_version}:binary:{binary_name}");
        if !stable_refs.insert(new_ref.clone()) || replacements.insert(old_ref, new_ref).is_some() {
            return Err("duplicate SBOM binary reference".into());
        }
    }
    replace_exact_strings(value, &replacements);

    let mut bom_refs = Vec::new();
    collect_bom_refs(value, &mut bom_refs);
    let unique_refs: HashSet<&str> = bom_refs.iter().map(String::as_str).collect();
    if unique_refs.len() != bom_refs.len() {
        return Err("duplicate SBOM bom-ref".into());
    }
    let dependencies = value
        .get("dependencies")
        .and_then(|value| value.as_array())
        .ok_or("SBOM dependency graph is missing")?;
    for dependency in dependencies {
        let reference = dependency
            .get("ref")
            .and_then(|value| value.as_str())
            .ok_or("SBOM dependency ref is missing")?;
        if !unique_refs.contains(reference) {
            return Err(format!("unresolved SBOM dependency ref: {reference}").into());
        }
        for target in dependency
            .get("dependsOn")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            let target = target.as_str().ok_or("malformed SBOM dependsOn ref")?;
            if !unique_refs.contains(target) {
                return Err(format!("unresolved SBOM dependsOn ref: {target}").into());
            }
        }
    }
    let mut forbidden = vec![
        root.to_string_lossy().into_owned(),
        env::temp_dir().to_string_lossy().into_owned(),
    ];
    for name in ["HOME", "USER", "USERNAME", "HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = env::var(name) {
            if value.len() >= 5 {
                forbidden.push(value);
            }
        }
    }
    if contains_local_or_absolute_path(value, &forbidden) {
        return Err(
            "SBOM contains a local, absolute, or environment-specific path/identity".into(),
        );
    }
    Ok(replacements)
}
fn normalize_sbom_file(root: &Path) -> Result {
    let path = root.join("deriv_bot.cdx.json");
    let input = fs::read_to_string(&path)?;
    let mut normalized: serde_json::Value = serde_json::from_str(&input)?;
    let replacements = normalize_sbom(&mut normalized, root)?;
    let mut replacements = replacements.into_iter().collect::<Vec<_>>();
    replacements.sort_by_key(|(old, _)| std::cmp::Reverse(old.len()));
    let mut output = input;
    for (old, new) in replacements {
        output = output.replace(&old, &new);
    }
    let mut reparsed: serde_json::Value = serde_json::from_str(&output)?;
    normalize_sbom(&mut reparsed, root)?;
    if reparsed != normalized {
        return Err("SBOM reference normalization was incomplete".into());
    }
    fs::write(path, output)?;
    Ok(())
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
        "SUBMISSION_SCOPE.md",
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
    let former_root_licence = root.join("LICENSE");
    if former_root_licence.exists()
        && fs::canonicalize(&licence)? == fs::canonicalize(&former_root_licence)?
    {
        return Err("the former root LICENSE is excluded".into());
    }
    let text = fs::read_to_string(&licence)?.to_ascii_uppercase();
    if ["DRAFT", "NOT SIGNED", "SUPERSEDED"]
        .iter()
        .any(|word| text.contains(word))
    {
        return Err("buyer licence appears draft or superseded".into());
    }
    run(
        "cargo",
        &["cyclonedx", "--target", "all", "--format", "json"],
    )?;
    normalize_sbom_file(root)?;
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
        "SUBMISSION_SCOPE.md",
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
        "usage: deriv_ops <release-readiness|reproducible-build|buyer-evaluate|normalize-sbom|package-data-room>"
    );
}
fn main() -> Result {
    let command = env::args().nth(1).unwrap_or_default();
    let root = root()?;
    match command.as_str() {
        "release-readiness" => release_readiness(&root),
        "reproducible-build" => reproducible_build(&root),
        "buyer-evaluate" => buyer_evaluate(&root),
        "normalize-sbom" => normalize_sbom_file(&root),
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
    #[test]
    fn sbom_normalization_is_stable_and_fail_closed() {
        let fixture = serde_json::json!({
            "metadata": {"component": {
                "type": "application",
                "bom-ref": "path+file:///checkout/one#1.0.0",
                "name": "deriv_bot",
                "version": "1.0.0",
                "components": [
                    {"type":"application","bom-ref":"path+file:///checkout/one#1.0.0 bin-target-0","name":"deriv_bot","version":"1.0.0"},
                    {"type":"application","bom-ref":"path+file:///checkout/one#1.0.0 bin-target-1","name":"deriv_ops","version":"1.0.0"}
                ]
            }},
            "components": [{"type":"library","bom-ref":"registry#index#serde@1.0.0","name":"serde","version":"1.0.0","licenses":[{"expression":"MIT OR Apache-2.0"}]}],
            "dependencies": [
                {"ref":"path+file:///checkout/one#1.0.0","dependsOn":["registry#index#serde@1.0.0"]},
                {"ref":"registry#index#serde@1.0.0","dependsOn":[]}
            ]
        });
        let mut first = fixture.clone();
        let mut second = fixture;
        normalize_sbom(&mut first, Path::new("/checkout/one")).unwrap();
        normalize_sbom(&mut second, Path::new("/different/location")).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first.pointer("/metadata/component/bom-ref").unwrap(),
            "urn:cdx:cargo:deriv_bot:1.0.0:application"
        );
        assert_eq!(
            first
                .pointer("/metadata/component/components/1/bom-ref")
                .unwrap(),
            "urn:cdx:cargo:deriv_bot:1.0.0:binary:deriv_ops"
        );
        assert_eq!(
            first
                .pointer("/components/0/licenses/0/expression")
                .unwrap(),
            "MIT OR Apache-2.0"
        );
        assert_eq!(
            first.pointer("/dependencies/0/ref").unwrap(),
            "urn:cdx:cargo:deriv_bot:1.0.0:application"
        );

        let mut duplicate = first.clone();
        duplicate["metadata"]["component"]["components"][1]["name"] =
            serde_json::json!("deriv_bot");
        assert!(normalize_sbom(&mut duplicate, Path::new("/checkout")).is_err());

        let mut unresolved = first.clone();
        unresolved["dependencies"][0]["dependsOn"] = serde_json::json!(["missing"]);
        assert!(normalize_sbom(&mut unresolved, Path::new("/checkout")).is_err());

        let mut location = first;
        location["metadata"]["component"]["local"] = serde_json::json!("/private/path");
        assert!(normalize_sbom(&mut location, Path::new("/checkout")).is_err());
    }
}
