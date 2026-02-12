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

/// Configuration for AWS S3 loader
#[derive(Debug, Clone)]
pub struct AwsS3Config {
    pub bucket: String,
    pub region: Option<String>,
    pub prefix: Option<String>,
    pub recursive: bool,
}

impl AwsS3Config {
    pub fn new(bucket: String) -> Self {
        Self {
            bucket,
            region: None,
            prefix: None,
            recursive: true,
        }
    }

    pub fn with_region<S: Into<String>>(mut self, region: S) -> Self {
        self.region = Some(region.into());
        self
    }

    pub fn with_prefix<S: Into<String>>(mut self, prefix: S) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    pub fn with_recursive(mut self, recursive: bool) -> Self {
        self.recursive = recursive;
        self
    }
}

/// AWS S3 loader that loads files from S3 bucket
#[derive(Debug, Clone)]
pub struct AwsS3Loader {
    config: AwsS3Config,
}

impl AwsS3Loader {
    pub fn new(config: AwsS3Config) -> Self {
        Self { config }
    }

    pub fn from_bucket<S: Into<String>>(bucket: S) -> Self {
        Self::new(AwsS3Config::new(bucket.into()))
    }
}

#[async_trait]
impl Loader for AwsS3Loader {
    async fn load(
        self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let config = self.config.clone();

        let stream = stream! {
            #[cfg(feature = "aws-s3")]
            {
                use aws_sdk_s3::Client as S3Client;
                use aws_config::{BehaviorVersion, Region};

                // Build AWS config
                let aws_config = if let Some(region_str) = &config.region {
                    aws_config::defaults(BehaviorVersion::latest())
                        .region(Region::new(region_str.clone()))
                        .load()
                        .await
                } else {
                    aws_config::defaults(BehaviorVersion::latest())
                        .load()
                        .await
                };

                // Build S3 client
                let client = S3Client::new(&aws_config);

                // List objects
                let mut list_request = client
                    .list_objects_v2()
                    .bucket(&config.bucket);

                if let Some(ref prefix) = config.prefix {
                    list_request = list_request.prefix(prefix);
                }

                let mut continuation_token = None;

                loop {
                    let mut request = list_request.clone();
                    if let Some(token) = continuation_token {
                        request = request.continuation_token(token);
                    }

                    let response = request
                        .send()
                        .await
                        .map_err(|e| LoaderError::OtherError(format!("S3 list error: {}", e)))?;

                    let contents = response.contents();
                    for object in contents {
                        if let Some(key) = object.key() {
                            // Skip if not recursive and has subdirectories
                            if !config.recursive && key.contains('/') && key != config.prefix.as_deref().unwrap_or("") {
                                continue;
                            }

                            // Get object
                            let get_response = client
                                .get_object()
                                .bucket(&config.bucket)
                                .key(key)
                                .send()
                                .await
                                .map_err(|e| LoaderError::OtherError(format!("S3 get error for {}: {}", key, e)))?;

                            let body_bytes = get_response
                                .body
                                .collect()
                                .await
                                .map_err(|e| LoaderError::OtherError(format!("Failed to collect S3 object body: {}", e)))?;

                            let content = String::from_utf8(body_bytes.into_bytes().to_vec())
                                .map_err(|e| LoaderError::FromUtf8Error(e))?;

                            let mut metadata = HashMap::new();
                            metadata.insert("source_type".to_string(), Value::from("s3"));
                            metadata.insert("bucket".to_string(), Value::from(config.bucket.clone()));
                            metadata.insert("key".to_string(), Value::from(key));

                            let doc = Document::new(content).with_metadata(metadata);
                            yield Ok(doc);
                        }
                    }

                    if response.is_truncated().unwrap_or(false) {
                        continuation_token = response.next_continuation_token().map(|s| s.to_string());
                    } else {
                        break;
                    }
                }
            }
            #[cfg(not(feature = "aws-s3"))]
            {
                yield Err(LoaderError::OtherError("AWS S3 feature not enabled. Add 'aws-s3' feature to use AwsS3Loader.".to_string()));
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
#[cfg(feature = "aws-s3")]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    #[ignore] // Requires AWS credentials
    async fn test_aws_s3_loader() {
        let config = AwsS3Config::new("test-bucket".to_string()).with_prefix("documents/");
        let loader = AwsS3Loader::new(config);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        // Results depend on S3 bucket content
    }
}
