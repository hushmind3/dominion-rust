//! Frozen, direction-aware paired measurements. Passing guards alone is not a gain.
use serde_json::{json, Value};

pub fn plan(value: &Value) -> Result<Vec<Value>, String> {
    let Some(rows) = value.as_array() else {
        return if value.is_null() {
            Ok(Vec::new())
        } else {
            Err("metrics는 배열이어야 합니다.".into())
        };
    };
    if rows.len() > 8 {
        return Err("사례별 지표는 최대 8개입니다.".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    rows.iter()
        .map(|row| {
            let key = row["key"]
                .as_str()
                .filter(|s| {
                    matches!(
                        *s,
                        "quality" | "elapsedMs" | "contextChars" | "tokens" | "discoveryScore"
                    )
                })
                .ok_or("알 수 없는 측정 지표입니다.")?;
            if !seen.insert(key) {
                return Err("중복 측정 지표입니다.".into());
            }
            let direction = row["direction"]
                .as_str()
                .filter(|s| matches!(*s, "higher" | "lower"))
                .ok_or("지표의 개선 방향이 필요합니다.")?;
            let threshold = |field: &str, default: f64| -> Result<f64, String> {
                let value = row
                    .get(field)
                    .map(|v| v.as_f64().unwrap_or(f64::NAN))
                    .unwrap_or(default);
                if value.is_finite() && (0.0..=1.0).contains(&value) {
                    Ok(value)
                } else {
                    Err(format!("{field}는 0~1의 유한수여야 합니다."))
                }
            };
            let absolute = row
                .get("minAbsoluteGain")
                .map(|v| v.as_f64().unwrap_or(f64::NAN))
                .unwrap_or(0.0);
            if !absolute.is_finite() || absolute < 0.0 {
                return Err("절대 향상 기준이 올바르지 않습니다.".into());
            }
            Ok(json!({"key":key,"direction":direction,
            "minRelativeGain":threshold("minRelativeGain",0.05)?,
            "maxRelativeRegression":threshold("maxRelativeRegression",0.0)?,
            "minAbsoluteGain":absolute}))
        })
        .collect()
}

pub fn compare(plan: &[Value], measurement: &Value) -> Result<(usize, usize, Vec<Value>), String> {
    let mut gains = 0;
    let mut losses = 0;
    let mut details = Vec::new();
    for spec in plan {
        let key = spec["key"].as_str().ok_or("손상된 지표 계획")?;
        let row = &measurement[key];
        let number = |field: &str| {
            row[field]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .ok_or_else(|| format!("{key}.{field} 실제 측정값이 없습니다."))
        };
        let before = number("baseline")?;
        let after = number("candidate")?;
        if matches!(key, "quality" | "discoveryScore") && (before > 1.0 || after > 1.0) {
            return Err("정확도는 0~1 범위의 측정값이어야 합니다.".into());
        }
        if key == "elapsedMs" && row["samples"].as_u64().unwrap_or(0) < 5 {
            return Err("시간 지표는 양쪽 각각 5회 이상 측정한 중앙값이 필요합니다.".into());
        }
        let delta = if spec["direction"] == "higher" {
            after - before
        } else {
            before - after
        };
        let required = (before * spec["minRelativeGain"].as_f64().unwrap_or(0.05))
            .max(spec["minAbsoluteGain"].as_f64().unwrap_or(0.0));
        let improved = delta > 0.0 && delta >= required;
        let regressed = -delta > before * spec["maxRelativeRegression"].as_f64().unwrap_or(0.0);
        gains += usize::from(improved);
        losses += usize::from(regressed);
        details.push(json!({"key":key,"baseline":before,"candidate":after,
            "improved":improved,"regressed":regressed,"direction":spec["direction"]}));
    }
    Ok((gains, losses, details))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equal_test_passes_can_have_a_measured_quality_gain_but_cost_regressions_block() {
        let specs = plan(&json!([
            {"key":"quality","direction":"higher","minRelativeGain":0.05},
            {"key":"tokens","direction":"lower"}
        ]))
        .unwrap();
        let (gains, losses, _) = compare(
            &specs,
            &json!({
                "quality":{"baseline":0.7,"candidate":0.9},
                "tokens":{"baseline":100,"candidate":100}
            }),
        )
        .unwrap();
        assert_eq!((gains, losses), (1, 0));
        assert_eq!(
            compare(
                &specs,
                &json!({"quality":{"baseline":0.7,"candidate":0.9},
            "tokens":{"baseline":100,"candidate":120}})
            )
            .unwrap()
            .1,
            1
        );
        assert!(compare(&specs, &json!({"quality":{"baseline":0.7,"candidate":0.9}})).is_err());
    }
    #[test]
    fn noisy_or_missing_timing_is_not_evidence_of_an_improvement() {
        let specs =
            plan(&json!([{"key":"elapsedMs","direction":"lower","minRelativeGain":0.15}])).unwrap();
        assert!(compare(
            &specs,
            &json!({"elapsedMs":{"baseline":100,"candidate":50,"samples":1}})
        )
        .is_err());
        assert_eq!(
            compare(
                &specs,
                &json!({"elapsedMs":{"baseline":100,"candidate":95,"samples":5}})
            )
            .unwrap()
            .0,
            0
        );
        assert!(
            plan(&json!([{"key":"tokens","direction":"lower","maxRelativeRegression":-1}]))
                .is_err()
        );
    }
}
