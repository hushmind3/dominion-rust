//! Controlled native worker used only to test process replacement and lineage.
//! This is not an LLM or evidence of autonomous intelligence gains.
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn accepts(n: i32) -> bool { n >= 0 }

fn put(db: &Connection, kind: &str, value: &Value) {
    db.execute("INSERT INTO records(kind,id,data) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data",
        params![kind,value["id"].as_str().unwrap(),value.to_string()]).unwrap();
}
fn main() {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args[0]=="--rsi-worker-identity" {println!("{}",env!("FIXTURE_COMMIT"));return;}
    assert_eq!(args[0],"--rsi-worker-job");
    let db=Connection::open(std::path::Path::new(&std::env::var("DOMINION_DATA").unwrap()).join("dominion.sqlite")).unwrap();
    db.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
    let raw:String=db.query_row("SELECT data FROM records WHERE kind='job' AND id=?1",[&args[1]],|r|r.get(0)).unwrap();
    let mut job:Value=serde_json::from_str(&raw).unwrap();
    job["rsiDomain"]=json!(if accepts(0){"original_executable"}else{"improved_executable"});
    if accepts(0) {
        let text=std::fs::read_to_string("src/main.rs").unwrap();
        let old="pub fn accepts(n: i32) -> bool { n >= 0 }";
        let new="pub fn accepts(n: i32) -> bool { n > 0 }";
        let lines=text.lines().collect::<Vec<_>>();
        let changed=lines.iter().position(|value|*value==old).unwrap();
        let begin=changed.saturating_sub(3);let end=(changed+4).min(lines.len());
        let start=begin+1;let count=end-begin;
        let mut patch=format!("diff --git a/src/main.rs b/src/main.rs\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -{start},{count} +{start},{count} @@\n");
        for index in begin..end {
            if index==changed{patch.push_str(&format!("-{old}\n+{new}\n"));}
            else{patch.push_str(&format!(" {}\n",lines[index]));}
        }
        let hash=format!("{:x}",Sha256::digest(patch.as_bytes()));
        let id=format!("controlled-{}",job["id"].as_str().unwrap());
        put(&db,"rsi_candidate",&json!({"id":id,"target":"harness","status":"proposed",
            "baseCommit":job["input"]["rsiBaseCommit"],"patch":patch,"patchSha256":hash,
            "paths":["src/main.rs"],"cases":job["input"]["rsiFrozenCases"],"oracleVersion":"controlled-boundary-v1",
            "rsiJobId":job["id"]}));
        // Static integrity is controlled by this fixture. The parent still
        // independently compiles and executes BOTH variants before adopting.
        put(&db,"rsi_run",&json!({"id":format!("rsi-run-{}",job["id"].as_str().unwrap()),
            "candidateId":id,"checksPassed":true,"sourceClean":true,"diffSha256":hash}));
        job["result"]=json!({"status":"completed","rsi":{"candidateId":id,"checksPassed":true}});
    } else {
        job["result"]=json!({"status":"needs_review","rsi":{"checksPassed":false}});
    }
    job["status"]=json!("completed");put(&db,"job",&job);
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
