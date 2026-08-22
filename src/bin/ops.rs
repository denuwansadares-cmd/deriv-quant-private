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
    let mut source = fs::File::open(from)?;
    let mut destination = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    io::copy(&mut source, &mut destination)?;
    destination.sync_all()?;
    Ok(())
}
fn is_delivery_source(path: &Path) -> bool {
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return false;
    }
    const FILES: [&str; 19] = [
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
        "BUYER_EVIDENCE_INDEX.md",
        "RELEASE_READINESS.md",
        "DEMO_SCRIPT.md",
        "SUBMISSION_SCOPE.md",
        "deny.toml",
        "about.toml",
        "about.hbs",
    ];
    FILES.iter().any(|candidate| path == Path::new(candidate))
        || ["src", "config", "schemas", "web", "fixtures"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
}
fn copy_tracked_source(root: &Path, destination: &Path) -> Result {
    let output = Command::new("git")
        .args([
            "-C",
            root.to_str().ok_or("non-UTF-8 repository root")?,
            "ls-files",
            "-z",
        ])
        .output()?;
    ensure(output.status, "git ls-files")?;
    let mut copied = 0usize;
    for raw in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative = PathBuf::from(std::str::from_utf8(raw)?);
        if !is_delivery_source(&relative) {
            continue;
        }
        let source = root.join(&relative);
        let metadata = fs::symlink_metadata(&source)?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "delivery source is not a regular file: {}",
                relative.display()
            )
            .into());
        }
        copy_file(&source, &destination.join(&relative))?;
        copied += 1;
    }
    if copied == 0 {
        return Err("delivery source allowlist selected no tracked files".into());
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
        "BUYER_EVIDENCE_INDEX.md",
        "RELEASE_READINESS.md",
        "DEMO_SCRIPT.md",
        "THIRD_PARTY_LICENSES.md",
        "THIRD_PARTY_NOTICES.md",
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
    run(
        "cargo",
        &["check", "--locked", "--workspace", "--all-targets"],
    )?;
    run(
        "cargo",
        &["build", "--locked", "--workspace", "--all-targets"],
    )?;
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
    run(
        "cargo",
        &["test", "--locked", "--workspace", "--all-targets"],
    )?;
    run(
        "cargo",
        &[
            "check",
            "--locked",
            "--workspace",
            "--all-targets",
            "--features",
            "live-trading",
        ],
    )?;
    run(
        "cargo",
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--features",
            "live-trading",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(
        "cargo",
        &[
            "test",
            "--locked",
            "--workspace",
            "--all-targets",
            "--features",
            "live-trading",
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
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn generate_notices(root: &Path) -> Result {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .ok_or("CARGO_HOME and HOME are unavailable")?;
    let registry = cargo_home.join("registry/src");
    let mut registry_roots = fs::read_dir(&registry)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    registry_roots.sort();
    let inventory = fs::read_to_string(root.join("THIRD_PARTY_LICENSES.tsv"))?;
    let mut rows = inventory
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    if rows.len() != 217 || rows.iter().any(|row| row.len() != 4) {
        return Err("third-party inventory must contain exactly 217 four-column rows".into());
    }
    rows.sort_by(|left, right| (left[0], left[1]).cmp(&(right[0], right[1])));
    let mut output = String::from("# Third-party distribution notices\n\nGenerated deterministically from `Cargo.lock`, `THIRD_PARTY_LICENSES.tsv`, and the checksum-verified local Cargo registry. Licence expressions are preserved as declared; inclusion does not select an `OR` alternative or provide legal advice. Copyright and attribution below belong to their respective third-party authors.\n\n");
    let mut without_packaged_text = Vec::new();
    for row in rows {
        let (name, version, expression, source) = (row[0], row[1], row[2], row[3]);
        let directory_name = format!("{name}-{version}");
        let crate_dir = registry_roots
            .iter()
            .map(|registry_root| registry_root.join(&directory_name))
            .find(|candidate| candidate.is_dir())
            .ok_or_else(|| format!("locked crate source unavailable: {directory_name}"))?;
        let mut files = fs::read_dir(&crate_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_ascii_uppercase();
                ["LICENSE", "COPYING", "NOTICE", "AUTHORS", "COPYRIGHT"]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        files.sort();
        output.push_str(&format!("## `{name}` `{version}`\n\n- Source: {source}\n- Declared licence expression: `{expression}`\n"));
        if files.is_empty() {
            without_packaged_text.push(format!("{name} {version}"));
            output.push_str("- Packaged licence/NOTICE files: none; **MANUAL REVIEW REQUIRED**. The declared MIT/Apache alternatives are reproduced by other components in this bundle, but the crate package itself supplied no licence file.\n\n");
        } else {
            output.push_str("- Packaged licence/NOTICE files follow.\n\n");
            for file in files {
                let relative = file.strip_prefix(&crate_dir)?;
                let bytes = fs::read(&file)?;
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| format!("non-UTF-8 notice file: {}", relative.display()))?;
                output.push_str(&format!(
                    "### `{}`\n\n<pre>{}</pre>\n\n",
                    relative.display(),
                    html_escape(text)
                ));
            }
        }
    }
    output.push_str(&format!("## Reconciliation summary\n\n- Locked third-party components: 217\n- Notice sections: 217\n- Components without a packaged licence/NOTICE file: {}\n", without_packaged_text.len()));
    for component in without_packaged_text {
        output.push_str(&format!("- MANUAL REVIEW: `{component}`\n"));
    }
    fs::write(root.join("THIRD_PARTY_NOTICES.md"), output)?;
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
    copy_tracked_source(root, &stage.join("source"))?;
    for item in [
        "SECURITY_EVIDENCE.md",
        "THIRD_PARTY_LICENSES.md",
        "THIRD_PARTY_LICENSES.tsv",
        "THIRD_PARTY_NOTICES.md",
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
    let extracted = temporary("data-room-extracted");
    fs::create_dir_all(&extracted)?;
    let extracted_s = extracted.to_string_lossy();
    run("tar", &["-C", &extracted_s, "-xzf", &archive_s])?;
    buyer_evaluate(&extracted)?;
    fs::remove_dir_all(extracted)?;
    env::set_current_dir(root)?;
    run("sha256sum", &[&archive_s])?;
    fs::remove_dir_all(stage)?;
    println!("{}", archive.display());
    Ok(())
}
fn usage() {
    eprintln!(
        "usage: deriv_ops <release-readiness|reproducible-build|buyer-evaluate|normalize-sbom|generate-notices|package-data-room>"
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
        "generate-notices" => generate_notices(&root),
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
    fn delivery_allowlist_excludes_untracked_and_unlisted_files() {
        assert!(!is_delivery_source(Path::new("../src/main.rs")));
        assert!(!is_delivery_source(Path::new("src/../private.txt")));
        assert!(!is_delivery_source(Path::new("/src/main.rs")));
        let root = temporary("ops-package-source-test");
        let destination = temporary("ops-package-destination-test");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("fixtures/synthetic")).unwrap();
        fs::write(root.join("Cargo.toml"), b"tracked root").unwrap();
        fs::write(root.join("src/main.rs"), b"tracked source").unwrap();
        fs::write(root.join("src/private.log"), b"untracked secret").unwrap();
        fs::write(
            root.join("fixtures/synthetic/private.csv"),
            b"untracked data",
        )
        .unwrap();
        fs::write(root.join("UNLISTED.md"), b"tracked but unlisted").unwrap();
        assert!(Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["add", "Cargo.toml", "src/main.rs", "UNLISTED.md"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        copy_tracked_source(&root, &destination).unwrap();
        assert_eq!(
            fs::read(destination.join("Cargo.toml")).unwrap(),
            b"tracked root"
        );
        assert_eq!(
            fs::read(destination.join("src/main.rs")).unwrap(),
            b"tracked source"
        );
        assert!(!destination.join("src/private.log").exists());
        assert!(!destination.join("fixtures/synthetic/private.csv").exists());
        assert!(!destination.join("UNLISTED.md").exists());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn package_copy_rejects_destination_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root = temporary("ops-package-symlink-test");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        let outside = root.join("outside");
        let destination = root.join("destination");
        fs::write(&source, b"package content").unwrap();
        fs::write(&outside, b"outside remains unchanged").unwrap();
        symlink(&outside, &destination).unwrap();
        assert!(copy_file(&source, &destination).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"outside remains unchanged");
        fs::remove_dir_all(root).unwrap();
    }

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
