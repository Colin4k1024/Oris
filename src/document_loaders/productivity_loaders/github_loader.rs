use std::{collections::HashMap, pin::Pin};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use serde_json::Value;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// Configuration for GitHub loader
#[derive(Debug, Clone)]
pub struct GitHubConfig {
    pub owner: String,
    pub repo: String,
    pub path: Option<String>,
    pub branch: Option<String>,
    pub token: Option<String>,
}

impl GitHubConfig {
    pub fn new(owner: String, repo: String) -> Self {
        Self {
            owner,
            repo,
            path: None,
            branch: None,
            token: None,
        }
    }

    pub fn with_path<S: Into<String>>(mut self, path: S) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_branch<S: Into<String>>(mut self, branch: S) -> Self {
        self.branch = Some(branch.into());
        self
    }

    pub fn with_token<S: Into<String>>(mut self, token: S) -> Self {
        self.token = Some(token.into());
        self
    }
}

/// GitHub loader that loads files from GitHub repositories
#[derive(Debug, Clone)]
pub struct GitHubLoader {
    config: GitHubConfig,
}

impl GitHubLoader {
    pub fn new(config: GitHubConfig) -> Self {
        Self { config }
    }

    pub fn from_repo(owner: impl Into<String>, repo: impl Into<String>) -> Self {
        Self::new(GitHubConfig::new(owner.into(), repo.into()))
    }
}

#[async_trait]
impl Loader for GitHubLoader {
    async fn load(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let config = self.config.clone();

        let stream = stream! {
            #[cfg(feature = "github")]
            {
                use octocrab::Octocrab;

                // Build GitHub client
                let mut builder = Octocrab::builder();
                if let Some(token) = &config.token {
                    builder = builder.personal_token(token.clone());
                }
                let octocrab = builder.build()
                    .map_err(|e| LoaderError::OtherError(format!("Failed to create GitHub client: {}", e)))?;

                let ref_name = config.branch.as_deref().unwrap_or("main");

                // Get repository contents
                let path = config.path.as_deref().unwrap_or("");
                let contents = octocrab
                    .repos(&config.owner, &config.repo)
                    .get_content()
                    .path(path)
                    .r#ref(ref_name)
                    .send()
                    .await
                    .map_err(|e| LoaderError::OtherError(format!("GitHub API error: {}", e)))?;

                for item in contents.items {
                    let item_type = item.r#type.as_str();
                    if item_type == "file" {
                        if let Some(content_b64) = &item.content {
                            let decoded = {
                                use base64::Engine;
                                base64::engine::general_purpose::STANDARD
                                    .decode(content_b64.replace('\n', ""))
                                    .map_err(|e| LoaderError::OtherError(format!("Base64 decode error: {}", e)))?
                            };

                            let content_str = String::from_utf8(decoded)
                                .map_err(|e| LoaderError::FromUtf8Error(e))?;

                            let mut metadata = HashMap::new();
                            metadata.insert("source_type".to_string(), Value::from("github"));
                            metadata.insert("owner".to_string(), Value::from(config.owner.clone()));
                            metadata.insert("repo".to_string(), Value::from(config.repo.clone()));
                            metadata.insert("path".to_string(), Value::from(item.path.clone()));
                            metadata.insert("name".to_string(), Value::from(item.name.clone()));
                            metadata.insert("branch".to_string(), Value::from(ref_name));

                            let doc = Document::new(content_str).with_metadata(metadata);
                            yield Ok(doc);
                        }
                    }
                    // Dir and other types: skip (would require additional API calls for recursion)
                }
            }
            #[cfg(not(feature = "github"))]
            {
                yield Err(LoaderError::OtherError("GitHub feature not enabled. Add 'github' feature to use GitHubLoader.".to_string()));
            }
        };

        Ok(Box::pin(stream))
    }

    async fn load_and_split<TS: TextSplitter + 'static>(
        self,
        splitter: TS,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let doc_stream = self.load().await?;
        let stream = process_doc_stream(doc_stream, splitter).await;
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
#[cfg(feature = "github")]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    #[ignore] // Requires GitHub token
    async fn test_github_loader() {
        let loader = GitHubLoader::from_repo("octocat", "Hello-World").with_path("README.md");

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        // Results depend on repository content
    }
}
