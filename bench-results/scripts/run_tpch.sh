#!/usr/bin/env bash
# bench-results/scripts/run_tpch.sh — TPC-H SF=1 campaign runner (scaffold).
#
# Companion to bench-results/13_tpc_h_1gb.md (section 6). This script is
# idempotent and intentionally aborts at the first missing precondition so
# that "did the run actually happen?" is unambiguous.
#
# Steps:
#   1. Toolchain check (duckdb CLI, cargo)
#   2. Generate SF=1 Parquet via duckdb tpch extension
#   3. Build Puffin sidecars for all 8 TPC-H tables
#   4. CPU-governor gate (refuse to run if not "performance")
#   5. Execute 22 queries x 2 modes x 30 replicates
#   6. Emit raw JSON + a sibling report 13_tpc_h_1gb_run.md
#
# Sole author: Prateek Singh. License: Apache-2.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TPCH_ROOT="${TPCH_ROOT:-${REPO_ROOT}/tpch-sf1}"
RAW_DIR="${REPO_ROOT}/bench-results/13_tpc_h_1gb/raw"
PLAN_DIR="${REPO_ROOT}/bench-results/13_tpc_h_1gb/plans"
REPLICATES="${REPLICATES:-30}"
WARMUP="${WARMUP:-3}"
SF="${SF:-1}"

die() { printf 'run_tpch.sh: FATAL: %s\n' "$*" >&2; exit 1; }
info() { printf 'run_tpch.sh: %s\n' "$*"; }

# --- 1. Toolchain check ------------------------------------------------------
need_bin() {
  local b="$1"; shift
  command -v "$b" >/dev/null 2>&1 || die "missing tool '$b'. $*"
}

need_bin duckdb \
  "Install via 'curl https://install.duckdb.org | sh' or your distro's package."
need_bin cargo \
  "Install via 'rustup' (rust-toolchain.toml pins the channel)."
need_bin sha256sum "Should be in coreutils on any Linux."

# --- 4. CPU governor gate (checked early so we fail fast) --------------------
if [[ -r /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]]; then
  gov=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)
  if [[ "$gov" != "performance" ]]; then
    cat >&2 <<EOF
run_tpch.sh: CPU governor is '$gov', need 'performance'.
  Run (requires sudo):
    sudo cpupower frequency-set -g performance
  Then re-invoke this script.
EOF
    exit 1
  fi
fi

mkdir -p "$TPCH_ROOT" "$RAW_DIR" "$PLAN_DIR/native" "$PLAN_DIR/samkhya"

# --- 2. Generate SF=1 (idempotent) -------------------------------------------
checksum_file="$TPCH_ROOT/_checksums.sha256"
TABLES=(lineitem orders partsupp part customer supplier nation region)

generate_needed=0
if [[ ! -f "$checksum_file" ]]; then
  generate_needed=1
else
  for t in "${TABLES[@]}"; do
    if [[ ! -f "$TPCH_ROOT/$t.parquet" ]]; then
      generate_needed=1
      break
    fi
  done
  if [[ $generate_needed -eq 0 ]]; then
    info "validating existing TPC-H SF=$SF parquet against $checksum_file"
    if ! (cd "$TPCH_ROOT" && sha256sum -c --quiet _checksums.sha256); then
      die "checksum mismatch in $TPCH_ROOT; refusing to proceed. Delete the directory to regenerate."
    fi
  fi
fi

if [[ $generate_needed -eq 1 ]]; then
  info "generating TPC-H SF=$SF parquet under $TPCH_ROOT (this can take ~1-2 min at SF=1)"
  duckdb -c "
    INSTALL tpch;
    LOAD tpch;
    CALL dbgen(sf=$SF);
    EXPORT DATABASE '$TPCH_ROOT' (FORMAT PARQUET, COMPRESSION ZSTD);
  " >/dev/null
  (cd "$TPCH_ROOT" && sha256sum *.parquet > _checksums.sha256)
fi

# --- 3. Build Puffin sidecars (one per table) --------------------------------
mkdir -p "$TPCH_ROOT/puffin"
for t in "${TABLES[@]}"; do
  out="$TPCH_ROOT/puffin/$t.puffin"
  if [[ -f "$out" ]]; then
    info "puffin sidecar for $t already present, skipping"
    continue
  fi
  info "building puffin sidecar for $t"
  cargo run --quiet --release -p samkhya-cli -- build-puffin \
    --table "$t" \
    --input "$TPCH_ROOT/$t.parquet" \
    --output "$out"
done

# --- 5. Execute the 22 x 2 x N grid ------------------------------------------
# Requires samkhya-bench Suite::TpcH wiring (see section 6.1 of 13_tpc_h_1gb.md).
# Until that lands, the bench binary will fail fast with a "TpcH suite not yet
# wired" error and this script will exit non-zero. That is the intended
# behaviour — no projections silently become "measured".

info "executing 22 queries x 2 modes x $REPLICATES replicates (warmup=$WARMUP)"
cargo run --quiet --release -p samkhya-bench -- \
  --suite tpc-h \
  --parquet-dir "$TPCH_ROOT" \
  --puffin-dir "$TPCH_ROOT/puffin" \
  --replicates "$REPLICATES" \
  --warmup "$WARMUP" \
  --raw-out "$RAW_DIR" \
  --plan-out-native "$PLAN_DIR/native" \
  --plan-out-samkhya "$PLAN_DIR/samkhya" \
  --drop-caches-between-replicates

# --- 6. Render the run-sibling report ----------------------------------------
# The bench binary writes a structured JSON summary to $RAW_DIR/summary.json;
# render_tpch_report consumes it and emits a sibling markdown file. This is
# deliberately separate from 13_tpc_h_1gb.md so the projection-vs-measurement
# audit trail stays explicit.
info "rendering 13_tpc_h_1gb_run.md from $RAW_DIR/summary.json"
cargo run --quiet --release -p samkhya-bench --bin render_tpch_report -- \
  --summary "$RAW_DIR/summary.json" \
  --out "$REPO_ROOT/bench-results/13_tpc_h_1gb_run.md"

info "done. Review bench-results/13_tpc_h_1gb_run.md and decide whether to update 13_tpc_h_1gb.md."
