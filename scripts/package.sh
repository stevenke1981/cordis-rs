#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NAME="cordis-rs-0.6.0-alpha.1"
OUT="${1:-$ROOT/dist}"
mkdir -p "$OUT"
cd "$ROOT"
python3 scripts/static_audit.py --require-git-clean --report "$OUT/STATIC_AUDIT_REPORT.json"
git archive --format=tar.gz --prefix="$NAME/" -o "$OUT/$NAME-source.tar.gz" HEAD
git bundle create "$OUT/$NAME.bundle" --all
sha256sum \
  "$OUT/$NAME-source.tar.gz" \
  "$OUT/$NAME.bundle" \
  "$OUT/STATIC_AUDIT_REPORT.json" > "$OUT/SHA256SUMS"
printf 'Created %s\n' "$OUT/$NAME-source.tar.gz"
