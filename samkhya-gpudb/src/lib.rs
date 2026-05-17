//! samkhya-gpudb — GPU batch-inference adapter for samkhya.
//!
//! # Role
//!
//! This crate is Layer 4 of the samkhya architecture (see
//! `ARCHITECTURE.md` §3 and `samkhya.md` §3). It pairs samkhya's
//! residual corrector with [gpudb][gpudb] — Prateek's GPU-accelerated
//! DuckDB extension targeting CUDA and Apple Silicon Metal — so that
//! thousands of subplan candidates can be scored in a single kernel
//! launch instead of being serialized through CPU inference.
//!
//! [gpudb]: https://github.com/singhpratech/gpudb
//!
//! # Why this is the differentiator
//!
//! Subplan enumeration is embarrassingly parallel: each candidate is an
//! independent forward pass through a sub-MB residual model (GBT or
//! PFN-style). CPU inference walks them one at a time; a GPU kernel
//! batches the lot. No published cardinality-estimation system —
//! including TiCard, the closest neighbor — targets batch GPU inference
//! of the *correction model itself*. That is the unbuilt synthesis the
//! samkhya architecture documents call out.
//!
//! # Status
//!
//! Scaffold. The trait surface and a CPU fallback live here today; the
//! real CUDA and Metal kernels are post-MVP and will live behind future
//! cargo feature flags. See the [`cuda`](#) and [`metal`](#) feature
//! placeholders in `Cargo.toml`.
//!
//! # Opt-in posture
//!
//! GPU is **strictly opt-in**. Default builds of samkhya, and default
//! builds of this crate, never link a CUDA toolkit or a Metal framework.
//! The trait is implementable on plain CPU (see [`CpuFallbackCorrector`])
//! so downstream code can depend on `samkhya-gpudb` and exercise the
//! interface today without any GPU dependency.
#![deny(rustdoc::broken_intra_doc_links)]

use samkhya_core::Result;
use samkhya_core::residual::CorrectionFeatures;

/// Batch-scoring corrector surface.
///
/// Implementations score a slice of [`CorrectionFeatures`] in a single
/// call. Real GPU implementations dispatch one CUDA / Metal kernel
/// launch over the entire batch; the CPU fallback walks the slice
/// sequentially. Either way the contract is the same: the returned
/// `Vec<u64>` is parallel to the input slice and uses LpBound-clamped
/// row-count estimates.
///
/// The trait deliberately mirrors the single-row
/// [`samkhya_core::residual::Corrector`] trait but in batch form. The
/// single-row trait stays the universal contract for engines that
/// estimate one plan at a time; this trait is the batch surface gpudb
/// needs for subplan enumeration.
pub trait GpuCorrector: Send + Sync {
    /// Score a batch of feature vectors. The output `Vec` is in the
    /// same order as the input slice and has the same length.
    fn batch_score(&self, features: &[CorrectionFeatures]) -> Result<Vec<u64>>;
}

/// CPU-only fallback that walks the batch sequentially.
///
/// Useful for three things:
///
/// 1. Keeping the [`GpuCorrector`] trait honest — there is a working
///    implementation in-tree that compiles on default features.
/// 2. Local development and CI without a GPU runtime installed.
/// 3. A safe default for engines that opt into the batch surface but
///    have not yet wired in a CUDA / Metal kernel.
///
/// Each row's prediction echoes its `baseline_estimate`, matching the
/// behavior of [`samkhya_core::residual::IdentityCorrector`]. Once real
/// CUDA / Metal backends land they will sit alongside this struct
/// behind the `cuda` and `metal` cargo feature flags.
#[derive(Debug, Default, Clone, Copy)]
pub struct CpuFallbackCorrector;

impl CpuFallbackCorrector {
    /// Construct a new CPU fallback. Cheap; the type is zero-sized.
    pub const fn new() -> Self {
        Self
    }
}

impl GpuCorrector for CpuFallbackCorrector {
    fn batch_score(&self, features: &[CorrectionFeatures]) -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(features.len());
        for f in features {
            out.push(f.baseline_estimate);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_fallback_echoes_baseline_estimates() {
        let corrector = CpuFallbackCorrector::new();
        let batch = vec![
            CorrectionFeatures {
                baseline_estimate: 10,
                ..Default::default()
            },
            CorrectionFeatures {
                baseline_estimate: 250,
                ..Default::default()
            },
            CorrectionFeatures {
                baseline_estimate: 9_999,
                ..Default::default()
            },
        ];

        let scored = corrector.batch_score(&batch).expect("batch_score");
        assert_eq!(scored, vec![10, 250, 9_999]);
    }

    #[test]
    fn cpu_fallback_handles_empty_batch() {
        let corrector = CpuFallbackCorrector::new();
        let scored = corrector.batch_score(&[]).expect("batch_score");
        assert!(scored.is_empty());
    }

    #[test]
    fn cpu_fallback_preserves_order_and_length() {
        let corrector = CpuFallbackCorrector::new();
        let batch: Vec<CorrectionFeatures> = (0..128)
            .map(|i| CorrectionFeatures {
                baseline_estimate: i as u64,
                ..Default::default()
            })
            .collect();

        let scored = corrector.batch_score(&batch).expect("batch_score");
        assert_eq!(scored.len(), batch.len());
        for (i, v) in scored.iter().enumerate() {
            assert_eq!(*v, i as u64);
        }
    }
}
