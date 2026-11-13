//! Pre-modification forecasting for RSI generations.
//!
//! Inspired by "Predict Before You Modify": the coordinator records the gain it
//! expects before a candidate is built, then learns from the signed bias and
//! absolute error measured by the independent paired evaluation.

use serde_json::{json, Value};

/// Mean absolute error and signed bias over rows that actually carried a forecast.
fn stats<'a>(rows: impl Iterator<Item = &'a Value>) -> Option<(f64, f64)> {
    let (mut error, mut bias, mut count) = (0.0, 0.0, 0u64);
    for row in rows {
        let Some(row_error) = row["forecast"]["error"].as_f64() else { continue };
        error += row_error;
        bias += row["forecast"]["bias"].as_f64().unwrap_or(0.0);
        count += 1;
    }
    (count > 0).then(|| (error / count as f64, bias / count as f64))
}

/// Measured forecast error and signed bias for a domain.
///
/// Only generations that actually carried a forecast contribute. A domain with
/// no forecast history reports zeros rather than a fabricated confidence.
pub(crate) fn domain_error(history: &[Value], domain: &str) -> (f64, f64) {
    stats(history.iter().filter(|row| row["domain"] == domain))
        .unwrap_or((0.0, 0.0))
}

/// Predict the next generation's verified holdout gain from measured history.
///
/// The signed bias is subtracted, so a coordinator that repeatedly over-predicts
/// stops re-committing the same overestimate. Without measurements the forecast
/// is zero and explicitly labelled `no_measured_history`.
pub(crate) fn forecast_next(history: &[Value], domain: &str, basis: &str) -> Value {
    let domain_rows = history
        .iter()
        .filter(|row| row["forecast"]["domain"].as_str() == Some(domain))
        .collect::<Vec<_>>();
    let measured = history
        .iter()
        .filter(|row| row["evaluation"]["metrics"]["improvedHoldouts"].as_u64().is_some())
        .collect::<Vec<_>>();
    let pool = if domain_rows.is_empty() { &measured } else { &domain_rows };
    let actuals = pool
        .iter()
        .filter_map(|row| row["evaluation"]["metrics"]["improvedHoldouts"].as_f64())
        .collect::<Vec<_>>();
    if actuals.is_empty() {
        return json!({
            "domain": domain,
            "expectedImprovedHoldouts": 0.0,
            "priorError": Value::Null,
            "priorBias": Value::Null,
            "basis": "no_measured_history",
            "decisionBasis": basis,
        });
    }
    let mean = actuals.iter().sum::<f64>() / actuals.len() as f64;
    let measured_error = stats(domain_rows.iter().copied());
    let (prior_error, bias) = measured_error.unwrap_or((0.0, 0.0));
    json!({
        "domain": domain,
        "expectedImprovedHoldouts": (mean - bias).max(0.0),
        "priorError": measured_error.map(|(error, _)| error),
        "priorBias": measured_error.map(|(_, bias)| bias),
        "basis": if domain_rows.is_empty() { "measured_global_history" } else { "measured_domain_history" },
        "decisionBasis": basis,
    })
}

/// Attach the forecast that preceded this generation and the measured outcome.
///
/// Returns false when either side lacks a number, so an unmeasured or
/// unpreceded generation never earns a prediction-error record.
pub(crate) fn attach(generation: &mut Value, prior: &Value) -> bool {
    let Some(expected) = prior["expectedImprovedHoldouts"].as_f64() else {
        return false;
    };
    let Some(actual) = generation["evaluation"]["metrics"]["improvedHoldouts"].as_f64() else {
        return false;
    };
    generation["forecast"] = json!({
        "domain": prior["domain"],
        "predictedImprovedHoldouts": expected,
        "actualImprovedHoldouts": actual,
        "error": (expected - actual).abs(),
        "bias": expected - actual,
        "basis": prior["basis"],
    });
    true
}

#[cfg(test)]
mod tests {
    use super::{attach, domain_error, forecast_next};
    use serde_json::json;

    fn history() -> Vec<serde_json::Value> {
        vec![
            json!({"domain":"memory","forecast":{"domain":"memory","predictedImprovedHoldouts":3.0,
                "actualImprovedHoldouts":1.0,"error":2.0,"bias":2.0},
                "evaluation":{"metrics":{"improvedHoldouts":1}}}),
            json!({"domain":"memory","forecast":{"domain":"memory","predictedImprovedHoldouts":3.0,
                "actualImprovedHoldouts":2.0,"error":1.0,"bias":1.0},
                "evaluation":{"metrics":{"improvedHoldouts":2}}}),
        ]
    }

    #[test]
    fn forecast_corrects_measured_bias() {
        let forecast = forecast_next(&history(), "memory", "accepted_implementation");
        assert_eq!(forecast["basis"], "measured_domain_history");
        // Mean measured gain 1.5 minus mean bias 1.5 leaves no expected gain.
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0), "{forecast}");
        assert_eq!(forecast["priorError"].as_f64(), Some(1.5));
    }

    #[test]
    fn forecast_without_measurements_claims_nothing() {
        let forecast = forecast_next(&[], "tools", "no_progress");
        assert_eq!(forecast["basis"], "no_measured_history");
        assert_eq!(forecast["expectedImprovedHoldouts"].as_f64(), Some(0.0));
        assert!(forecast["priorError"].is_null());
    }

    #[test]
    fn domain_error_only_counts_forecast_rows() {
        let (error, bias) = domain_error(&history(), "memory");
        assert_eq!(error, 1.5);
        assert_eq!(bias, 1.5);
        assert_eq!(domain_error(&history(), "tools"), (0.0, 0.0));
    }

    #[test]
    fn attach_needs_both_a_forecast_and_a_measurement() {
        let mut generation = json!({"evaluation":{"metrics":{"improvedHoldouts":2}}});
        assert!(!attach(&mut generation, &json!({})));
        assert!(generation.get("forecast").is_none());
        let prior = json!({"domain":"memory","expectedImprovedHoldouts":3.0,"basis":"measured_domain_history"});
        assert!(attach(&mut generation, &prior));
        assert_eq!(generation["forecast"]["error"].as_f64(), Some(1.0));
        assert_eq!(generation["forecast"]["bias"].as_f64(), Some(1.0));
    }
}
