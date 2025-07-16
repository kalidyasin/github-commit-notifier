use std::env;
use std::error::Error;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::Deserialize;
use tokio::time;

#[derive(Deserialize, Debug)]
struct CommitDetails {
    message: String,
}

#[derive(Deserialize, Debug)]
struct Author {
    login: String,
}

#[derive(Deserialize, Debug)]
struct Commit {
    sha: String,
    html_url: String,
    author: Option<Author>,
    commit: CommitDetails,
}

#[derive(Deserialize, Debug)]
struct Repository {
    full_name: String,
}

async fn get_latest_commit(
    client: &reqwest::Client,
    repo_full_name: &str,
    token: &str,
) -> Result<Option<Commit>, Box<dyn Error>> {
    let url = format!(
        "https://api.github.com/repos/{}/commits",
        repo_full_name
    );
    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("token {}", token))
        .send()
        .await?;

    if response.status().is_success() {
        let commits: Vec<Commit> = response.json().await?;
        Ok(commits.into_iter().next())
    } else {
        Ok(None)
    }
}

async fn get_repositories(
    client: &reqwest::Client,
    org_name: &str,
    token: &str,
) -> Result<Vec<Repository>, Box<dyn Error>> {
    let url = format!("https://api.github.com/orgs/{}/repos", org_name);
    let response = client
        .get(&url)
        .header(AUTHORIZATION, format!("token {}", token))
        .send()
        .await?;

    if response.status().is_success() {
        let repos: Vec<Repository> = response.json().await?;
        Ok(repos)
    } else {
        println!(
            "Failed to fetch repositories for {}: {}",
            org_name,
            response.status()
        );
        Ok(Vec::new())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv::dotenv().ok();

    let token = env::var("GITHUB_TOKEN")?;
    let orgs = env::var("ORGS")?;

    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("reqwest"));
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github.v3+json"),
    );

    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()?;

    let mut last_commits: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    loop {
        for org_name in orgs.split(',') {
            println!("Fetching repositories for {}", org_name);
            let repos = get_repositories(&client, org_name, &token).await?;

            for repo in repos {
                if let Ok(Some(commit)) =
                    get_latest_commit(&client, &repo.full_name, &token).await
                {
                    if let Some(last_commit_sha) = last_commits.get(&repo.full_name) {
                        if last_commit_sha != &commit.sha {
                            println!(
                                // "New commit in {} by {}: {}\n    Message: {}\n    URL: {}",
                                "New commit in {}\nBy {}: Message: {}\nURL: {}",
                                repo.full_name,
                                commit.author.as_ref().map_or("Unknown", |a| &a.login),
                                // commit.sha,
                                commit.commit.message,
                                commit.html_url
                            );
                            last_commits.insert(repo.full_name.clone(), commit.sha.clone());
                        }
                    } else {
                        last_commits.insert(repo.full_name.clone(), commit.sha.clone());
                    }
                }
            }
        }

        time::sleep(Duration::from_secs(5)).await;
    }
}
