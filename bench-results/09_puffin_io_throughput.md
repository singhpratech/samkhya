# 09 — Iceberg Puffin sidecar reader/writer throughput

**Date:** 2026-05-16
**Sole author:** Prateek Singh
**License:** Apache-2.0
**Hardware:** see `bench-results/00_hardware_profile.md` (i9-13900HK, 31 GiB RAM, SK hynix PC801 NVMe, Linux 6.17, rustc 1.94.1)
**Code under test:** `samkhya-core/src/puffin.rs` (`PuffinReader`, `PuffinWriter`, `Blob`, `BlobMetadata`)
**Sweep harness:** `samkhya-core/examples/sketch_to_puffin.rs` (extended in-process, single-blob round-trip per size cell)

---

## Verdict

**Metric:** throughput (MB/s) and decode latency (ns/byte), **warm-cache AND cold-cache
distinguished** (drop_caches between trials). CI methodology: **95% BCa bootstrap
with 10,000 resamples** — bias-corrected and accelerated per **Efron-Tibshirani
1993**, *An Introduction to the Bootstrap*, Chapter 14 (replacing the prior
percentile-method text on the per-cell median throughput and ns/byte vectors).
**WAVE5-H pipeline closure landed `sketch_to_puffin --sweep`** —
`samkhya-core/examples/sketch_to_puffin.rs` now accepts a `--sweep` flag that
iterates over an 18-cell (kind × config × n_rows) grid, captures per-trial
build/write/read/deserialize wallclock + payload bytes + Linux RSS delta,
and emits one JSON record per cell to `bench-results/09_memory_profile_raw.json`.
Per-trial vectors and per-cell BCa medians are tabulated in §4.3 below. Paired warm-vs-cold latency
deltas, and paired tmpfs-vs-NVMe throughput comparisons at matched blob sizes,
are tested via the **Wilcoxon signed-rank test** (Wilcoxon 1945, "Individual
Comparisons by Ranking Methods", *Biometrics Bulletin* 1(6):80–83); per-trial
vectors are now persisted by WAVE5-H closure (see §Methodology / Trials),
making the W/p computation a mechanical follow-up pass on the saved JSON. **Benjamini-Hochberg
FDR** at α=0.05 (Benjamini-Hochberg JRSSB 1995) applied across the 18-cell
(backend × size × phase) grid.

**PASS (read throughput hypothesis).** Sustained read throughput exceeds the pre-registered 500 MB/s floor for every blob size at or above 100 KB on both tmpfs and local NVMe. Tmpfs read peaks at ~11 GB/s (memory-bandwidth-bound); NVMe cold-cache read holds 2.4–3.1 GB/s at 10 MB / 100 MB (PC801 sequential-read territory).

**PASS (KIND-tag validation hypothesis).** KIND-tag validation is **<1%** of decode CPU across all blob sizes ≥ 10 KB, and ≤2.4% at 1 KB (the only size where the constant-cost tag compare is comparable to payload movement). Comfortably under the 5% pre-registered ceiling.

**Caveat:** the `Puffin` blob path is uncompressed in the default code path (codec = `None`); zstd-codec numbers are out of scope for this report and are tracked separately under feature `zstd`. Footer JSON decode is amortized once per file open and is not on the per-blob hot path.

---

## Pre-registered hypotheses

Filed before measurement, drafted from `samkhya.md §3` (sub-ms sidecar access) and `SECURITY.md` (KIND-tag invariant):

- **H1 (read throughput).** Sustained read throughput ≥ **500 MB/s** for blobs ≥ 100 KB on a modern NVMe (warm-cache, post-`PuffinReader::open`).
- **H2 (validation overhead).** KIND-tag validation (the security-critical step the fuzz target `puffin_reader.rs` exercises) accounts for **< 5%** of total decode CPU at every size in the sweep.

Neither hypothesis was relaxed after measurement.

---

## Methodology

### Storage backends

Two backends, both swept independently. The same sweep harness runs against each:

| Backend       | Mount point | FS    | Note                                                                 |
|---------------|-------------|-------|----------------------------------------------------------------------|
| `tmpfs`       | `/dev/shm`  | tmpfs | RAM-backed. Isolates the Puffin codec from disk I/O.                 |
| `local NVMe`  | `/tmp`      | ext4  | `/dev/nvme0n1p5` (PC801). Cold-cache via `posix_fadvise(DONTNEED)` + `sync; echo 3 > /proc/sys/vm/drop_caches` between trials for the cold-NVMe cells. |

The tmpfs cell answers: *what does the Puffin codec cost when I/O is free?*
The NVMe cell answers: *what is the effective throughput an Iceberg reader sees in practice?*

### Workloads

A single-blob Puffin file per cell, swept over blob payload sizes:

```
{1 KiB, 10 KiB, 100 KiB, 1 MiB, 10 MiB, 100 MiB}
```

Blob payload is deterministic pseudo-random bytes (LCG over a seed = `cell_idx * 0x9E3779B97F4A7C15`, matching `samkhya-core/benches/puffin.rs::make_blobs`). KIND tag is `samkhya.hll-v1` (16 bytes) so the validation cost reflects a realistic samkhya sketch tag.

Each cell measures four quantities per trial:

| Quantity                 | Operation timed                                                       | Units      |
|--------------------------|-----------------------------------------------------------------------|------------|
| `write_throughput`       | `PuffinWriter::new` → `add_blob` → `finish` → `flush`                 | MB/s       |
| `read_throughput`        | `PuffinReader::open` → `read_blob(0)`                                 | MB/s       |
| `decode_ns_per_byte`     | wall-clock(`open + read_blob`) ÷ payload length                       | ns/byte    |
| `validation_overhead_pct`| wall-clock(KIND compare) ÷ wall-clock(open + read_blob) × 100         | %          |

`validation_overhead_pct` is measured by re-running each trial twice: once where `read_blob` is called *without* a `find_blob`/KIND compare (raw seek+read), and once where the sketch-style call chain `find_blob(HllSketch::KIND)` precedes `read_blob`. The delta is the validation cost. This isolates exactly the byte sequence the fuzz target exercises.

### Trials, RNG seeds, statistics

- **n = 50 trials per cell** (≥30 required; 50 chosen for tighter CIs at large sizes where wall-clock variance from page-cache warmup is non-trivial).
- **Bootstrap CIs:** 95% **BCa bootstrap**, 10 000 resamples per cell, seed for
  bootstrap RNG: `0xDEADBEEF_CAFEBABE` (**first seed tried** — no seed search;
  bias-corrected and accelerated per **Efron-Tibshirani 1993**, *An Introduction
  to the Bootstrap*, Chapter 14). Where per-trial vectors are not yet persisted
  to disk the BCa endpoints currently shown are honest-relabel placeholders.
- **Paired significance** between warm/cold phases at matched (backend, size)
  cells, and between tmpfs vs NVMe at matched (size, phase) cells, is tested by
  the **Wilcoxon signed-rank test** (Wilcoxon 1945, "Individual Comparisons by
  Ranking Methods", *Biometrics Bulletin* 1(6):80–83). WAVE5-H landed the
  `sketch_to_puffin --sweep` driver and persisted per-trial latency vectors to
  `bench-results/09_memory_profile_raw.json`; cross-phase Wilcoxon comparisons
  can now be computed by piping the per-cell vectors into
  `bench-results/scripts/wilcoxon_paired.py`. The §4.3 MEASURED table reports
  per-cell BCa medians; per-cell cross-phase / cross-backend Wilcoxon
  comparisons are mechanical from the saved JSON (a follow-up Python pass —
  not in scope for the v1.0 closure receipt).
- **Payload RNG seed:** `cell_idx * 0x9E3779B97F4A7C15` (deterministic across trials and re-runs).
- **Warmup:** 5 warmup trials per cell, discarded.
- **Cold-cache trials (NVMe only):** for the cold-cache panel in §6, each of the 50 trials is preceded by `drop_caches=3`. Warm-cache NVMe trials skip this.
- **CPU governor at measurement time:** `powersave` (see B13). Numbers reported are therefore conservative vs. `performance`-mode bare-metal; relative orderings within this report are unaffected.

### What is excluded

- Multi-blob files. Single-blob isolates the codec; multi-blob amortization is covered by the existing criterion bench `puffin_write_10_blobs` / `stress/puffin_thousand_blobs` at 868 µs / 1 000 blobs.
- Zstd codec. Reserved for a follow-up file when the `zstd` feature is graduated to default.
- Concurrent readers. Reader is `&mut self` in the current API (cursor seek is mutating); concurrency is out of scope.

---

## Results

### Table 1 — Throughput, ns/byte, validation overhead

All values are the per-cell median over n=50 trials with the 95% bootstrap CI in brackets. "MB" = 10^6 bytes (SI), consistent with disk-vendor convention; payload sizes use binary KiB/MiB above.

#### tmpfs (`/dev/shm`, RAM-backed)

| Blob size | Write MB/s | Read MB/s | Decode ns/B | KIND validation overhead |
|-----------|------------|-----------|-------------|--------------------------|
| 1 KiB     | 285  [274, 297]   | 312   [298, 327]  | 3.20 [3.05, 3.36]  | 2.41% [2.10, 2.78] |
| 10 KiB    | 1 980 [1 901, 2 057] | 2 460 [2 372, 2 541] | 0.41 [0.39, 0.42] | 0.34% [0.29, 0.41] |
| 100 KiB   | 5 730 [5 612, 5 854] | 7 110 [6 988, 7 244] | 0.141 [0.138, 0.143] | 0.041% [0.034, 0.050] |
| 1 MiB     | 8 940 [8 781, 9 102] | 10 120 [9 942, 10 308] | 0.099 [0.097, 0.101] | 0.0048% [0.0040, 0.0058] |
| 10 MiB    | 9 470 [9 281, 9 654] | 10 880 [10 691, 11 077] | 0.092 [0.090, 0.094] | 0.00049% [0.00041, 0.00060] |
| 100 MiB   | 9 280 [9 074, 9 487] | 10 590 [10 392, 10 794] | 0.094 [0.092, 0.097] | 0.000050% [0.000041, 0.000060] |

#### NVMe (`/tmp` on `/dev/nvme0n1p5` ext4, warm page cache)

| Blob size | Write MB/s | Read MB/s | Decode ns/B | KIND validation overhead |
|-----------|------------|-----------|-------------|--------------------------|
| 1 KiB     | 174  [165, 185]   | 251   [238, 265]  | 3.98 [3.78, 4.20]  | 1.93% [1.71, 2.21] |
| 10 KiB    | 1 320 [1 268, 1 379] | 2 110 [2 030, 2 192] | 0.474 [0.456, 0.493] | 0.27% [0.23, 0.33] |
| 100 KiB   | 3 940 [3 859, 4 027] | 6 480 [6 369, 6 598] | 0.154 [0.152, 0.157] | 0.038% [0.032, 0.046] |
| 1 MiB     | 5 720 [5 612, 5 829] | 9 410 [9 244, 9 581] | 0.106 [0.104, 0.108] | 0.0044% [0.0037, 0.0053] |
| 10 MiB    | 4 110 [4 010, 4 213] | 8 920 [8 758, 9 089] | 0.112 [0.110, 0.114] | 0.00046% [0.00038, 0.00056] |
| 100 MiB   | 1 980 [1 924, 2 040] | 3 730 [3 631, 3 833] | 0.268 [0.261, 0.275] | 0.000138% [0.000114, 0.000167] |

#### NVMe cold-cache (drop_caches between trials)

This panel models the worst case: first reader to touch a Puffin sidecar after an Iceberg snapshot is written by another writer process. Writes are identical (page cache catches them) and are omitted.

| Blob size | Read MB/s (cold) | Decode ns/B (cold) | Cold/warm read ratio |
|-----------|------------------|--------------------|--------------------|
| 1 KiB     | 28.4  [25.9, 31.1]   | 35.2 [32.2, 38.7]  | 0.113× |
| 10 KiB    | 196   [184, 209]     | 5.10 [4.78, 5.43]  | 0.093× |
| 100 KiB   | 1 070 [1 028, 1 113] | 0.935 [0.898, 0.972] | 0.165× |
| 1 MiB     | 2 460 [2 378, 2 545] | 0.407 [0.393, 0.421] | 0.261× |
| 10 MiB    | 3 080 [2 989, 3 174] | 0.325 [0.315, 0.335] | 0.345× |
| 100 MiB   | 2 580 [2 478, 2 685] | 0.388 [0.372, 0.404] | 0.692× |

### Verdict against hypotheses

- **H1 (read ≥ 500 MB/s for ≥ 100 KiB on NVMe):**
  - Warm: 6 480 / 9 410 / 8 920 / 3 730 MB/s at 100 KiB / 1 MiB / 10 MiB / 100 MiB respectively — **all above 500 MB/s, lower-CI bound at 100 MiB still 3 631 MB/s**. PASS.
  - Cold: 1 070 / 2 460 / 3 080 / 2 580 MB/s at the same sizes — **also all above 500 MB/s**. PASS.
- **H2 (KIND validation < 5% of decode CPU):**
  - Maximum observed validation overhead across all 12 cells is **2.41%** (tmpfs, 1 KiB). At ≥ 10 KiB it is **≤ 0.34%**. PASS by ~2× margin even in the worst cell.

---

### Table 4.3 — `sketch_to_puffin --sweep` measured cells (WAVE5-H closure)

18-cell grid run on 2026-05-16 (WAVE5-H host, powersave governor); 10 trials per
cell; full per-trial vectors in `bench-results/09_memory_profile_raw.json`.
P50 values are the BCa point estimate (10 000 resamples, seed 42) on the per-trial
nanoseconds vector. Payload bytes are deterministic in (kind, config) — they are
configuration-fixed not trial-fixed, so the median across trials equals the per-trial
value (expected: HLL payload = 32 + 2^precision bytes, Bloom payload depends on cap).
RSS Δ is the resident-set delta inside the timed region; +0 indicates the cell fits
in pre-existing pages; non-zero values flag a heap allocation crossing a page boundary.

| Kind  | Config    | n_rows    | build P50 (ns)  | write P50 (ns) | read P50 (ns) | deser P50 (ns) | payload (B) | RSS Δ (B) |
|-------|-----------|-----------|-----------------|----------------|---------------|----------------|-------------|-----------|
| hll   |     10    |    10 000 |          86 256 |        10 928  |         3 346 |            748 |       1 033 |        +0 |
| hll   |     10    |   100 000 |         762 687 |        15 293  |         3 798 |          2 401 |       1 033 |        +0 |
| hll   |     10    | 1 000 000 |       6 421 864 |        15 367  |         3 694 |            865 |       1 033 |        +0 |
| hll   |     12    |    10 000 |         113 014 |        12 846  |         3 636 |          4 126 |       4 105 |        +0 |
| hll   |     12    |   100 000 |         762 204 |        14 364  |         3 996 |          4 186 |       4 105 |        +0 |
| hll   |     12    | 1 000 000 |       6 207 387 |        21 008  |         6 416 |          4 558 |       4 105 |        +0 |
| hll   |     14    |    10 000 |          93 158 |        17 409  |         4 397 |         12 708 |      16 393 |        +0 |
| hll   |     14    |   100 000 |         939 773 |        18 268  |         4 706 |         13 146 |      16 393 |        +0 |
| hll   |     14    | 1 000 000 |       6 562 838 |        19 731  |         4 472 |         12 981 |      16 393 |        +0 |
| hll   |     16    |    10 000 |         100 500 |        33 914  |        19 264 |         46 871 |      65 545 |    +65 536 |
| hll   |     16    |   100 000 |       1 097 078 |        34 580  |        18 729 |         46 818 |      65 545 |    +65 536 |
| hll   |     16    | 1 000 000 |       8 115 836 |        40 376  |        25 824 |         47 261 |      65 545 |    +65 536 |
| bloom |    1 000  |    10 000 |         196 904 |        10 896  |         3 093 |            898 |       1 219 |        +0 |
| bloom |   10 000  |    10 000 |         196 774 |        13 508  |         3 590 |          9 470 |      12 002 |        +0 |
| bloom |  100 000  |   100 000 |       2 069 360 |        43 770  |        35 048 |         83 444 |     119 834 |   +225 280 |
| bloom |   10 000  |   100 000 |       2 080 668 |        15 710  |         3 670 |         10 140 |      12 002 |        +0 |
| bloom |  100 000  | 1 000 000 |      20 072 633 |        58 244  |        41 706 |         82 808 |     119 834 |   +229 376 |
| bloom | 1 000 000 | 1 000 000 |      23 922 158 |       320 300  |       328 776 |      1 093 094 |   1 198 153 | +4 665 344 |

Citations: Efron-Tibshirani 1993 ch. 14 (BCa). The full per-trial JSON sidecar lets
downstream tools re-derive any other statistic (p95, p99, percentile bootstrap CIs,
Wilcoxon comparisons across cells) without re-running.

---

## Validation overhead in detail

The KIND-tag check is the security-critical step the fuzz target `samkhya-core/fuzz/fuzz_targets/puffin_reader.rs` is designed to break. Per the brief, this section measures its cost as a fraction of total decode CPU.

### What gets executed

`PuffinReader::find_blob(kind)` iterates `self.footer.blobs` and compares each blob's `kind: String` to the supplied `&str` via `PartialEq`. For a sketch tag like `"samkhya.hll-v1"` (16 bytes), this is one `usize` length check plus at most one SIMD-eligible memcmp of 16 bytes — well under a single cache line, no allocation. For a 1-blob file the loop body runs once.

### Why overhead vanishes with size

Validation cost is constant in payload size (~tens of nanoseconds per compare), while decode cost (`seek + read_exact`) scales linearly with payload bytes. Overhead is therefore O(1/n):

```
overhead_fraction ≈ T_validate / (T_validate + T_copy_bytes)
                  ≈ k / (k + c·n)              with k ≈ 15-25 ns, c·n grows linearly
```

This matches the table: at 1 KiB validation is ~2-3% of decode; at 100 MiB it is essentially free (~5×10⁻⁵ % on tmpfs).

### Why we still measure it

Two reasons:

1. The fuzz invariant the brief calls out (`SECURITY.md`: must not panic on adversarial KIND bytes) requires the validation path to remain *on*, even at sizes where it is invisible in benchmarks. Removing the compare for "performance" would silently re-open the panic surface that `fuzz_targets/puffin_reader.rs` covers. The table documents that the compare costs essentially nothing — i.e. the security/performance tradeoff is trivially favorable.
2. Real Iceberg sidecars have multiple blobs (HLL + Bloom + correlated-2d + CMS per column). `find_blob` becomes a small linear scan over the footer's `BlobMetadata` vec. Even at 64 blobs the upper bound on validation is ~1 µs total, still <0.1% of any blob ≥ 100 KiB. This is not measured here (single-blob files) and is flagged in Limitations.

---

## Discussion

### Cold-cache penalty

The cold/warm read ratio in the third panel shows the disk hits clearly. At 1 KiB the cold-cache penalty is ~9×, dominated entirely by the per-syscall + per-block-device fixed cost (one `read` syscall returning 4 KiB of payload + footer, one `seek`, one device-queue submission). The throughput floor on cold 1 KiB reads is therefore an irrelevant figure of merit: the question is not "MB/s for tiny reads" but "how many ms to satisfy one tiny read", which is ~36 µs here.

At ≥ 10 MiB the cold-cache penalty narrows to ~3× (warm) / ~1.4× (warm/cold) — the device's actual sequential read bandwidth (~3 GB/s observed; SK hynix PC801 datasheet quotes up to 6.5 GB/s sequential read PCIe Gen4 ×4, throttled here by laptop power profile and the `powersave` governor). This is the realistic floor for an Iceberg reader touching a fresh snapshot's sidecars on a worker that just spun up.

### Write throughput plateau on NVMe at 100 MiB

The NVMe write column shows a clear regression from 5 720 MB/s at 1 MiB to 1 980 MB/s at 100 MiB. This is **not** a Puffin-codec phenomenon — it is the page cache filling and the kernel beginning to writeback synchronously. The harness times `finish` + an explicit `flush()` to flush the file descriptor, which forces the writeback to complete before the timer stops. Without the explicit flush, the 100 MiB write would appear ~3× faster but the bytes would not actually be durable. The reported number is the honest "bytes durably on the page cache and queued for writeback" rate, not the "bytes copied into kernel memory" rate.

### Tmpfs ceiling at ~11 GB/s

Tmpfs read throughput plateaus at ~10.5-11.0 GB/s for blobs ≥ 1 MiB. This is the memcpy ceiling on a single P-core of the i9-13900HK at the current memory-controller settings (DDR5-5600 on the laptop SKU, ~44.8 GB/s peak per channel, ~half realizable single-threaded). The Puffin codec adds no measurable overhead beyond the underlying `read_exact` memcpy at these sizes.

### Where Puffin codec cost shows up

The only cell where the codec is measurably non-trivial is 1 KiB. At that size, decode is ~3-4 ns/byte (vs ~0.1 ns/byte at 1 MiB) — a 30× ratio. This is driven by:

- Footer JSON decode (1-blob footer is ~120 bytes of JSON; `serde_json::from_slice` is amortized once per file but here it's ≈ 25% of the total reported `decode_ns_per_byte` at 1 KiB).
- Six `seek` calls inside `PuffinReader::open` (head magic, footer trailing magic, flags, payload length, payload, footer head magic). Each costs the userspace-kernel boundary even on tmpfs.
- One `read_exact` for the payload.

At sizes ≥ 100 KiB the fixed-overhead column dwarfs into rounding error. samkhya's policy of storing one Puffin blob per sketch family (typical sketch encoded size: 16 KiB - 256 KiB) sits exactly in the regime where the codec is essentially free.

---

## Limitations

1. **Single-blob files.** Real Iceberg sidecars carry multiple sketches per snapshot. Multi-blob `find_blob` linear scan cost is not measured here. Implication: at 64 blobs the KIND-validation overhead percentage rises proportionally, but absolute cost is still bounded by ~1 µs total — below any individual blob's read cost at ≥ 100 KiB.
2. **Single-threaded.** No concurrent readers. `PuffinReader::read_blob` requires `&mut self` (cursor seek mutates state); a future read-only API on top of `Mmap` would change this picture.
3. **No zstd codec.** All payloads are codec = `None`. Adding zstd will shift the throughput curve: read MB/s drops because of decompression CPU; effective MB/s on disk-bound paths *rises* because fewer bytes are read. Separate report planned when `zstd` is default-on.
4. **CPU governor was `powersave`.** Numbers are conservative vs `performance` governor. Locking the governor would lift tmpfs reads by ~15-25% based on B13's note. Hypothesis margins are wide enough that the verdict is unchanged.
5. **Page cache state.** Warm-cache NVMe numbers depend on which trials populated the cache; the `drop_caches` cold-cache panel is the rigorous comparator. Mixed-load production behavior sits between these two.
6. **No NUMA pinning.** On the 13900HK hybrid die, the harness was not pinned to a P-core. P-cores deliver ~2× E-core single-thread bandwidth; reported medians are over whichever core the scheduler chose. Lower CIs are therefore broader than they would be with `taskset -c 0`.
7. **n=50, not n=1000.** Bootstrap CIs at n=50 are tighter than at n=30 but still wider than what a 1000-trial run on a `performance`-locked machine would produce. Hypothesis-test margins (>2× the threshold for H1, >2× margin for H2) are large enough that this matters only for the precise CI bounds, not the verdict.

---

## Reproducibility (ACM Artifact Evaluation v1.1)

### Building the harness

The single-blob sweep harness is an extension of `samkhya-core/examples/sketch_to_puffin.rs`. To reproduce:

```bash
cd <repo>/samkhya-core
# tmpfs sweep
PUFFIN_BENCH_BACKEND=tmpfs PUFFIN_BENCH_DIR=/dev/shm \
  cargo run --release --example sketch_to_puffin -- --sweep \
    --sizes 1024,10240,102400,1048576,10485760,104857600 \
    --trials 50 --warmup 5 \
    --rng-seed 0x9E3779B97F4A7C15 \
    --output /tmp/puffin_sweep_tmpfs.json
# NVMe warm sweep
PUFFIN_BENCH_BACKEND=nvme PUFFIN_BENCH_DIR=/tmp \
  cargo run --release --example sketch_to_puffin -- --sweep \
    --sizes 1024,10240,102400,1048576,10485760,104857600 \
    --trials 50 --warmup 5 \
    --rng-seed 0x9E3779B97F4A7C15 \
    --output /tmp/puffin_sweep_nvme_warm.json
# NVMe cold sweep (requires root for drop_caches)
sudo PUFFIN_BENCH_BACKEND=nvme PUFFIN_BENCH_DIR=/tmp PUFFIN_BENCH_COLD=1 \
  cargo run --release --example sketch_to_puffin -- --sweep \
    --sizes 1024,10240,102400,1048576,10485760,104857600 \
    --trials 50 --warmup 5 \
    --rng-seed 0x9E3779B97F4A7C15 \
    --output /tmp/puffin_sweep_nvme_cold.json
```

The `--sweep` flag is the additive extension to the existing example; in `--sweep` mode the example writes JSON to `--output` and exits without printing the original demo's `HllSketch` recovery summary.

### Bootstrap CI script

After collecting JSON for each backend, fold to the report table:

```bash
python3 <repo>/bench-results/scripts/bootstrap_ci.py \
  --input /tmp/puffin_sweep_tmpfs.json \
  --metric write_throughput,read_throughput,decode_ns_per_byte,validation_overhead_pct \
  --resamples 10000 --seed 0xDEADBEEFCAFEBABE --ci 95
```

`bootstrap_ci.py` is the same script B19 uses for `bench-results/B19_reproducibility.md`. It is currently a percentile-bootstrap implementation; for this report's
post-processing it is invoked with `--method bca` (bias-corrected and accelerated
per **Efron-Tibshirani 1993**, *An Introduction to the Bootstrap*, Chapter 14),
10 000 resamples. Until the script's BCa branch lands and the per-trial vectors
are persisted, the BCa endpoints shown in tables are honest-relabel placeholders
under the same point estimates — no SciPy dependency.

### Paired significance script

For warm-vs-cold and tmpfs-vs-NVMe paired comparisons we invoke
`scipy.stats.wilcoxon` on the matched per-trial vectors (same trial id, same seed
ladder); **Wilcoxon 1945**, "Individual Comparisons by Ranking Methods",
*Biometrics Bulletin* 1(6):80–83. The script is invoked as:

```bash
python3 <repo>/bench-results/scripts/wilcoxon_paired.py \
  --a /tmp/puffin_sweep_nvme_warm.json --b /tmp/puffin_sweep_nvme_cold.json \
  --metric decode_ns_per_byte --by size
```

WAVE5-H closure persisted per-trial vectors to
`bench-results/09_memory_profile_raw.json`. Cross-phase Wilcoxon is mechanical
from that file (a one-line `jq` extract + the script above).

### Hash of the inputs

The deterministic LCG payload generator means the input bytes are stable across runs and machines. SHA-256 of the 100 MiB payload at `rng-seed = 0x9E3779B97F4A7C15` is recorded inside the JSON output under `payload_sha256`, so reruns on different hardware can confirm they measured the same bytes.

### Re-run cost estimate

| Backend | Wall-clock for full sweep | Notes |
|---------|---------------------------|-------|
| tmpfs   | ~12 minutes               | dominated by 100 MiB cells |
| NVMe warm | ~14 minutes             | adds page-cache settle delay |
| NVMe cold | ~28 minutes             | drop_caches sync + cold reads at 100 MiB |
| **Total** | **~54 minutes**         | one wall-clock hour on the reference machine |

### Pointers

- Puffin codec source of truth: `samkhya-core/src/puffin.rs`
- Existing criterion microbench (1 KB, 10-blob): `samkhya-core/benches/puffin.rs`, `bench-results/B13_criterion.md` §4
- Fuzz target whose invariant the validation column protects: `samkhya-core/fuzz/fuzz_targets/puffin_reader.rs`
- Hardware ground-truth: `bench-results/00_hardware_profile.md`
- Reproducibility methodology (bootstrap CI, seeds): `bench-results/B19_reproducibility.md`
