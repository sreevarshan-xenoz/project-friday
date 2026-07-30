use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::ops;

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    pub fn none() -> Self { Self(0.0) }
    pub fn low() -> Self { Self(0.25) }
    pub fn medium() -> Self { Self(0.5) }
    pub fn high() -> Self { Self(0.75) }
    pub fn certain() -> Self { Self(1.0) }
    pub fn from_raw(value: f32) -> Self { Self(value.clamp(0.0, 1.0)) }
    pub fn from_ratio(numerator: u32, denominator: u32) -> Self {
        if denominator == 0 { Self(0.0) }
        else { Self((numerator as f32 / denominator as f32).clamp(0.0, 1.0)) }
    }
    pub fn value(&self) -> f32 { self.0 }
    pub fn and(self, other: Confidence) -> Confidence { Confidence::from_raw(self.0 * other.0) }
    pub fn or(self, other: Confidence) -> Confidence { Confidence::from_raw((self.0 + other.0) / 2.0) }
    pub fn discount(self, factor: f32) -> Confidence { Confidence::from_raw(self.0 * factor.clamp(0.0, 1.0)) }
    pub fn level(&self) -> ConfidenceLevel {
        if self.0 < 0.1 { ConfidenceLevel::None }
        else if self.0 < 0.35 { ConfidenceLevel::Low }
        else if self.0 < 0.65 { ConfidenceLevel::Medium }
        else if self.0 < 0.9 { ConfidenceLevel::High }
        else { ConfidenceLevel::Certain }
    }
    pub fn meets(&self, threshold: Confidence) -> bool { self.0 >= threshold.0 }
}

impl Default for Confidence { fn default() -> Self { Self::none() } }
impl From<f32> for Confidence { fn from(v: f32) -> Self { Self::from_raw(v) } }
impl From<Confidence> for f32 { fn from(c: Confidence) -> Self { c.0 } }
impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{:.0}%", self.0 * 100.0) }
}
impl ops::Mul for Confidence {
    type Output = Confidence;
    fn mul(self, rhs: Confidence) -> Confidence { self.and(rhs) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    None, Low, Medium, High, Certain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidenceRule {
    Multiply { sources: Vec<String> },
    Average { sources: Vec<String> },
    Discount { source: String, factor: f32 },
    Cap { source: String, max: Confidence },
    Min { sources: Vec<String> },
}

pub struct ConfidenceEngine {
    rules: Vec<(String, ConfidenceRule)>,
}

impl ConfidenceEngine {
    pub fn new() -> Self { Self { rules: Vec::new() } }
    pub fn add_rule(mut self, name: impl Into<String>, rule: ConfidenceRule) -> Self {
        self.rules.push((name.into(), rule));
        self
    }
    pub fn evaluate(&self, inputs: &HashMap<String, Confidence>) -> HashMap<String, Confidence> {
        let default_conf = Confidence::none();
        let mut outputs = HashMap::new();
        for (name, rule) in &self.rules {
            let result = match rule {
                ConfidenceRule::Multiply { sources } => {
                    let mut acc = Confidence::certain();
                    for src in sources {
                        let val = inputs.get(src.as_str()).unwrap_or(&default_conf);
                        acc = acc.and(*val);
                    }
                    acc
                }
                ConfidenceRule::Average { sources } => {
                    if sources.is_empty() { Confidence::none() }
                    else {
                        let sum: f32 = sources.iter()
                            .map(|s| inputs.get(s.as_str()).unwrap_or(&default_conf).value())
                            .sum();
                        Confidence::from_raw(sum / sources.len() as f32)
                    }
                }
                ConfidenceRule::Discount { source, factor } => {
                    let base = inputs.get(source.as_str()).unwrap_or(&default_conf);
                    base.discount(*factor)
                }
                ConfidenceRule::Cap { source, max } => {
                    let base = inputs.get(source.as_str()).unwrap_or(&default_conf);
                    Confidence::from_raw(base.value().min(max.value()))
                }
                ConfidenceRule::Min { sources } => {
                    let mut best = Confidence::certain();
                    for s in sources {
                        let val = inputs.get(s.as_str()).unwrap_or(&default_conf);
                        if val.value() < best.value() { best = *val; }
                    }
                    if sources.is_empty() { Confidence::none() } else { best }
                }
            };
            outputs.insert(name.clone(), result);
        }
        outputs
    }
}

impl Default for ConfidenceEngine { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_from_raw_clamped() {
        assert!((Confidence::from_raw(1.5).value() - 1.0).abs() < 1e-6);
        assert!((Confidence::from_raw(-0.5).value() - 0.0).abs() < 1e-6);
        assert!((Confidence::from_raw(0.5).value() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn confidence_into_f32() {
        let c: f32 = Confidence::medium().into();
        assert!((c - 0.5).abs() < 1e-6);
    }

    #[test]
    fn confidence_semantic_constructors() {
        assert!((Confidence::none().value() - 0.0).abs() < 1e-6);
        assert!((Confidence::low().value() - 0.25).abs() < 1e-6);
        assert!((Confidence::medium().value() - 0.5).abs() < 1e-6);
        assert!((Confidence::high().value() - 0.75).abs() < 1e-6);
        assert!((Confidence::certain().value() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn confidence_display() {
        assert_eq!(format!("{}", Confidence::medium()), "50%");
        assert_eq!(format!("{}", Confidence::certain()), "100%");
    }

    #[test]
    fn confidence_level_classification() {
        assert_eq!(Confidence::from_raw(0.05).level(), ConfidenceLevel::None);
        assert_eq!(Confidence::from_raw(0.2).level(), ConfidenceLevel::Low);
        assert_eq!(Confidence::from_raw(0.5).level(), ConfidenceLevel::Medium);
        assert_eq!(Confidence::from_raw(0.8).level(), ConfidenceLevel::High);
        assert_eq!(Confidence::from_raw(0.95).level(), ConfidenceLevel::Certain);
    }

    #[test]
    fn confidence_mul_operator() {
        let c = Confidence::high() * Confidence::medium();
        assert!((c.value() - 0.375).abs() < 1e-6);
    }

    #[test]
    fn confidence_or_average() {
        let c = Confidence::high().or(Confidence::medium());
        assert!((c.value() - 0.625).abs() < 1e-6);
    }

    #[test]
    fn confidence_and_multiply() {
        let c = Confidence::certain().and(Confidence::low());
        assert!((c.value() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn confidence_discount() {
        let c = Confidence::high().discount(0.5);
        assert!((c.value() - 0.375).abs() < 1e-6);
    }

    #[test]
    fn confidence_meets_threshold() {
        assert!(Confidence::high().meets(Confidence::medium()));
        assert!(!Confidence::low().meets(Confidence::medium()));
    }

    #[test]
    fn confidence_from_ratio() {
        assert!((Confidence::from_ratio(3, 4).value() - 0.75).abs() < 1e-6);
        assert!((Confidence::from_ratio(0, 0).value() - 0.0).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_multiply() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Multiply { sources: vec!["a".into()] });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_average() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Average { sources: vec!["a".into(), "b".into()] });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());
        inputs.insert("b".into(), Confidence::medium());
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.625).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_discount() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Discount { source: "a".into(), factor: 0.5 });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::from_raw(0.5));
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_cap() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Cap { source: "a".into(), max: Confidence::from_raw(0.3) });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_min() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Min { sources: vec!["a".into(), "b".into()] });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());
        inputs.insert("b".into(), Confidence::low());
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn confidence_engine_missing_input_defaults_to_none() {
        let engine = ConfidenceEngine::new()
            .add_rule("op", ConfidenceRule::Multiply { sources: vec!["missing".into()] });
        let mut inputs = HashMap::new();
        inputs.insert("a".into(), Confidence::high());
        let outputs = engine.evaluate(&inputs);
        assert!((outputs.get("op").unwrap().value() - 0.0).abs() < 1e-6);
    }
}
