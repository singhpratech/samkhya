# Contributing to samkhya

Thanks for your interest in samkhya. The project is pre-alpha and the
public surface is still in flux, so the most useful contributions right
now are:

- Bug reports against `samkhya-core` sketches (HLL, Bloom) and the
  `ColumnStats` shape.
- Benchmark queries for `samkhya-bench` — additional JOB-Slow / TPC-H /
  STATS-CEB workloads, ideally with reference cardinalities.
- Integration sketches against new engines (DataFusion, DuckDB, Polars,
  gpudb, Postgres). Open an issue first so we can align on the seam.
- Documentation fixes and clarifications.

If you are unsure whether a change is in scope, open an issue before
writing code. Larger redesigns (Puffin layout, feedback recorder schema,
LpBound envelope semantics) should be discussed in an issue or short
RFC-style PR description.

---

## Development setup

You will need a recent Rust toolchain. The pinned version is declared in
`rust-toolchain.toml` at the workspace root, so `rustup` will install
the right channel automatically the first time you build:

```sh
# One-time: install rustup if you do not have it.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Clone the repo.
git clone https://github.com/singhpratech/samkhya
cd samkhya

# rustup reads rust-toolchain.toml and installs Rust 1.94 with rustfmt
# and clippy automatically. This first build will take a few minutes.
cargo build --workspace --exclude samkhya-duckdb --exclude samkhya-py
```

`samkhya-duckdb` and `samkhya-py` have system dependencies (DuckDB C++
headers and Python dev headers respectively) and are excluded from the
default workspace build until you opt in explicitly. See below.

### Optional: building `samkhya-duckdb`

You need a C++17 toolchain and DuckDB headers on the include path. On
Debian/Ubuntu:

```sh
sudo apt-get install build-essential
cargo build -p samkhya-duckdb
```

### Optional: building `samkhya-py`

You need Python 3.9+ and `maturin`:

```sh
pip install maturin
maturin develop -m samkhya-py/Cargo.toml
```

`maturin develop` builds the extension module and installs it into the
active virtualenv as `samkhya`.

---

## Running tests

The full test suite for the parts of the workspace that build without
extra system dependencies:

```sh
cargo test --workspace --exclude samkhya-duckdb --exclude samkhya-py
```

If you have the DuckDB or PyO3 prerequisites set up, you can include
those crates by dropping the corresponding `--exclude` flag.

For a single crate:

```sh
cargo test -p samkhya-core
```

For a single test:

```sh
cargo test -p samkhya-core --test <test_name> -- <pattern>
```

The sketches under `samkhya-core/src/sketches/` ship with property and
statistical tests (relative-error bounds, false-positive rates) that
must keep passing. If you change a sketch's math, update both the
assertions and the doc comment that states the error bound.

---

## Formatting

We use `rustfmt` with the configuration in `rustfmt.toml` (max width
100, edition 2024). The CI runs `cargo fmt --all -- --check` and a
mismatch fails the build, so always run:

```sh
cargo fmt --all
```

before committing. Some of the import-related options in
`rustfmt.toml` (`group_imports`, `imports_granularity`,
`reorder_imports`) are nightly-only and ignored by stable rustfmt; they
are documented for contributors who run a nightly toolchain locally.

---

## Linting

We use `cargo clippy` with `-D warnings`. The CI runs:

```sh
cargo clippy --workspace --exclude samkhya-duckdb --exclude samkhya-py -- -D warnings
```

Run the same command locally before opening a PR. Clippy's complexity
thresholds are relaxed slightly in `clippy.toml` to fit analytical code
paths (sketch math, optimizer rules). If clippy flags real complexity,
fix the code rather than raising the threshold further.

If clippy flags something that is genuinely a false positive, prefer
`#[allow(clippy::<lint>)]` on the narrowest scope (function or block)
with a short comment explaining why. Avoid crate-wide allows.

---

## Submitting pull requests

### Branch naming

Use descriptive, kebab-case branch names. A short prefix helps reviewers
scan the branch list:

- `fix/<short-description>` — bug fixes
- `feat/<short-description>` — new functionality
- `docs/<short-description>` — documentation only
- `perf/<short-description>` — performance work, including benchmark
  additions
- `refactor/<short-description>` — internal restructuring

Examples: `fix/hll-precision-bounds`, `feat/kll-sketch`,
`docs/contributing-tweaks`.

### Commit messages

We do not enforce Conventional Commits, but commits should:

- Have a short imperative subject line (≤ 72 chars).
- Explain *why* in the body, not just *what* — the diff already shows
  what changed.
- Reference issues with `Fixes #N` or `Refs #N` where applicable.

Squash trivially related commits before opening the PR. Keep meaningful
history when separate commits aid review (for example: refactor first,
new feature second, tests third).

### What to test

Before opening a PR, run the three CI gates locally:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --exclude samkhya-duckdb --exclude samkhya-py -- -D warnings
cargo test --workspace --exclude samkhya-duckdb --exclude samkhya-py
```

The PR template includes a checklist for these. If your change touches
a sketch or stats type, add or extend the relevant unit / property
tests. If it touches a benchmark corpus, run `samkhya-bench` locally
and paste the output (or the relevant subset) into the PR description.

### Review

PRs need at least one approving review before merge. Be patient — the
maintainer is a single author and reviews land in batches.

---

## Architecture

For a high-level tour of the workspace and the design decisions that
shaped it, see `samkhya.md` §3. A dedicated `ARCHITECTURE.md` will land
once the major subsystems (Puffin I/O, feedback recorder, LpBound
envelope) have first implementations — when it exists, it will be
linked here.

The short version:

- `samkhya-core` — pure-Rust primitives: sketches, stats schema, error
  type, eventually the LpBound envelope and feedback store.
- `samkhya-datafusion` — DataFusion `OptimizerRule` adapter; the first
  engine seam.
- `samkhya-duckdb` — DuckDB extension via the `cxx` bridge.
- `samkhya-py` — PyO3 bindings around the core surface.
- `samkhya-bench` — benchmark harness (JOB-Slow, TPC-H, STATS-CEB).

---

## Code of Conduct

We follow the Rust Code of Conduct:
<https://www.rust-lang.org/policies/code-of-conduct>

Be kind, assume good faith, and prefer technical critique over personal
critique. Maintainers will enforce the CoC at their discretion.

---

## Reporting security issues

Please do **not** open a public GitHub issue for security-sensitive
reports (sketch payloads that trigger panics, deserialization issues,
unsafe-code soundness, etc.).

Email the maintainer directly:

**aiexplore369@gmail.com**

Include a description of the issue, reproduction steps if possible, and
the affected crate(s) and versions. We will acknowledge receipt within
seven days and work with you on a coordinated disclosure timeline.

---

## License

By contributing, you agree that your contributions will be licensed
under the Apache License, Version 2.0, the same license that covers the
rest of the project. See `LICENSE-APACHE` for the full text.
