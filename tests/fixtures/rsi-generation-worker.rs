//! Controlled native worker used only to test process replacement, lineage and
//! decision handoff. This is not an LLM or evidence of autonomous intelligence
//! gains.
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn accepts(n: i32) -> bool { n >= 0 }

fn put(db: &Connection, kind: &str, value: &Value) {
    db.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",
        params![kind,value["id"].as_str().unwrap(),value.to_string()]).unwrap();
}

fn generation_count(db: &Connection, campaign: &str) -> usize {
    let mut count = 0;
    let Ok(mut statement) = db.prepare("SELECT data FROM records WHERE kind='rsi_generation'") else {
        return 0;
    };
    let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0)) else {
        return 0;
    };
    for row in rows.flatten() {
        if let Ok(value) = serde_json::from_str::<Value>(&row) {
            if value["campaignId"].as_str() == Some(campaign) {
                count += 1;
            }
        }
    }
    count
}

fn record_research_read(db: &Connection, job: &mut Value, label: &str) {
    let text = std::fs::read_to_string("src/main.rs").unwrap();
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let line = text.lines().position(|line|line.starts_with("pub fn accepts")).unwrap();
    let numbered = text.lines().enumerate().skip(line).take(1)
        .map(|(index, line)| format!("{:>5} {}\n", index + 1, line))
        .collect::<String>();
    let session = "fixture-session";
    job["sessionId"] = json!(session);
    put(db, "execution_operation", &json!({
        "id": format!("fixture-research-{}-{label}", job["id"].as_str().unwrap()),
        "sessionId": session,
        "tool": "workspace_read_range",
        "status": "completed",
        "arguments": {"path":"src/main.rs","startLine":line+1,"endLine":line+1},
        "result": {"content":numbered,"startLine":line+1,"endLine":line+1,
            "totalLines":text.lines().count(),"sourceSha256":hash}
    }));
}

fn produce_boundary_patch(db: &Connection, job: &mut Value, learned: Option<&str>) {
    let text = std::fs::read_to_string("src/main.rs").unwrap();
    let old = learned.unwrap_or("pub fn accepts(n: i32) -> bool { n >= 0 }");
    let new = old.replace("n >= 0", "n > 0");
    let lines = text.lines().collect::<Vec<_>>();
    let changed = lines.iter().position(|value| *value == old).unwrap();
    let begin = changed.saturating_sub(3);
    let end = (changed + 4).min(lines.len());
    let start = begin + 1;
    let count = end - begin;
    let mut patch = format!("diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -{start},{count} +{start},{count} @@\n");
    for index in begin..end {
        if index == changed {
            patch.push_str(&format!("-{old}\n+{new}\n"));
        } else {
            patch.push_str(&format!(" {}\n", lines[index]));
        }
    }
    let hash = format!("{:x}", Sha256::digest(patch.as_bytes()));
    let id = format!("controlled-{}", job["id"].as_str().unwrap());
    put(db, "rsi_candidate", &json!({"id":id,"target":"harness","status":"proposed",
        "baseCommit":job["input"]["rsiBaseCommit"],"patch":patch,"patchSha256":hash,
        "paths":["src/main.rs"],"cases":job["input"]["rsiFrozenCases"],"oracleVersion":"controlled-boundary-v1",
        "rsiJobId":job["id"]}));
    put(db, "rsi_run", &json!({"id":format!("rsi-run-{}",job["id"].as_str().unwrap()),
        "candidateId":id,"checksPassed":true,"sourceClean":true,"diffSha256":hash}));
    job["result"] = json!({"status":"completed","rsi":{"candidateId":id,"checksPassed":true,"diffBytes":patch.len()}});
}

fn learned_boundary(db: &Connection) -> Option<String> {
    let hash = format!("{:x}",Sha256::digest(std::fs::read("src/main.rs").ok()?));
    let mut statement = db.prepare("SELECT data FROM records WHERE kind='experience'").ok()?;
    for row in statement.query_map([],|row|row.get::<_,String>(0)).ok()?.flatten() {
        let value: Value = serde_json::from_str(&row).ok()?;
        for artifact in value["researchArtifacts"].as_array().into_iter().flatten() {
            if artifact["sourceSha256"] != hash {continue;}
            for line in artifact["content"].as_str().unwrap_or("").lines() {
                if let Some(start)=line.find("pub fn accepts") {
                    let code=&line[start..];
                    if code.contains("n >= 0") {return Some(code.to_string());}
                }
            }
        }
    }
    None
}

fn produce_comment_patch(db: &Connection, job: &mut Value) {
    let text = std::fs::read_to_string("src/main.rs").unwrap();
    let lines = text.lines().collect::<Vec<_>>();
    let target = lines.iter().position(|line| line.contains("pub fn accepts")).unwrap();
    let begin = target.saturating_sub(3);
    let end = (target + 4).min(lines.len());
    let start = begin + 1;
    let old_count = end - begin;
    let mut patch = format!("diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -{start},{old_count} +{start},{} @@\n", old_count + 1);
    for index in begin..end {
        patch.push_str(&format!(" {}\n", lines[index]));
        if index == target {
            patch.push_str("+// phase-2 observation\n");
        }
    }
    let hash = format!("{:x}", Sha256::digest(patch.as_bytes()));
    let id = format!("controlled-comment-{}", job["id"].as_str().unwrap());
    put(db, "rsi_candidate", &json!({"id":id,"target":"harness","status":"proposed",
        "baseCommit":job["input"]["rsiBaseCommit"],"patch":patch,"patchSha256":hash,
        "paths":["src/main.rs"],"cases":job["input"]["rsiFrozenCases"],"oracleVersion":"controlled-comment-v1",
        "rsiJobId":job["id"]}));
    put(db, "rsi_run", &json!({"id":format!("rsi-run-{}",job["id"].as_str().unwrap()),
        "candidateId":id,"checksPassed":true,"sourceClean":true,"diffSha256":hash}));
    job["result"] = json!({"status":"completed","rsi":{"candidateId":id,"checksPassed":true,"diffBytes":patch.len()}});
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args[0] == "--rsi-worker-identity" {
        println!("{}", env!("FIXTURE_COMMIT"));
        return;
    }
    assert_eq!(args[0], "--rsi-worker-job");
    let db = Connection::open(std::path::Path::new(&std::env::var("DOMINION_DATA").unwrap()).join("dominion.sqlite")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    let raw: String = db.query_row("SELECT data FROM records WHERE kind='job' AND id=?1", [&args[1]], |row| row.get(0)).unwrap();
    let mut job: Value = serde_json::from_str(&raw).unwrap();
    let selected_domain = job["input"]["rsiSelectedDomain"].as_str().map(str::to_string);
    job["rsiDomain"] = json!(selected_domain.as_deref().unwrap_or("memory"));
    job["rsiDomainSelection"] = json!("algorithm_active");
    job["rsiSearchStrategy"] = json!({
        "mode": "active",
        "decisionSource": if selected_domain.is_some() { "parent_next_decision" } else { "fixture_default" }
    });
    job["rsiDecisionInput"] = job["input"]["rsiDecisionInput"].clone();
    let mode = job["input"]["rsiFixtureMode"].as_str().unwrap_or("").to_string();
    let research_mode = mode == "research_then_adopt";
    let campaign = job["input"]["rsiCampaignId"].as_str().unwrap_or("").to_string();
    if research_mode {
        let phase = generation_count(&db, &campaign);
        match phase {
            0 => {
                record_research_read(&db, &mut job, "base");
                job["result"] = json!({"status":"completed","rsi":{"checksPassed":false,"diffBytes":0,"fixturePhase":"research"}});
            }
            1 => {
                let learned = learned_boundary(&db);
                job["rsiResearchDelivered"] = json!(learned.is_some());
                if let Some(code) = learned {
                    produce_boundary_patch(&db, &mut job, Some(&code));
                    job["result"]["rsi"]["fixturePhase"] = json!("implementation");
                } else {
                    job["result"] = json!({"status":"needs_review","rsi":{"checksPassed":false,"diffBytes":0,"fixturePhase":"research_missing"}});
                }
            }
            2 => {
                let boundary = accepts(0);
                job["rsiBoundaryResult"] = json!(boundary);
                if boundary {
                    job["result"] = json!({"rsi":{"checksPassed":false,"fixturePhase":"old_behavior"}});
                    job["status"] = json!("completed");
                    put(&db,"job",&job);
                    return;
                }
                record_research_read(&db, &mut job, "improved");
                produce_comment_patch(&db, &mut job);
                job["result"]["rsi"]["fixturePhase"] = json!("improved_execution");
            }
            _ => {
                job["result"] = json!({"status":"completed","rsi":{"checksPassed":false,"diffBytes":0,"fixturePhase":"decision_use"}});
            }
        }
    } else {
        job["rsiDomain"] = json!(if accepts(0) { "original_executable" } else { "improved_executable" });
        if accepts(0) {
            produce_boundary_patch(&db, &mut job, None);
        } else {
            job["result"] = json!({"status":"needs_review","rsi":{"checksPassed":false,"diffBytes":0}});
        }
    }
    job["status"] = json!("completed");
    put(&db, "job", &job);
}

#[cfg(test)]
mod cases {
    #[test] fn zero_boundary(){assert!(!super::accepts(0));}
    #[test] fn negative_one(){assert!(!super::accepts(-1));}
    #[test] fn negative_two(){assert!(!super::accepts(-2));}
    #[test] fn negative_three(){assert!(!super::accepts(-3));}
    #[test] fn positive_one(){assert!(super::accepts(1));}
    #[test] fn positive_two(){assert!(super::accepts(2));}
    #[test] fn positive_three(){assert!(super::accepts(3));}
    #[test] fn positive_four(){assert!(super::accepts(4));}
}
