//! Deterministic multi-agent debate and conflict resolution.
//!
//! Peers in a room can disagree about whether work succeeded. The harness must
//! not turn the last speaker into truth by default, and it must not invent a
//! resolution when there is no conflict. This module detects outcome claims,
//! resolves disagreements with a selected kernel, and reports the decision as
//! structured data so the next speaker sees the same ruling.
use serde_json::{json, Value};

pub const KERNELS: &[&str] = &["evidence-first", "owner-authority", "latest-speaker"];

const SUCCESS_MARKERS: &[&str] = &[
    "완료", "성공", "됐습니다", "됐다", "끝냈", "마쳤", "passed", "success",
    "completed", "done",
];
const FAILURE_MARKERS: &[&str] = &[
    "실패", "못 했", "못했", "안 됐", "안됐", "불가능", "오류", "에러", "failed",
    "failure", "cannot", "error",
];

fn polarity(text: &str) -> i64 {
    let lower = text.to_lowercase();
    let success = SUCCESS_MARKERS
        .iter()
        .filter(|marker| lower.contains(*marker))
        .count() as i64;
    let failure = FAILURE_MARKERS
        .iter()
        .filter(|marker| lower.contains(*marker))
        .count() as i64;
    success - failure
}

fn evidence_score(output: &Value) -> i64 {
    let mut score = output["evidenceCount"].as_u64().unwrap_or(0) as i64 * 3;
    if output["toolCalls"].as_array().is_some_and(|calls| !calls.is_empty()) {
        score += 2;
    }
    let content = output["content"].as_str().unwrap_or("").to_lowercase();
    for marker in ["workspace_read", "workspace_write", "shell_exec", "http", "exec", ".rs", ".txt", ".md"] {
        if content.contains(marker) {
            score += 1;
        }
    }
    score
}

/// Resolves conflicting outcome claims in `outputs`. `owner_text` is the
/// authoritative owner message; `strategy` is the selected kernel. The result
/// is empty (`conflict=false`) when all speakers agree or no one made an
/// outcome claim.
pub fn resolve(owner_text: &str, outputs: &[Value], strategy: &str) -> Value {
    let strategy = if KERNELS.contains(&strategy) {
        strategy
    } else {
        "evidence-first"
    };
    let claims = outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| {
            let content = output["content"].as_str().unwrap_or("");
            let sign = polarity(content);
            (sign != 0).then(|| (index, output, sign))
        })
        .collect::<Vec<_>>();
    let has_success = claims.iter().any(|(_, _, sign)| *sign > 0);
    let has_failure = claims.iter().any(|(_, _, sign)| *sign < 0);
    if !(has_success && has_failure) {
        return json!({
            "conflict": false,
            "strategy": strategy,
            "outcome": "unresolved",
            "claims": claims.iter().map(|(_, output, sign)| json!({
                "agentId": output["agentId"],
                "messageId": output["messageId"],
                "polarity": sign,
            })).collect::<Vec<_>>(),
        });
    }
    let owner_polarity = polarity(owner_text);
    let winner = match strategy {
        "latest-speaker" => claims.last().copied(),
        "owner-authority" => claims
            .iter()
            .rev()
            .find(|(_, _, sign)| owner_polarity != 0 && *sign == owner_polarity.signum())
            .copied()
            .or_else(|| claims.last().copied()),
        _ => claims
            .iter()
            .copied()
            .max_by(|(a_index, a, _), (b_index, b, _)| {
                evidence_score(a)
                    .cmp(&evidence_score(b))
                    .then_with(|| a_index.cmp(b_index))
            }),
    };
    let Some((index, output, sign)) = winner else {
        return json!({"conflict": false, "strategy": strategy, "outcome": "unresolved"});
    };
    let outcome = if sign > 0 { "success" } else { "failure" };
    json!({
        "conflict": true,
        "strategy": strategy,
        "outcome": outcome,
        "winner": {
            "agentId": output["agentId"],
            "name": output["name"],
            "messageId": output["messageId"],
            "evidenceScore": evidence_score(output),
            "position": index,
        },
        "ownerPolarity": owner_polarity,
        "claims": claims.iter().map(|(_, output, sign)| json!({
            "agentId": output["agentId"],
            "messageId": output["messageId"],
            "polarity": sign,
            "evidenceScore": evidence_score(output),
        })).collect::<Vec<_>>(),
        "revision": crate::engine::memory::stable_id(&[
            strategy,
            &claims.iter().map(|(_, output, _)| {
                output["messageId"].as_str().unwrap_or("").to_string()
            }).collect::<Vec<_>>().join(","),
        ]),
    })
}

/// A short instruction for the next speaker so every participant works from
/// the same ruling instead of re-litigating the disagreement.
pub fn guidance(resolution: &Value) -> Option<String> {
    if resolution["conflict"].as_bool() != Some(true) {
        return None;
    }
    Some(format!(
        "Engine conflict resolution ({}): earlier participants disagreed about the outcome. Treat the {} claim by {} as the preferred reading; do not present the opposite as an owner fact.",
        resolution["strategy"].as_str().unwrap_or("evidence-first"),
        resolution["outcome"].as_str().unwrap_or("unresolved"),
        resolution["winner"]["name"]
            .as_str()
            .or_else(|| resolution["winner"]["agentId"].as_str())
            .unwrap_or("a participant"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(agent: &str, content: &str, evidence: u64) -> Value {
        json!({
            "agentId": agent, "name": agent, "messageId": format!("m-{agent}"),
            "content": content, "evidenceCount": evidence,
        })
    }

    #[test]
    fn agreement_does_not_create_a_conflict() {
        let resolution = resolve(
            "배포해",
            &[output("a", "배포를 완료했습니다", 0), output("b", "배포 성공", 1)],
            "evidence-first",
        );
        assert_eq!(resolution["conflict"], false);
    }

    #[test]
    fn evidence_first_prefers_the_supported_claim() {
        let resolution = resolve(
            "테스트를 실행해",
            &[
                output("a", "테스트를 실패했습니다", 0),
                output("b", "테스트를 완료했습니다", 2),
            ],
            "evidence-first",
        );
        assert_eq!(resolution["conflict"], true, "{resolution}");
        assert_eq!(resolution["outcome"], "success", "{resolution}");
        assert_eq!(resolution["winner"]["agentId"], "b");
    }

    #[test]
    fn latest_speaker_uses_the_last_claim() {
        let resolution = resolve(
            "테스트를 실행해",
            &[
                output("a", "테스트 성공", 5),
                output("b", "테스트 실패", 0),
            ],
            "latest-speaker",
        );
        assert_eq!(resolution["outcome"], "failure", "{resolution}");
        assert_eq!(resolution["winner"]["agentId"], "b");
    }

    #[test]
    fn owner_authority_follows_an_explicit_owner_statement() {
        let resolution = resolve(
            "아직 실패 상태야",
            &[
                output("a", "테스트를 완료했습니다", 0),
                output("b", "테스트를 실패했습니다", 0),
            ],
            "owner-authority",
        );
        assert_eq!(resolution["outcome"], "failure", "{resolution}");
        assert_eq!(resolution["winner"]["agentId"], "b");
        assert!(guidance(&resolution).unwrap().contains("failure"));
    }
}
