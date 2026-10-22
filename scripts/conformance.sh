#!/bin/sh
# Differential conformance run against the reference daff (npm `daff@1.4.2`).
# Usage: scripts/conformance.sh [N]   (N random cases per mode, default 100)
set -e
N=${1:-100}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ORACLE=${ORACLE_DIR:-/tmp/daff-oracle}
if [ ! -f "$ORACLE/node_modules/daff/bin/daff.js" ]; then
    mkdir -p "$ORACLE" && (cd "$ORACLE" && npm init -y >/dev/null && npm install daff@1.4.2)
fi
REF="$ORACLE/node_modules/daff/bin/daff.js"
cargo build --release --manifest-path "$ROOT/Cargo.toml"
OURS="$ROOT/target/release/daff"
for mode in diff patch patch3 merge html color render copy; do
    printf '%-7s ' "$mode:"
    MODE=$mode THREE=0.3 python3 "$ROOT/scripts/fuzz_oracle.py" "$REF" "$OURS" "$N" 1 | tail -1
done
printf '%-7s ' "json:"
python3 "$ROOT/scripts/fuzz_json.py" "$REF" "$OURS" "$N" 1 | tail -1
