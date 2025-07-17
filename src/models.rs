use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Repo {
    pub full_name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Branch {
    pub name: String,
    pub commit: Commit,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Commit {
    pub sha: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct FullCommit {
    pub html_url: String,
    pub commit: CommitDetails,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CommitDetails {
    pub message: String,
    pub author: CommitAuthor,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CommitAuthor {
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct PullRequest {
    pub id: u64,
    pub html_url: String,
    pub title: String,
    pub user: User,
}

#[derive(Deserialize, Debug, Clone)]
pub struct User {
    pub login: String,
    pub name: Option<String>,
}
