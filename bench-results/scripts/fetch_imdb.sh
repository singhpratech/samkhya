#!/usr/bin/env bash
# bench-results/scripts/fetch_imdb.sh
# Reproducibility driver for bench-results/12_job_slow.md.
#
# Fetches the IMDb CSV dump bundled with the Leis et al. VLDB 2015
# Join-Order-Benchmark (JOB), verifies size, and optionally re-encodes
# every table to Parquet for fast cold-start in the samkhya-bench
# runner.
#
# Canonical source (CWI mirror used by the JOB authors):
#   https://event.cwi.nl/da/job/imdb.tgz
#
# Note: the older URL https://homepages.cwi.nl/~boncz/job/imdb.tgz now
# returns 404 (verified 2026-05-16). The event.cwi.nl path is the live
# mirror maintained by the JOB authors.
#
# Compressed size: ~1.2 GB
# Unzipped size:   ~3.7 GB across 21 CSV files
#
# Idempotent: re-running with the same DEST is a no-op if the dump is
# already extracted. Force re-fetch with FORCE=1.
#
# Usage:
#   bench-results/scripts/fetch_imdb.sh
#   DEST=/tmp/imdb bench-results/scripts/fetch_imdb.sh
#   PARQUET=1 bench-results/scripts/fetch_imdb.sh   # also pre-Parquet
#   URL=https://alt.example/imdb.tgz bench-results/scripts/fetch_imdb.sh
#   EXPECTED_SHA256=... bench-results/scripts/fetch_imdb.sh

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")"/../.. && pwd)"
DEST="${DEST:-$ROOT/samkhya-bench/data/job}"
URL="${URL:-https://event.cwi.nl/da/job/imdb.tgz}"
# SHA-256 of the canonical imdb.tgz pinned 2026-05-16 against the
# event.cwi.nl mirror. Override via env when fetching from a custom URL
# with a known-different digest.
EXPECTED_SHA256="${EXPECTED_SHA256:-25f9d893c54f903366e0c263f88db0d429dbc2b159d4987ebc1e203242a7e988}"
TARBALL="$DEST/imdb.tgz"
FORCE="${FORCE:-0}"
PARQUET="${PARQUET:-0}"

# The 21 IMDb tables expected to land as CSV after extraction.
TABLES=(
  aka_name aka_title cast_info char_name comp_cast_type company_name
  company_type complete_cast info_type keyword kind_type link_type
  movie_companies movie_info movie_info_idx movie_keyword movie_link
  name person_info role_type title
)

mkdir -p "$DEST"

already_extracted() {
  local n=0
  for t in "${TABLES[@]}"; do
    [[ -f "$DEST/$t.csv" ]] && n=$((n + 1))
  done
  [[ $n -eq ${#TABLES[@]} ]]
}

if already_extracted && [[ "$FORCE" != "1" ]]; then
  echo "[fetch_imdb] all 21 CSVs already present in $DEST; skip (FORCE=1 to overwrite)"
else
  echo "[fetch_imdb] downloading $URL -> $TARBALL"
  curl -fL --retry 3 --retry-delay 5 -o "$TARBALL" "$URL"

  echo "[fetch_imdb] verifying sha256 (expect $EXPECTED_SHA256)"
  ACTUAL_SHA256="$(sha256sum "$TARBALL" | awk '{print $1}')"
  echo "[fetch_imdb] actual sha256: $ACTUAL_SHA256"
  if [[ "$ACTUAL_SHA256" != "$EXPECTED_SHA256" ]]; then
    echo "[fetch_imdb] FATAL: sha256 mismatch for $TARBALL" >&2
    echo "  expected: $EXPECTED_SHA256" >&2
    echo "  actual:   $ACTUAL_SHA256" >&2
    echo "  (override EXPECTED_SHA256 only if you intentionally fetched a different tarball)" >&2
    rm -f "$TARBALL"
    exit 2
  fi

  echo "[fetch_imdb] extracting into $DEST"
  tar xzf "$TARBALL" -C "$DEST"
  rm -f "$TARBALL"
fi

echo "[fetch_imdb] CSV inventory:"
for t in "${TABLES[@]}"; do
  f="$DEST/$t.csv"
  if [[ -f "$f" ]]; then
    sz=$(stat -c '%s' "$f")
    printf "  %-18s %12d bytes\n" "$t" "$sz"
  else
    printf "  %-18s MISSING\n" "$t"
  fi
done

if [[ "$PARQUET" == "1" ]]; then
  if ! command -v duckdb >/dev/null 2>&1; then
    echo "[fetch_imdb] PARQUET=1 requested but duckdb not on PATH; skip re-encoding" >&2
    exit 0
  fi
  mkdir -p "$DEST/parquet"
  echo "[fetch_imdb] re-encoding CSV -> Parquet via duckdb"
  for t in "${TABLES[@]}"; do
    out="$DEST/parquet/$t.parquet"
    if [[ -f "$out" && "$FORCE" != "1" ]]; then
      echo "  $t: already Parquet"
      continue
    fi
    echo "  $t: csv -> parquet"
    duckdb -c "COPY (SELECT * FROM read_csv_auto('$DEST/$t.csv', header=false, escape='\\\\', delim=',')) TO '$out' (FORMAT PARQUET, COMPRESSION zstd);"
  done
fi

echo "[fetch_imdb] done. Next step:"
echo "  cargo run -p samkhya-bench --release -- run --suite job-slow-real --imdb-dir $DEST"
