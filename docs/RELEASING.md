# Releasing samkhya

Releases are built from a reviewed, clean commit. CI never publishes packages,
pushes tags, or changes registry state; those remain explicit release-owner
actions.

## 1. Prepare the Candidate

1. Set the final version in the workspace, Python project, private Node package,
   and both Cargo lockfiles.
2. Keep `CHANGELOG.md` under `[Unreleased]` while validating. Update
   `CITATION.cff`, add the release date, and create the changelog comparison
   link only when the release is actually cut.
3. Run `python3 scripts/check_version_sync.py` and `git diff --check`.

## 2. Run Source Gates

```bash
cargo +1.94 fmt --all -- --check
cargo +1.94 clippy --locked --workspace --exclude samkhya-py -- -D warnings
cargo +1.94 test --locked --workspace --exclude samkhya-py
cargo +1.94 test --locked -p samkhya-it \
  --features puffin-cross-engine --test puffin_cross_engine
cargo +1.85 check --locked --workspace
```

Also run the optional-feature matrix in `.github/workflows/ci.yml`, Python wheel
tests on 3.9/3.12/3.13, the TypeScript wire tests, `cargo deny`, the documented
fuzz budget, and `cargo semver-checks` against the previous stable tag.

## 3. Build and Inspect Artifacts

Run the manual `release-candidate.yml` workflow at the candidate commit. It
creates Rust package archives, a Python wheel, checksums, and an SBOM without
publishing them. Candidate-only archive construction patches the crates.io
source to the tested local `samkhya-core`; after core is published, remove that
substitution from the real `cargo publish --dry-run`. Install the wheel into an
empty environment and verify both
`importlib.metadata.version("samkhya")` and `samkhya.__version__`.

Publish Rust crates in dependency order: `samkhya-core` first, then adapters
that depend on it. Use `cargo publish --dry-run` immediately before each real
publish. The Node transport remains private and is not published to npm.

## 4. Finalize

After registry smoke tests pass, move the changelog section to a dated release,
update `CITATION.cff`, commit those changes, and create the signed `vX.Y.Z` tag.
Never reuse or move a published tag. If any artifact differs from the tested
commit, discard it and restart from step 1.
