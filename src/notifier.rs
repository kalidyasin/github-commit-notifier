use crate::github_client::GithubClient;
use crate::models::{Branch, FullCommit, PullRequest, Repo};
use anyhow::{Context, Result};
use futures::{stream, StreamExt};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

const CONCURRENT_REQUESTS: usize = 10;

#[derive(Clone)]
pub struct GithubNotifier {
    client: Arc<GithubClient>,
    orgs: Vec<String>,
    seen_commits: Arc<Mutex<HashMap<String, String>>>,
    seen_prs: Arc<Mutex<HashMap<String, HashSet<u64>>>>,
    seen_branches: Arc<Mutex<HashMap<String, HashSet<String>>>>,
    etags: Arc<Mutex<HashMap<String, String>>>,
}

impl GithubNotifier {
    pub fn new(token: String, orgs: String) -> Result<Self> {
        Ok(Self {
            client: Arc::new(GithubClient::new(token)?),
            orgs: orgs.split(',').map(String::from).collect(),
            seen_commits: Arc::new(Mutex::new(HashMap::new())),
            seen_prs: Arc::new(Mutex::new(HashMap::new())),
            seen_branches: Arc::new(Mutex::new(HashMap::new())),
            etags: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn check_all_repos(&self) -> Result<()> {
        let mut all_repos = Vec::new();
        for org in &self.orgs {
            let url = format!("https://api.github.com/orgs/{}/repos", org);
            let (repos, _etag) = self
                .client
                .get_paged::<Repo>(&url, None)
                .await
                .with_context(|| format!("Failed to get repos for org {}", org))?;
            all_repos.extend(repos);
        }

        stream::iter(all_repos)
            .for_each_concurrent(CONCURRENT_REQUESTS, |repo| {
                let notifier = self.clone();
                async move {
                    notifier.check_repo(repo).await;
                }
            })
            .await;

        Ok(())
    }

    async fn check_repo(&self, repo: Repo) {
        if let Err(e) = self.check_branches_and_commits(&repo).await {
            eprintln!("Error checking branches for {}: {}", repo.full_name, e);
        }
        if let Err(e) = self.check_pull_requests(&repo).await {
            eprintln!("Error checking PRs for {}: {}", repo.full_name, e);
        }
    }

    async fn check_branches_and_commits(&self, repo: &Repo) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/branches", repo.full_name);
        let etag = self.etags.lock().await.get(&url).cloned();
        let (branches, new_etag) = self
            .client
            .get_paged::<Branch>(&url, etag.as_deref())
            .await?;

        if let Some(new_etag) = new_etag {
            self.etags.lock().await.insert(url.clone(), new_etag);
        }

        let mut current_branches = HashSet::new();
        let mut commits_to_notify = Vec::new();
        let mut branches_with_commit_info = Vec::new();

        for branch in branches {
            current_branches.insert(branch.name.clone());
            branches_with_commit_info.push(branch.clone());
            let key = format!("{}/{}", repo.full_name, branch.name);
            let mut seen_commits = self.seen_commits.lock().await;
            if let Some(seen_sha) = seen_commits.get(&key) {
                if *seen_sha != branch.commit.sha {
                    match self
                        .client
                        .get_commit(&repo.full_name, &branch.commit.sha)
                        .await
                    {
                        Ok(commit_details) => {
                            commits_to_notify
                                .push((repo.full_name.clone(), branch.name.clone(), commit_details));
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to get commit details for {}: {}",
                                branch.commit.sha, e
                            );
                        }
                    }
                    seen_commits.insert(key, branch.commit.sha);
                }
            } else {
                seen_commits.insert(key, branch.commit.sha);
            }
        }

        let new_branches_to_notify = {
            let mut seen_branches = self.seen_branches.lock().await;
            if let Some(seen_repo_branches) = seen_branches.get_mut(&repo.full_name) {
                let new_branches = current_branches
                    .difference(seen_repo_branches)
                    .cloned()
                    .collect::<Vec<_>>();
                for branch_name in &new_branches {
                    seen_repo_branches.insert(branch_name.clone());
                }
                new_branches
            } else {
                seen_branches
                    .insert(repo.full_name.clone(), current_branches);
                Vec::new()
            }
        };

        for (repo_full_name, branch_name, commit) in commits_to_notify {
            self.notify_commit(&repo_full_name, &branch_name, &commit)
                .await;
        }

        for branch_name in new_branches_to_notify {
            if let Some(branch_info) = branches_with_commit_info
                .iter()
                .find(|b| b.name == branch_name)
            {
                match self
                    .client
                    .get_commit(&repo.full_name, &branch_info.commit.sha)
                    .await
                {
                    Ok(commit_details) => {
                        self.notify_branch(&repo.full_name, &branch_name, &commit_details)
                            .await;
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to get commit details for new branch {}: {}",
                            branch_name, e
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn check_pull_requests(&self, repo: &Repo) -> Result<()> {
        let url = format!("https://api.github.com/repos/{}/pulls", repo.full_name);
        let etag = self.etags.lock().await.get(&url).cloned();
        let (prs, new_etag) = self
            .client
            .get_paged::<PullRequest>(&url, etag.as_deref())
            .await?;

        if let Some(new_etag) = new_etag {
            self.etags.lock().await.insert(url, new_etag);
        }

        let mut new_prs_to_notify = Vec::new();
        {
            let mut seen_repo_prs = self.seen_prs.lock().await;
            let seen_repo_prs = seen_repo_prs.entry(repo.full_name.clone()).or_default();
            for pr in prs {
                if !seen_repo_prs.contains(&pr.id) {
                    match self.client.get_user(&pr.user.login).await {
                        Ok(user) => {
                            let mut pr_with_full_user = pr.clone();
                            pr_with_full_user.user = user;
                            new_prs_to_notify.push(pr_with_full_user);
                        }
                        Err(e) => {
                            eprintln!("Failed to get user details for {}: {}", pr.user.login, e);
                            new_prs_to_notify.push(pr.clone());
                        }
                    }
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
        let author_name = pr.user.name.as_deref().unwrap_or(&pr.user.login);
        let body = format!(
            "#{} {}\nBy: {}\nURL: {}",
            pr.id, pr.title, author_name, pr.html_url
        );
        println!("{} - {}", title, body);
        self.send_notification(&title, &body);
    }

    async fn notify_branch(&self, repo_full_name: &str, branch_name: &str, commit: &FullCommit) {
        let title = format!("New Branch in {}", repo_full_name);
        let branch_url = format!("https://github.com/{}/tree/{}", repo_full_name, branch_name);
        let body = format!(
            "Branch: {}\nBy: {}\nURL: {}",
            branch_name, commit.commit.author.name, branch_url
        );
        println!("{} - {}", title, body);
        self.send_notification(&title, &body);
    }

    fn send_notification(&self, title: &str, body: &str) {
        if let Err(e) = Command::new("notify-send").arg(title).arg(body).spawn() {
            eprintln!("Failed to send notification: {}", e);
        }
    }
}
