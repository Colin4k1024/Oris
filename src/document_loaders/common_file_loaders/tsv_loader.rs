use crate::document_loaders::{process_doc_stream, LoaderError};
use crate::{document_loaders::Loader, schemas::Document, text_splitter::TextSplitter};
use async_stream::stream;
use async_trait::async_trait;
use csv;
use futures::Stream;
use serde_json::Value;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::Path;
use std::pin::Pin;

/// TSV (Tab-Separated Values) loader
/// Similar to CsvLoader but uses tab as delimiter
#[derive(Debug, Clone)]
pub struct TsvLoader<R> {
    reader: R,
    columns: Option<Vec<String>>,
}

impl<R: Read> TsvLoader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            columns: None,
        }
    }

    /// Filter specific columns (if None, all columns are included)
    pub fn with_columns(mut self, columns: Vec<String>) -> Self {
        self.columns = Some(columns);
        self
    }
}

impl TsvLoader<Cursor<Vec<u8>>> {
    pub fn from_string<S: Into<String>>(input: S) -> Self {
        let input = input.into();
        let reader = Cursor::new(input.into_bytes());
        Self::new(reader)
    }
}

impl TsvLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for TsvLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .from_reader(self.reader);

        let headers = reader.headers()?.clone();
        let columns = self.columns.clone();

        let mut row_number: i64 = 0;

        let stream = stream! {
            for result in reader.records() {
                let record = result?;
                let mut content = String::new();

                for (i, field) in record.iter().enumerate() {
                    let header = &headers[i];

                    if let Some(ref cols) = columns {
                        if !cols.contains(&header.to_string()) {
                            continue;
                        }
                    }

                    let line = format!("{}: {}", header, field);
                    content.push_str(&line);
                    content.push('\n');
                }

                row_number += 1;

                let mut document = Document::new(content);
                let mut metadata = HashMap::new();
                metadata.insert("row".to_string(), Value::from(row_number));
                metadata.insert("source_type".to_string(), Value::from("tsv"));

                document.metadata = metadata;

                yield Ok(document);
            }
        };

        Ok(Box::pin(stream))
    }

    async fn load_and_split<TS: TextSplitter + 'static>(
        mut self,
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
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    async fn test_tsv_loader() {
        let input = "name\tage\tcity\nJohn\t30\tNew York\nJane\t25\tLondon";
        let loader = TsvLoader::from_string(input);

        let documents = loader
            .load()
            .await
            .unwrap()
            .map(|x| x.unwrap())
            .collect::<Vec<_>>()
            .await;

        assert_eq!(documents.len(), 2);
        assert!(documents[0].page_content.contains("John"));
        assert_eq!(documents[0].metadata.get("row").unwrap(), &Value::from(1));
    }
}
