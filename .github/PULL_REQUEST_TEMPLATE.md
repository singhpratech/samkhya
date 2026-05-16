# Summary

<!-- One or two sentences: what does this PR change? -->

# Motivation

<!--
Why is this change needed? Link issues with `Fixes #N` or `Refs #N`.
For larger changes, include a sentence on alternatives considered.
-->

# Test plan

<!--
How did you verify the change? Paste commands and (where relevant) the
tail of their output. Mention any benchmark numbers if you ran
samkhya-bench.
-->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --exclude samkhya-duckdb --exclude samkhya-py -- -D warnings`
- [ ] `cargo test --workspace --exclude samkhya-duckdb --exclude samkhya-py`

# Checklist

- [ ] Formatting passes (`cargo fmt --all -- --check`)
- [ ] Clippy passes with `-D warnings`
- [ ] Tests pass on `ubuntu-latest`-equivalent locally
- [ ] Public API changes are documented (rustdoc on the changed item)
- [ ] No new "learned" / "adaptive" / "AI" framing in user-facing text
  (see `samkhya.md` §3 for the naming rules)
- [ ] No new external dependencies without a one-line justification in
  the PR description
