#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/buyer-data-room"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/source" "$stage/evidence" "$stage/legal" "$stage/media" "$stage/scripts" "$out"
cd "$root"
cargo cyclonedx --format json
cp Cargo.toml Cargo.lock Dockerfile docker-compose.yml PRODUCT_API.md BACKTESTING.md CLAIMS.md BUYER_ONBOARDING.md "$stage/source/"
mkdir -p "$stage/source/src"
cp src/main.rs src/modules.rs src/num_serde.rs "$stage/source/src/"
cp -R src/modules src/bin "$stage/source/src/"
cp -R config schemas web fixtures "$stage/source/"
cp SECURITY_EVIDENCE.md THIRD_PARTY_LICENSES.md THIRD_PARTY_LICENSES.tsv "$stage/evidence/"
cp deriv_bot.cdx.json "$stage/evidence/SBOM.cdx.json"
cp LICENSE COPYRIGHT AUTHORSHIP_DECLARATION.md "$stage/legal/"
cp AUDIT.md "$stage/legal/DERIV_API_TERMS_ENGINEERING_MEMO.md"
cp scripts/reproducible-build.sh "$stage/scripts/"
cp data-room/*.md "$stage/"
cp data-room/buyer-evaluate.sh "$stage/scripts/"
printf '%s
' 'Demo video: MISSING  record from a clean demo build.' 'Product screenshots: MISSING  capture from buyer-verifiable dashboard build.' > "$stage/media/STATUS.txt"
find "$stage" -type f -print0 | sort -z | xargs -0 sha256sum > "$stage/MANIFEST.sha256"
archive="$out/deriv-bot-data-room-1.0.0.tar.gz"
tar -C "$stage" -czf "$archive" .
sha256sum "$archive" > "$archive.sha256"
echo "$archive"
