//! Honest, runnable demonstration of samkhya's two defensible contributions.
//!
//! Run it with:
//!
//! ```text
//! cargo run -p samkhya-core --example honest_demo
//! # ...or, to also exercise the real LP fractional-edge-cover ceiling:
//! cargo run -p samkhya-core --example honest_demo --features lp_solver
//! ```
//!
//! Everything printed below is a number this program actually computes — no
//! figures are hand-typed into the output. The two contributions shown are:
//!
//!   PART 1  A pessimistic upper-bound *envelope* (`samkhya_core::lpbound`)
//!           that is a provable ceiling on a join's output cardinality. A
//!           correction layer clamped to this ceiling can never push an
//!           estimate above it — the "never-regress" guarantee.
//!
//!   PART 2  Cross-engine portability via Iceberg Puffin sidecars
//!           (`samkhya_core::puffin`): a sketch written by one engine reads
//!           back byte-identically (and estimate-identically) in another.

use std::fs::File;
use std::io::BufWriter;

use samkhya_core::lpbound::{
    AgmBound, ChainBound, ProductBound, UpperBound, clamp_estimate, saturating_clamp,
};
use samkhya_core::puffin::{Blob, PuffinReader, PuffinWriter};
use samkhya_core::sketches::{HllSketch, Sketch};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    part1_never_regress_envelope()?;
    println!();
    let (puffin_before, puffin_after, puffin_path) = part2_puffin_round_trip()?;
    println!();
    print_summary(puffin_before, puffin_after, &puffin_path);
    Ok(())
}

// =============================================================================
// PART 1 — never-regress upper-bound envelope
// =============================================================================

/// A small, *known* tree-shaped (chain / path) join scenario.
///
/// We model three relations whose actual contents we control, so the true
/// output cardinality is computed directly from the data rather than asserted.
///
/// Schema:
///   R0(a)        — rows keyed by attribute `a`
///   R1(a, b)     — rows keyed by `a` (joins R0) and `b` (joins R2)
///   R2(b)        — rows keyed by attribute `b`
///
/// Equality predicates form a path:  R0 —(a)— R1 —(b)— R2  (tree-shaped, no
/// cycle — deliberately not the size-7 clique corner where the LP ceiling can
/// overshoot AGM under uniform ℓp=1).
fn part1_never_regress_envelope() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== PART 1 — never-regress upper-bound envelope (chain / path join) ===");

    // --- Build concrete relations with known contents ------------------------
    // R0: keys a ∈ {0, 1, 2, 3}  → 4 rows, 4 distinct `a`.
    let r0_a: Vec<u64> = vec![0, 1, 2, 3];

    // R1: (a, b) pairs. Some `a` values repeat (fan-out), some don't appear.
    //   a ∈ {0, 0, 1, 2, 2, 2}  (distinct a = {0, 1, 2})
    //   b ∈ {10,11,10,12,12,13}
    let r1_ab: Vec<(u64, u64)> = vec![(0, 10), (0, 11), (1, 10), (2, 12), (2, 12), (2, 13)];

    // R2: keys b ∈ {10, 12, 13, 14} → 4 rows, 4 distinct `b`.
    let r2_b: Vec<u64> = vec![10, 12, 13, 14];

    // Row counts the bounds consume.
    let relations: Vec<u64> = vec![r0_a.len() as u64, r1_ab.len() as u64, r2_b.len() as u64];

    // Path predicates: R0.a = R1.a  (relations 0,1) and R1.b = R2.b (1,2).
    let predicates: Vec<(usize, usize)> = vec![(0, 1), (1, 2)];

    // Distinct-key counts per relation (used by ChainBound and as LP hints).
    // ChainBound divides by max(D_i, D_j) on each predicate; we feed the
    // distinct count of the join key carried by each relation.
    let distinct_counts: Vec<u64> = vec![
        distinct(r0_a.iter().copied()),          // R0 distinct a
        distinct(r1_ab.iter().map(|&(a, _)| a)), // R1 distinct a (join key to R0)
        distinct(r2_b.iter().copied()),          // R2 distinct b
    ];

    // --- Compute the TRUE output cardinality from the actual data ------------
    // Path semantics: rows of R1 whose `a` is in R0 AND whose `b` is in R2,
    // each multiplied by the matching multiplicity on either side. Because R0
    // and R2 hold each key at most once here, the true output equals the count
    // of R1 rows whose `a` appears in R0 and whose `b` appears in R2.
    let mut true_card: u64 = 0;
    for &(a, b) in &r1_ab {
        let a_matches = r0_a.iter().filter(|&&x| x == a).count() as u64;
        let b_matches = r2_b.iter().filter(|&&x| x == b).count() as u64;
        true_card += a_matches * b_matches;
    }

    println!(
        "  scenario: R0(|{}|) --a-- R1(|{}|) --b-- R2(|{}|)   distinct keys = {:?}",
        relations[0], relations[1], relations[2], distinct_counts
    );
    println!("  TRUE output cardinality (counted from data) = {true_card}");

    // --- Compute each available bound ---------------------------------------
    let product = ProductBound.ceiling(&relations, &predicates);
    let agm = AgmBound.ceiling(&relations, &predicates);
    let chain = ChainBound::new(distinct_counts.clone()).ceiling(&relations, &predicates);

    println!("  ProductBound ceiling = {product}");
    println!("  AgmBound     ceiling = {agm}");
    println!("  ChainBound   ceiling = {chain}");

    // --- Invariant 1: every bound is an inclusive upper bound ----------------
    assert!(
        product >= true_card,
        "ProductBound {product} < true {true_card}"
    );
    assert!(agm >= true_card, "AgmBound {agm} < true {true_card}");
    assert!(chain >= true_card, "ChainBound {chain} < true {true_card}");
    println!("  [ok] every bound >= true cardinality (sound inclusive ceiling)");

    // --- Invariant 2: documented partial order -------------------------------
    //   ProductBound >= { ChainBound, AgmBound }
    assert!(
        product >= chain,
        "ProductBound {product} < ChainBound {chain}"
    );
    assert!(product >= agm, "ProductBound {product} < AgmBound {agm}");
    println!("  [ok] ProductBound >= {{ChainBound, AgmBound}} (documented partial order)");

    // --- Optional LP leg: LpJoinBound <= AgmBound on this tree-shaped input ---
    #[cfg(feature = "lp_solver")]
    let lp_ceiling: Option<u64> = {
        use samkhya_core::lpbound::LpJoinBound;
        let lp = LpJoinBound::new();
        let lp_bound = lp.ceiling(&relations, &predicates);
        println!("  LpJoinBound  ceiling = {lp_bound}   (real fractional-edge-cover LP)");
        assert!(
            lp_bound >= true_card,
            "LpJoinBound {lp_bound} < true {true_card}"
        );
        // On this tree-shaped (path) input the LP ceiling is tighter than or
        // equal to the coarse AGM `min * max` shortcut.
        assert!(
            lp_bound <= agm,
            "LpJoinBound {lp_bound} must be <= AgmBound {agm} on a tree-shaped join"
        );
        println!("  [ok] LpJoinBound <= AgmBound on this path join (LP refinement holds)");
        Some(lp_bound)
    };
    #[cfg(not(feature = "lp_solver"))]
    let lp_ceiling: Option<u64> = {
        println!("  LpJoinBound  ceiling = (skipped — rerun with --features lp_solver)");
        None
    };

    // --- Invariant 3: the clamp itself — the never-regress guarantee ---------
    // The tightest sound ceiling we hold is the min over all computed bounds
    // (the optimizer is documented to take the minimum, not a strict chain).
    let mut ceiling = product.min(agm).min(chain);
    if let Some(lp) = lp_ceiling {
        ceiling = ceiling.min(lp);
    }
    println!("  tightest ceiling = min(all bounds) = {ceiling}");

    // A hypothetical *over-eager* corrector that wildly over-estimates.
    let corrector_output: f64 = 1_000_000.0;
    // Production path: saturating clamp never crashes and never exceeds ceiling.
    let clamped = saturating_clamp(corrector_output, ceiling);
    assert!(
        clamped <= ceiling,
        "clamped {clamped} exceeded ceiling {ceiling}"
    );
    // Also show the plain `min` the guarantee reduces to.
    assert_eq!(clamped, (corrector_output as u64).min(ceiling));
    println!(
        "  corrector proposed {} → saturating_clamp = {} (<= ceiling, never regresses)",
        corrector_output as u64, clamped
    );

    // A corrector that *respects* the envelope passes the strict (fallible)
    // clamp; one that violates it is rejected as a corrector bug.
    let good_estimate = (true_card as f64).max(0.0); // a reasonable, in-envelope guess
    let ok = clamp_estimate(good_estimate, ceiling)?;
    assert!(ok <= ceiling);
    let violating = clamp_estimate((ceiling + 1) as f64, ceiling);
    assert!(
        violating.is_err(),
        "an estimate above the ceiling must be rejected"
    );
    println!(
        "  clamp_estimate({}) = Ok({}); clamp_estimate({}) = Err (corrector-violation guard)",
        good_estimate as u64,
        ok,
        ceiling + 1
    );

    Ok(())
}

/// Count distinct values in an iterator of `u64`.
fn distinct<I: IntoIterator<Item = u64>>(it: I) -> u64 {
    let set: std::collections::BTreeSet<u64> = it.into_iter().collect();
    set.len() as u64
}

// =============================================================================
// PART 2 — Puffin cross-engine round-trip
// =============================================================================

/// Build an HLL sketch, write it to a real Puffin sidecar on disk, read it
/// back in a fresh reader, reconstruct the sketch, and verify the cardinality
/// estimate round-trips byte-for-byte.
///
/// Returns `(estimate_before, estimate_after, sidecar_path)` for the summary.
fn part2_puffin_round_trip() -> Result<(u64, u64, String), Box<dyn std::error::Error>> {
    println!("=== PART 2 — Puffin cross-engine round-trip (HLL sidecar) ===");

    // --- Build + populate the sketch (the "writer" engine) -------------------
    let mut hll = HllSketch::try_new(12)?;
    for i in 0..1_000u32 {
        hll.add(&i.to_le_bytes());
    }
    let estimate_before = hll.estimate();
    let payload = hll.to_bytes()?;
    println!(
        "  built HllSketch(p=12), added 1000 distinct keys → estimate = {estimate_before} \
         ({} payload bytes)",
        payload.len()
    );

    // --- Write a real Puffin sidecar to the temp dir -------------------------
    let path = std::env::temp_dir().join("samkhya_honest_demo.puffin");
    let path_str = path.display().to_string();
    {
        let mut w = PuffinWriter::new(BufWriter::new(File::create(&path)?));
        w.add_blob(Blob::new(HllSketch::KIND, vec![0], &payload))?;
        // finish() returns the inner BufWriter; drop it to flush to disk.
        let _inner = w.finish()?;
    }
    println!("  wrote Puffin sidecar → {path_str}");

    // --- Read it back in a fresh reader (the "reader" engine) ----------------
    let mut reader = PuffinReader::open(File::open(&path)?)?;
    let (idx, meta) = reader
        .find_blob(HllSketch::KIND)
        .ok_or("HLL blob missing from sidecar")?;
    println!(
        "  reopened sidecar: found blob type=\"{}\" fields={:?} length={}",
        meta.kind, meta.fields, meta.length
    );
    let blob_bytes = reader.read_blob(idx)?;
    let restored = HllSketch::from_bytes(&blob_bytes)?;
    let estimate_after = restored.estimate();
    println!("  reconstructed HllSketch → estimate = {estimate_after}");

    // --- Verify the estimate round-trips exactly -----------------------------
    assert_eq!(
        estimate_before, estimate_after,
        "HLL estimate changed across the Puffin round-trip ({estimate_before} -> {estimate_after})"
    );
    assert_eq!(
        blob_bytes, payload,
        "blob bytes differ from what was written"
    );
    println!(
        "  [ok] estimate round-trips exactly across write→read ({estimate_before} == {estimate_after})"
    );

    // Clean up the temp sidecar (best-effort).
    let _ = std::fs::remove_file(&path);

    Ok((estimate_before, estimate_after, path_str))
}

// =============================================================================
// Summary
// =============================================================================

fn print_summary(puffin_before: u64, puffin_after: u64, puffin_path: &str) {
    println!("=== SUMMARY — what this run demonstrated (all numbers computed above) ===");
    println!(
        "  PART 1  Every shipped upper bound (Product/AGM/Chain{}) is a SOUND inclusive",
        if cfg!(feature = "lp_solver") {
            "/LpJoin"
        } else {
            ""
        }
    );
    println!("          ceiling on a known path join, and obeys the documented partial order.");
    println!("          Clamping an over-eager corrector to the tightest ceiling can NEVER push");
    println!("          an estimate above that ceiling — the never-regress guarantee, enforced by");
    println!(
        "          saturating_clamp (production) and clamp_estimate (corrector-violation guard)."
    );
    println!("  PART 2  An HLL sketch round-tripped through a real on-disk Iceberg Puffin sidecar");
    println!("          (written at {puffin_path}); the distinct-count estimate was identical");
    println!(
        "          before ({puffin_before}) and after ({puffin_after}) read-back — engine-agnostic portability."
    );
    if !cfg!(feature = "lp_solver") {
        println!("  NOTE    Re-run with `--features lp_solver` to also exercise the real");
        println!("          fractional-edge-cover LP ceiling (LpJoinBound).");
    }
}
