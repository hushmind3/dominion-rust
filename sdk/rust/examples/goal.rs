use dominion_sdk::{DominionClient, GoalCreate, WaitOptions};

#[tokio::main]
async fn main() -> dominion_sdk::Result<()> {
    let instruction = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if instruction.is_empty() {
        eprintln!("Usage: cargo run --example goal -- 'Implement and test a feature'");
        std::process::exit(2);
    }

    let sdk = DominionClient::new(
        &std::env::var("DOMINION_URL").unwrap_or_else(|_| "http://127.0.0.1:4317".into())
    )?;
    let sdk = match std::env::var("DOMINION_ADMIN_TOKEN") {
        Ok(token) if !token.is_empty() => sdk.with_admin_token(token)?,
        _ => sdk,
    };

    let goal = sdk.start_goal(GoalCreate::new(instruction).max_iterations(16)).await?;
    println!("Goal ID: {}", goal.id);
    let final_record = goal.wait(WaitOptions::default()).await?;
    println!("status = {}", final_record.status);
    println!("evidence = {:?}", final_record.evidence);
    if !final_record.is_success() {
        eprintln!("Goal did not complete; inspect the persisted review/evidence.");
        std::process::exit(1);
    }
    Ok(())
}
