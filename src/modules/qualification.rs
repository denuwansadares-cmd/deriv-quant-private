const MODEL_NAMES: [&str; 8] = [
    "fluid_dynamics",
    "chaos_theory",
    "mean_reversion",
    "trend_analysis",
    "multi_timeframe",
    "nash_equilibrium",
    "tick_velocity",
    "volume_surge",
];

const QUALIFICATION_MIN_SAMPLES: u64 = 500;
const QUALIFICATION_MAX_DRAWDOWN_FRACTION: f64 = 0.20;
const QUALIFICATION_MAX_RISK_OF_RUIN: f64 = 0.05;
const QUALIFICATION_MIN_PARAMETER_VARIANTS: u64 = 5;
const QUALIFICATION_MIN_STABLE_NEIGHBOURS: f64 = 0.80;
const QUALIFICATION_MIN_MARKETS: u64 = 3;
const QUALIFICATION_MIN_REGIMES: u64 = 3;
const QUALIFICATION_MAX_MARKET_DEPENDENCY: f64 = 0.50;
const QUALIFICATION_MAX_BRIER_SCORE: f64 = 0.25;

#[derive(Debug, Clone, Deserialize)]
struct ModelEvidence {
    name: String,
    evidence_version: u64,
    code_version: String,
    holdout_dataset_id: String,
    evidence_checksum: String,
    independently_evaluated: bool,
    enabled_requested: bool,
    independent_sample_size: u64,
    holdout_expectancy_minor: f64,
    holdout_expectancy_ci95_lower_minor: f64,
    break_even_expectancy_minor: f64,
    maximum_drawdown_fraction: f64,
    risk_of_ruin: f64,
    parameter_variants_tested: u64,
    positive_parameter_neighbour_fraction: f64,
    simple_baseline_lift_ci95_lower_minor: f64,
    random_baseline_lift_ci95_lower_minor: f64,
    independent_market_count: u64,
    independent_regime_count: u64,
    maximum_single_market_profit_fraction: f64,
    calibration_test_passed: bool,
    brier_score: f64,
    failure_stress_test_passed: bool,
}

#[derive(Debug, Deserialize)]
struct QualificationRegistry {
    models: Vec<ModelEvidence>,
}

#[derive(Debug, Clone, PartialEq)]
enum ModelSignal {
    Qualified { name: &'static str, score: f64 },
    NotQualified { name: &'static str, reason: String },
}

#[derive(Debug, Clone)]
struct QualifiedConsensus {
    score: f64,
    confidence: f64,
    signals: Vec<ModelSignal>,
}

impl QualifiedConsensus {
    fn qualified_scores(&self) -> impl Iterator<Item = f64> + '_ {
        self.signals.iter().filter_map(|signal| match signal {
            ModelSignal::Qualified { score, .. } => Some(*score),
            ModelSignal::NotQualified { .. } => None,
        })
    }
    fn bullish_count(&self) -> usize { self.qualified_scores().filter(|score| *score > 0.55).count() }
    fn bearish_count(&self) -> usize { self.qualified_scores().filter(|score| *score < 0.45).count() }
    fn directional_agreement(&self, bullish: bool) -> usize {
        self.qualified_scores().filter(|score| if bullish { *score > 0.55 } else { *score < 0.45 }).count()
    }
}

#[derive(Debug, Clone)]
struct QualificationGate {
    qualified: [bool; 8],
    reasons: [String; 8],
}

impl QualificationGate {
    fn load() -> Result<Self> {
        let path = env::var("STRATEGY_QUALIFICATIONS_PATH")
            .unwrap_or_else(|_| "config/strategy-qualifications.json".to_string());
        let registry: QualificationRegistry = serde_json::from_str(
            &fs::read_to_string(&path)
                .with_context(|| format!("Failed to read strategy qualification registry: {path}"))?,
        )
        .with_context(|| format!("Invalid strategy qualification registry: {path}"))?;
        let mut gate = Self {
            qualified: [false; 8],
            reasons: std::array::from_fn(|_| "no evidence record".to_string()),
        };
        for (index, name) in MODEL_NAMES.iter().enumerate() {
            if let Some(evidence) = registry.models.iter().find(|model| model.name == *name) {
                let failures = qualification_failures(evidence);
                gate.qualified[index] = failures.is_empty();
                gate.reasons[index] = if failures.is_empty() {
                    "all qualification gates passed".to_string()
                } else {
                    failures.join("; ")
                };
            }
        }
        Ok(gate)
    }

    fn qualified_count(&self) -> usize {
        self.qualified.iter().filter(|qualified| **qualified).count()
    }

    fn research_consensus(scores: &[f64; 8]) -> QualifiedConsensus {
        // Demo-only research path: all eight implemented models contribute, but
        // this does not promote them through the production qualification gate.
        let signals = MODEL_NAMES.iter().zip(scores.iter()).map(|(name, score)| {
            ModelSignal::Qualified { name, score: *score }
        }).collect::<Vec<_>>();
        let total_weight = STRATEGY_WEIGHTS.iter().sum::<f64>();
        let consensus = scores.iter().zip(STRATEGY_WEIGHTS).map(|(score, weight)| score * weight).sum::<f64>() / total_weight;
        let variance = scores.iter().zip(STRATEGY_WEIGHTS).map(|(score, weight)| weight * (score - consensus).powi(2)).sum::<f64>() / total_weight;
        QualifiedConsensus { score: consensus, confidence: (1.0 - variance.sqrt() * 2.4).clamp(0.0, 1.0), signals }
    }

    fn consensus(&self, scores: &[f64; 8]) -> Option<QualifiedConsensus> {
        let signals = (0..MODEL_NAMES.len()).map(|index| {
            if self.qualified[index] {
                ModelSignal::Qualified { name: MODEL_NAMES[index], score: scores[index] }
            } else {
                ModelSignal::NotQualified { name: MODEL_NAMES[index], reason: self.reasons[index].clone() }
            }
        }).collect::<Vec<_>>();
        let total_weight = STRATEGY_WEIGHTS.iter().enumerate()
            .filter(|(index, _)| matches!(signals[*index], ModelSignal::Qualified { .. }))
            .map(|(_, weight)| *weight).sum::<f64>();
        if total_weight <= 0.0 { return None; }
        let consensus = signals.iter().zip(STRATEGY_WEIGHTS).filter_map(|(signal, weight)| match signal {
            ModelSignal::Qualified { score, .. } => Some(score * weight),
            ModelSignal::NotQualified { .. } => None,
        }).sum::<f64>() / total_weight;
        let variance = signals.iter().zip(STRATEGY_WEIGHTS).filter_map(|(signal, weight)| match signal {
            ModelSignal::Qualified { score, .. } => Some(weight * (score - consensus).powi(2)),
            ModelSignal::NotQualified { .. } => None,
        }).sum::<f64>() / total_weight;
        Some(QualifiedConsensus { score: consensus, confidence: (1.0 - variance.sqrt() * 2.4).clamp(0.0, 1.0), signals })
    }
}

fn qualification_failures(evidence: &ModelEvidence) -> Vec<&'static str> {
    let mut failures = Vec::new();
    if evidence.evidence_version == 0
        || evidence.code_version.trim().is_empty()
        || evidence.holdout_dataset_id.trim().is_empty()
        || evidence.evidence_checksum.trim().is_empty()
        || !evidence.independently_evaluated {
        failures.push("unverifiable or non-independent evidence");
    }
    if !evidence.enabled_requested { failures.push("not explicitly enabled"); }
    if evidence.independent_sample_size < QUALIFICATION_MIN_SAMPLES { failures.push("insufficient independent sample"); }
    if evidence.holdout_expectancy_minor <= 0.0 { failures.push("non-positive holdout expectancy"); }
    if evidence.holdout_expectancy_ci95_lower_minor <= evidence.break_even_expectancy_minor { failures.push("expectancy CI not above break-even"); }
    if evidence.maximum_drawdown_fraction > QUALIFICATION_MAX_DRAWDOWN_FRACTION { failures.push("drawdown too high"); }
    if evidence.risk_of_ruin > QUALIFICATION_MAX_RISK_OF_RUIN { failures.push("risk of ruin too high"); }
    if evidence.parameter_variants_tested < QUALIFICATION_MIN_PARAMETER_VARIANTS
        || evidence.positive_parameter_neighbour_fraction < QUALIFICATION_MIN_STABLE_NEIGHBOURS {
        failures.push("unstable parameter neighbourhood");
    }
    if evidence.simple_baseline_lift_ci95_lower_minor <= 0.0
        || evidence.random_baseline_lift_ci95_lower_minor <= 0.0 {
        failures.push("not better than simple and random baselines");
    }
    if evidence.independent_market_count < QUALIFICATION_MIN_MARKETS
        || evidence.independent_regime_count < QUALIFICATION_MIN_REGIMES
        || evidence.maximum_single_market_profit_fraction > QUALIFICATION_MAX_MARKET_DEPENDENCY {
        failures.push("single market or regime dependency");
    }
    if !evidence.calibration_test_passed || evidence.brier_score > QUALIFICATION_MAX_BRIER_SCORE {
        failures.push("calibration failed");
    }
    if !evidence.failure_stress_test_passed { failures.push("failure stress test failed"); }
    failures
}

#[cfg(test)]
mod qualification_tests {
    use super::*;

    fn passing_evidence() -> ModelEvidence {
        ModelEvidence {
            name: MODEL_NAMES[0].to_string(),
            evidence_version: 1,
            code_version: "abc123".to_string(),
            holdout_dataset_id: "holdout-2026-01".to_string(),
            evidence_checksum: "checksum".to_string(),
            independently_evaluated: true,
            enabled_requested: true,
            independent_sample_size: QUALIFICATION_MIN_SAMPLES,
            holdout_expectancy_minor: 1.0,
            holdout_expectancy_ci95_lower_minor: 0.25,
            break_even_expectancy_minor: 0.0,
            maximum_drawdown_fraction: QUALIFICATION_MAX_DRAWDOWN_FRACTION,
            risk_of_ruin: QUALIFICATION_MAX_RISK_OF_RUIN,
            parameter_variants_tested: QUALIFICATION_MIN_PARAMETER_VARIANTS,
            positive_parameter_neighbour_fraction: QUALIFICATION_MIN_STABLE_NEIGHBOURS,
            simple_baseline_lift_ci95_lower_minor: 0.1,
            random_baseline_lift_ci95_lower_minor: 0.1,
            independent_market_count: QUALIFICATION_MIN_MARKETS,
            independent_regime_count: QUALIFICATION_MIN_REGIMES,
            maximum_single_market_profit_fraction: QUALIFICATION_MAX_MARKET_DEPENDENCY,
            calibration_test_passed: true,
            brier_score: QUALIFICATION_MAX_BRIER_SCORE,
            failure_stress_test_passed: true,
        }
    }

    #[test]
    fn complete_independent_evidence_passes_every_gate() {
        assert!(qualification_failures(&passing_evidence()).is_empty());
    }

    #[test]
    fn every_required_gate_is_fail_closed() {
        let mut evidence = passing_evidence();
        evidence.independent_sample_size = 1;
        evidence.holdout_expectancy_minor = 0.0;
        evidence.holdout_expectancy_ci95_lower_minor = 0.0;
        evidence.maximum_drawdown_fraction = 1.0;
        evidence.risk_of_ruin = 1.0;
        evidence.positive_parameter_neighbour_fraction = 0.0;
        evidence.simple_baseline_lift_ci95_lower_minor = 0.0;
        evidence.random_baseline_lift_ci95_lower_minor = 0.0;
        evidence.independent_market_count = 1;
        evidence.independent_regime_count = 1;
        evidence.maximum_single_market_profit_fraction = 1.0;
        evidence.calibration_test_passed = false;
        evidence.failure_stress_test_passed = false;
        let failures = qualification_failures(&evidence);
        assert!(failures.len() >= 8, "all evidence families must reject");
    }

    #[test]
    fn unqualified_scores_have_zero_consensus_weight() {
        let gate = QualificationGate {
            qualified: [true, false, false, false, false, false, false, false],
            reasons: std::array::from_fn(|_| String::new()),
        };
        let consensus = gate.consensus(&[0.9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]).unwrap();
        assert!((consensus.score - 0.9).abs() < 1e-12);
        assert!(matches!(consensus.signals[0], ModelSignal::Qualified { score, .. } if (score - 0.9).abs() < 1e-12));
        assert!(matches!(consensus.signals[1], ModelSignal::NotQualified { .. }));
        assert_eq!(consensus.qualified_scores().count(), 1);
    }
}
