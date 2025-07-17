mod github_client;
mod models;
mod notifier;

use anyhow::{Context, Result};
use notifier::GithubNotifier;
use std::env;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let token = env::var("GITHUB_TOKEN").context("GITHUB_TOKEN not set")?;
    let orgs = env::var("ORGS").context("ORGS not set")?;
    let sleep_secs = env::var("SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let notifier = GithubNotifier::new(token, orgs)?;

    loop {
        println!("Checking for notifications...");
        if let Err(e) = notifier.check_all_repos().await {
            eprintln!("An error occurred: {}. Retrying...", e);
        }
        time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}
