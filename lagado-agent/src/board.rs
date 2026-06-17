//! board.rs — The Board: a Park-scored slice-assembler over `memory_tiers`.
//!
//! Doctrine: `score = α·recency + β·relevance + γ·importance`, recomputed STATELESS
//! per step, top-k (Park / Generative Agents). This is the "what to surface for THIS
//! step now" scorer — deliberately distinct from `memory_tiers::information_value`,
//! which is the multiplicative entropy/pruning "what to forget" scorer (λ = ln2/30d).
//! Two different masters, different time constants — keep them separate.
//!
//! CRITICAL (G3_RESULTS.md, advisor 2026-06-16): the relevance signal is ColBERT
//! pooled cosine, COMPRESSED into ~[0.96, 0.99]. Used raw in the additive sum it
//! contributes a near-constant β to every particle and goes inert while recency and
//! importance (range ~[0,1]) drown it. So relevance and recency are NORMALIZED across
//! the candidate set BEFORE they enter the sum. Normalization restores *range*, not
//! *ordering* — if relevance still drags noise into the slice after tuning, that's the
//! trigger to move to late-interaction MaxSim.
//!
//! This module is the pure ③a floor: scoring math only. The slice-assembler that
//! gathers candidates + embeds + calls this lives in `memory_tiers` (③a wiring) and is
//! parity-tested against the Python G3 eval before any α/β/γ tuning.

use crate::memory_tiers::{MemoryEntry, Tier};

/// Park score weights: α recency, β relevance, γ importance.
#[derive(Debug, Clone, Copy)]
pub struct ParkWeights {
    pub alpha: f32,
    pub beta: f32,
    pub gamma: f32,
}

impl Default for ParkWeights {
    /// Set BY PRINCIPLE, not tuned. The G3 fixture has uniform recency and no
    /// importance labels, so it can only tune β-relevance quality — α and γ have no
    /// oracle. Equal weighting until an enriched fixture justifies otherwise.
    fn default() -> Self {
        Self { alpha: 1.0, beta: 1.0, gamma: 1.0 }
    }
}

/// Raw, pre-normalization per-candidate signals.
#[derive(Debug, Clone, Copy)]
pub struct ParkSignals {
    /// Raw recency factor in (0, 1], e.g. `recency_factor(age, half_life)`.
    pub recency: f32,
    /// Raw relevance (cosine), typically compressed ~[0.96, 0.99].
    pub relevance: f32,
    /// Importance, already in [0, 1] (see `importance_heuristic`).
    pub importance: f32,
}

/// Recency factor: exponential decay on age since last access, in (0, 1].
/// `half_life_secs` is the Board's freshness time constant — INTENTIONALLY faster
/// than `information_value`'s 30-day forgetting curve; this is "fresh for this step".
pub fn recency_factor(age_secs: i64, half_life_secs: f32) -> f32 {
    if half_life_secs <= 0.0 {
        return 1.0;
    }
    let lambda = std::f32::consts::LN_2 / half_life_secs;
    (-lambda * age_secs.max(0) as f32).exp()
}

/// Deterministic importance heuristic — the always-on FLOOR (③c refines it with an
/// async model rater in the sleep gate). Range [0, 1]. Transparent, honestly-weak
/// signals, since no importance oracle exists in G3 (set by principle, not tuned):
///   - tier: cold/vault (the user chose to keep it) > warm (survived consolidation) > hot
///   - reinforcement: access_count, log-saturating toward +0.20
///   - substance: text length, log-saturating toward +0.20
pub fn importance_heuristic(entry: &MemoryEntry) -> f32 {
    let tier_base = match entry.tier {
        Tier::Cold => 0.6,
        Tier::Warm => 0.4,
        Tier::Hot => 0.3,
    };
    let reinforce = 0.20 * (1.0 - (-(entry.access_count as f32) / 5.0).exp());
    let substance = 0.20 * (1.0 - (-(entry.text.len() as f32) / 200.0).exp());
    (tier_base + reinforce + substance).clamp(0.0, 1.0)
}

/// Min-max normalize to [0, 1]. When the set has no spread (all equal), every signal
/// carries no information this step → return a neutral 0.5 for all (so it neither
/// helps nor hurts the additive sum).
fn min_max_norm(xs: &[f32]) -> Vec<f32> {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &x in xs {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    let range = hi - lo;
    if range <= f32::EPSILON {
        return vec![0.5; xs.len()];
    }
    xs.iter().map(|&x| (x - lo) / range).collect()
}

/// Compute Park scores for a candidate set, returned in input order.
/// Recency and relevance are min-max normalized ACROSS the set first (so compressed
/// relevance still registers); importance is assumed already in [0, 1].
pub fn park_scores(signals: &[ParkSignals], w: &ParkWeights) -> Vec<f32> {
    if signals.is_empty() {
        return Vec::new();
    }
    let rel_n = min_max_norm(&signals.iter().map(|s| s.relevance).collect::<Vec<_>>());
    let rec_n = min_max_norm(&signals.iter().map(|s| s.recency).collect::<Vec<_>>());
    (0..signals.len())
        .map(|i| w.alpha * rec_n[i] + w.beta * rel_n[i] + w.gamma * signals[i].importance)
        .collect()
}

/// Indices of the top-k candidates by Park score (descending), stable on ties.
pub fn top_k_indices(signals: &[ParkSignals], w: &ParkWeights, k: usize) -> Vec<usize> {
    let scores = park_scores(signals, w);
    let mut idx: Vec<usize> = (0..scores.len()).collect();
    idx.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b)) // stable on ties
    });
    idx.truncate(k);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tier: Tier, text: &str, access_count: u32) -> MemoryEntry {
        MemoryEntry {
            id: "x".into(),
            text: text.into(),
            tier,
            temperature: 1.0,
            created_at: 0,
            accessed_at: 0,
            access_count,
        }
    }

    #[test]
    fn relevance_compression_registers_after_norm() {
        // Three candidates differing ONLY in relevance, in the compressed band.
        let s = vec![
            ParkSignals { recency: 0.5, relevance: 0.96,  importance: 0.5 },
            ParkSignals { recency: 0.5, relevance: 0.975, importance: 0.5 },
            ParkSignals { recency: 0.5, relevance: 0.99,  importance: 0.5 },
        ];
        let scores = park_scores(&s, &ParkWeights::default());
        // Strictly increasing — a 0.03 raw band still orders the slice.
        assert!(scores[2] > scores[1] && scores[1] > scores[0]);
        // Spread equals β·(full normalized range) = 1.0 — relevance is NOT inert.
        assert!((scores[2] - scores[0] - 1.0).abs() < 1e-4, "scores={scores:?}");
    }

    #[test]
    fn normalized_relevance_can_overcome_importance() {
        // A: top relevance but low importance.  B: weak relevance but high importance.
        // Raw, relevance differs by only 0.03 and importance (0.4 spread) would decide
        // → B wins. After normalization relevance gets full range and A overtakes.
        let s = vec![
            ParkSignals { recency: 0.0, relevance: 0.99, importance: 0.1 }, // A
            ParkSignals { recency: 0.0, relevance: 0.96, importance: 0.5 }, // B
        ];
        let scores = park_scores(&s, &ParkWeights::default());
        assert!(scores[0] > scores[1], "normalized relevance must overcome importance: {scores:?}");
    }

    #[test]
    fn importance_ranks_by_tier_then_reinforcement() {
        let cold = importance_heuristic(&entry(Tier::Cold, "same text", 0));
        let warm = importance_heuristic(&entry(Tier::Warm, "same text", 0));
        let hot  = importance_heuristic(&entry(Tier::Hot,  "same text", 0));
        assert!(cold > warm && warm > hot);
        // Reinforcement lifts importance within a tier.
        let hot_used = importance_heuristic(&entry(Tier::Hot, "same text", 50));
        assert!(hot_used > hot);
        // Always bounded.
        for imp in [cold, warm, hot, hot_used] {
            assert!((0.0..=1.0).contains(&imp));
        }
    }

    #[test]
    fn recency_factor_decays() {
        assert!((recency_factor(0, 100.0) - 1.0).abs() < 1e-6);
        assert!((recency_factor(100, 100.0) - 0.5).abs() < 1e-4); // one half-life
        assert!(recency_factor(1000, 100.0) < recency_factor(100, 100.0));
    }

    #[test]
    fn min_max_neutral_when_no_spread() {
        assert_eq!(min_max_norm(&[0.97, 0.97, 0.97]), vec![0.5, 0.5, 0.5]);
        assert_eq!(min_max_norm(&[0.0, 1.0]), vec![0.0, 1.0]);
    }

    #[test]
    fn top_k_and_empty() {
        assert!(park_scores(&[], &ParkWeights::default()).is_empty());
        let s = vec![
            ParkSignals { recency: 0.1, relevance: 0.96, importance: 0.1 },
            ParkSignals { recency: 0.9, relevance: 0.99, importance: 0.9 },
        ];
        assert_eq!(top_k_indices(&s, &ParkWeights::default(), 1), vec![1]);
    }
}
