use crate::models::{FullCommit, User};
use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, ETAG, IF_NONE_MATCH, USER_AGENT};
use std::sync::Arc;

#[derive(Clone)]
pub struct GithubClient {
    client: Arc<reqwest::Client>,
}

impl GithubClient {
    pub fn new(token: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("github-commit-notifier"));
        headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github.v3+json"));
        headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("token {}", token))?);

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;
        Ok(Self { client: Arc::new(client) })
    }

    pub async fn get_paged<T: serde::de::DeserializeOwned>(
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

    pub async fn get_commit(&self, repo_full_name: &str, sha: &str) -> Result<FullCommit> {
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

    pub async fn get_user(&self, username: &str) -> Result<User> {
        let url = format!("https://api.github.com/users/{}", username);
        let response = self.client.get(&url).send().await?;
        if response.status().is_success() {
            let user = response.json().await?;
            Ok(user)
        } else {
            Err(anyhow!(
                "Failed to fetch user {}: {}",
                username,
                response.status()
            ))
        }
    }
}