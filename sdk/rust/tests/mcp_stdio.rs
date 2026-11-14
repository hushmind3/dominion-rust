use serde_json::{json,Value};
use tokio::{io::{AsyncBufReadExt,AsyncWriteExt,BufReader},process::Command};

#[tokio::test]
async fn actual_child_process_implements_dominion_mcp_contract() {
    let mut child=Command::new(env!("CARGO_BIN_EXE_dominion-sdk-tool-server"))
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped())
        .spawn().expect("launch Rust MCP process");
    let mut input=child.stdin.take().unwrap();
    let mut reader=BufReader::new(child.stdout.take().unwrap()).lines();
    for (request, expected) in [
      (json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),"2024-11-05"),
      (json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),"word_count"),
      (json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"word_count","arguments":{"text":"Rust SDK is native"}}}),"4"),
    ] {
        input.write_all(request.to_string().as_bytes()).await.unwrap();
        input.write_all(b"\n").await.unwrap(); input.flush().await.unwrap();
        let line=reader.next_line().await.unwrap().unwrap();
        let reply:Value=serde_json::from_str(&line).unwrap();
        match request["id"].as_u64().unwrap() {
          1=>assert_eq!(reply["result"]["protocolVersion"],expected),
          2=>assert_eq!(reply["result"]["tools"][0]["name"],expected),
          _=>{
            let payload:Value=serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(payload["count"],4);
            assert_eq!(reply["result"]["isError"],false);
          }
        }
    }
    drop(input);
    assert!(child.wait().await.unwrap().success());
}
