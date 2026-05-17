# Summary

<!-- One or two sentences: what does this PR change? -->

# Motivation

<!--
Why is this change needed? What user-visible problem or internal
shortcoming does it address? For larger changes, include a sentence on
alternatives considered.
-->

# Changes

<!-- Bullet list of the concrete changes in this PR. -->

-
-
-

# Test plan

<!--
How did you verify the change? Paste commands and (where relevant) the
tail of their output. Mention any benchmark numbers if you ran
samkhya-bench.
-->

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --exclude samkhya-duckdb --exclude samkhya-py --exclude samkhya-postgres --all-features -- -D warnings`
- [ ] `cargo test --workspace --exclude samkhya-duckdb --exclude samkhya-py --no-fail-fast`
- [ ] Added or updated tests for changed behavior
- [ ] Public API changes are documented (rustdoc on the changed item)
- [ ] **Industry-standard metrics applied** if this PR touches measurement code: BCa bootstrap CIs (Efron-Tibshirani 1993), Wilcoxon paired signed-rank (1945), BH-FDR correction (1995), q-error per Moerkotte VLDB 2009 — see `bench-results/METHODOLOGY.md`

# Naming-compliance checklist

- [ ] No "learned" / "adaptive" / "AI" framing in primary user-facing
  text (see [[feedback-samkhya-naming]] rule: prefer "portable" /
  "feedback-driven" / "self-correcting" / "LLM-pluggable")
- [ ] If this PR touches the `Corrector` trait surface, the
  pluggable-corrector-backend framing (GBT default · TabPFN-2.5 ·
  LLM TODO) is preserved (no single-backend framing in trait docs)

# Breaking changes

<!--
samkhya is post-1.0. Public API and on-disk format changes are
governed by `docs/SEMVER.md`. State the semver impact explicitly.
-->

- Breaking change? <!-- yes / no -->
- Semver impact: <!-- patch (1.x.y) / minor (1.x.0) / major (2.0.0) -->
- If yes, link the deprecation note and migration guidance:

# Related issues

<!--
Reference issues with `Fixes #N` or `Refs #N`. List any companion PRs
in sibling crates.
-->

---

*By submitting this PR you agree to dual Apache-2.0 OR MIT licensing
of your contribution.*
