use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, ETAG, IF_NONE_MATCH, USER_AGENT};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::env;
use std::process::Command;
use std::time::Duration;
use tokio::time;

#[derive(Deserialize, Debug, Clone)]
struct Repo {
    full_name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Branch {
    name: String,
    commit: Commit,
}

#[derive(Deserialize, Debug, Clone)]
struct Commit {
    sha: String,
}

#[derive(Deserialize, Debug, Clone)]
struct FullCommit {
    html_url: String,
    commit: CommitDetails,
}

#[derive(Deserialize, Debug, Clone)]
struct CommitDetails {
    message: String,
    author: CommitAuthor,
}

#[derive(Deserialize, Debug, Clone)]
struct CommitAuthor {
    name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct PullRequest {
    id: u64,
    html_url: String,
    title: String,
    user: User,
}

#[derive(Deserialize, Debug, Clone)]
struct User {
    login: String,
}

struct GithubClient {
    client: reqwest::Client,
}

impl GithubClient {
    fn new(token: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("github-commit-notifier"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github.v3+json"));
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("token {}", token))?);

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self { client })
    }

    async fn get_paged<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<(Vec<T>, Option<String>)> {
        let mut request = self.client.get(url);
        if let Some(etag) = etag {
            request = request.header(IF_NONE_MATCH, etag);
        }

        let response = request.send().await?;

        if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok((Vec::new(), etag.map(String::from)));
        }

        let new_etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        if response.status().is_success() {
            let items = response.json().await?;
            Ok((items, new_etag))
        } else {
            Err(anyhow!("Failed to fetch {}: {}", url, response.status()))
        }
    }

    async fn get_commit(&self, repo_full_name: &str, sha: &str) -> Result<FullCommit> {
        let url = format!(
            "https://api.github.com/repos/{}/commits/{}",
            repo_full_name, sha
        );
        let response = self.client.get(&url).send().await?;
        if response.status().is_success() {
            let commit = response.json().await?;
            Ok(commit)
        } else {
            Err(anyhow!(
                "Failed to fetch commit {}: {}",
                sha,
                response.status()
            ))
        }
    }
}

struct GithubNotifier {
    client: GithubClient,
    orgs: Vec<String>,
    seen_commits: HashMap<String, String>, // key: "repo/branch", value: sha
    seen_prs: HashMap<String, HashSet<u64>>, // key: "repo", value: set of PR IDs
    seen_branches: HashMap<String, HashSet<String>>, // key: "repo", value: set of branch names
    etags: HashMap<String, String>, // key: url, value: etag
}

impl GithubNotifier {
    fn new(token: String, orgs: String) -> Result<Self> {
        Ok(Self {
            client: GithubClient::new(token)?,
            orgs: orgs.split(',').map(String::from).collect(),
            seen_commits: HashMap::new(),
            seen_prs: HashMap::new(),
            seen_branches: HashMap::new(),
            etags: HashMap::new(),
        })
    }

    async fn check_all_repos(&mut self) -> Result<()> {
        let mut all_repos = Vec::new();
        for org in &self.orgs {
            let url = format!("https://api.github.com/orgs/{}/repos", org);
            let (repos, _etag) = self.client.get_paged::<Repo>(&url, None).await
                .with_context(|| format!("Failed to get repos for org {}", org))?;
            all_repos.extend(repos);
        }

        for repo in all_repos {
            self.check_repo(repo).await;
        }

        Ok(())
    }

    async fn check_repo(&mut self, repo: Repo) {
        if let Err(e) = self.check_branches_and_commits(&repo).await {
            eprintln!("Error checking branches for {}: {}", repo.full_name, e);
        }
        if let Err(e) = self.check_pull_requests(&repo).await {
            eprintln!("Error checking PRs for {}: {}", repo.full_name, e);
        }
    }

    async fn check_branches_and_commits(&mut self, repo: &Repo) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/branches", repo.full_name);
        let etag = self.etags.get(&url).cloned();
        let (branches, new_etag) = self.client.get_paged::<Branch>(&url, etag.as_deref()).await?;

        if let Some(new_etag) = new_etag {
            self.etags.insert(url.clone(), new_etag);
        }

        let mut current_branches = HashSet::new();
        let mut commits_to_notify = Vec::new();

        for branch in branches {
            current_branches.insert(branch.name.clone());
            let key = format!("{}/{}", repo.full_name, branch.name);
            if let Some(seen_sha) = self.seen_commits.get(&key) {
                if *seen_sha != branch.commit.sha {
                    match self.client.get_commit(&repo.full_name, &branch.commit.sha).await {
                        Ok(commit_details) => {
                            commits_to_notify.push((repo.full_name.clone(), branch.name.clone(), commit_details));
                        }
                        Err(e) => {
                            eprintln!("Failed to get commit details for {}: {}", branch.commit.sha, e);
                        }
                    }
                    self.seen_commits.insert(key, branch.commit.sha);
                }
            } else {
                self.seen_commits.insert(key, branch.commit.sha);
            }
        }

        let new_branches_to_notify = {
            if let Some(seen_repo_branches) = self.seen_branches.get_mut(&repo.full_name) {
                let new_branches = current_branches.difference(seen_repo_branches).cloned().collect::<Vec<_>>();
                for branch_name in &new_branches {
                    seen_repo_branches.insert(branch_name.clone());
                }
                new_branches
            } else {
                self.seen_branches.insert(repo.full_name.clone(), current_branches);
                Vec::new()
            }
        };

        for (repo_full_name, branch_name, commit) in commits_to_notify {
            self.notify_commit(&repo_full_name, &branch_name, &commit).await;
        }

        for branch_name in new_branches_to_notify {
            self.notify_branch(&repo.full_name, &branch_name).await;
        }

        Ok(())
    }

    async fn check_pull_requests(&mut self, repo: &Repo) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/pulls", repo.full_name);
        let etag = self.etags.get(&url).cloned();
        let (prs, new_etag) = self.client.get_paged::<PullRequest>(&url, etag.as_deref()).await?;

        if let Some(new_etag) = new_etag {
            self.etags.insert(url, new_etag);
        }

        let mut new_prs_to_notify = Vec::new();
        {
            let seen_repo_prs = self.seen_prs.entry(repo.full_name.clone()).or_default();
            for pr in prs {
                if !seen_repo_prs.contains(&pr.id) {
                    new_prs_to_notify.push(pr.clone());
                    seen_repo_prs.insert(pr.id);
                }
            }
        }

        for pr in new_prs_to_notify {
            self.notify_pr(&repo.full_name, &pr).await;
        }
        Ok(())
    }

    async fn notify_commit(&self, repo_full_name: &str, branch_name: &str, commit: &FullCommit) {
        let title = format!("New Commit on {}/{}", repo_full_name, branch_name);
        let body = format!(
            "By {}: {}\nURL: {}",
            commit.commit.author.name, commit.commit.message, commit.html_url
        );
        println!("{} - {}", title, body);
        self.send_notification(&title, &body);
    }

    async fn notify_pr(&self, repo_full_name: &str, pr: &PullRequest) {
        let title = format!("New PR in {}", repo_full_name);
        let body = format!("#{} {} by {}\n{}", pr.id, pr.title, pr.user.login, pr.html_url);
        println!("{} - {}", title, body);
        self.send_notification(&title, &body);
    }

    async fn notify_branch(&self, repo_full_name: &str, branch_name: &str) {
        let title = format!("New Branch in {}", repo_full_name);
        let body = format!("Branch: {}", branch_name);
        println!("{} - {}", title, body);
        self.send_notification(&title, &body);
    }

    fn send_notification(&self, title: &str, body: &str) {
        if let Err(e) = Command::new("notify-send").arg(title).arg(body).spawn() {
            eprintln!("Failed to send notification: {}", e);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let token = env::var("GITHUB_TOKEN").context("GITHUB_TOKEN not set")?;
    let orgs = env::var("ORGS").context("ORGS not set")?;
    let sleep_secs = env::var("SLEEP_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);

    let mut notifier = GithubNotifier::new(token, orgs)?;

    loop {
        println!("Checking for notifications...");
        if let Err(e) = notifier.check_all_repos().await {
            eprintln!("An error occurred: {}. Retrying...", e);
        }
        time::sleep(Duration::from_secs(sleep_secs)).await;
    }
}