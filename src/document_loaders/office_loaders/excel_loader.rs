use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
    pin::Pin,
};

use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use serde_json::Value;

use crate::{
    document_loaders::{process_doc_stream, Loader, LoaderError},
    schemas::Document,
    text_splitter::TextSplitter,
};

/// Excel loader that loads Excel files (.xlsx, .xls)
#[derive(Debug, Clone)]
pub struct ExcelLoader<R> {
    reader: R,
    /// Optional sheet name to load (if None, loads all sheets)
    sheet_name: Option<String>,
}

impl<R: Read> ExcelLoader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            sheet_name: None,
        }
    }

    /// Load a specific sheet by name
    pub fn with_sheet<S: Into<String>>(mut self, sheet_name: S) -> Self {
        self.sheet_name = Some(sheet_name.into());
        self
    }
}

impl ExcelLoader<Cursor<Vec<u8>>> {
    pub fn from_string(input: Vec<u8>) -> Self {
        let reader = Cursor::new(input);
        Self::new(reader)
    }
}

impl ExcelLoader<BufReader<File>> {
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, LoaderError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(Self::new(reader))
    }
}

#[async_trait]
impl<R: Read + Send + Sync + 'static> Loader for ExcelLoader<R> {
    async fn load(
        mut self,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<Document, LoaderError>> + Send + 'static>>,
        LoaderError,
    > {
        let mut buffer = Vec::new();
        self.reader.read_to_end(&mut buffer)?;

        let sheet_name = self.sheet_name.clone();

        let stream = stream! {
            #[cfg(feature = "excel")]
            {
                use calamine::{open_workbook_auto, Reader};


                // calamine::open_workbook_auto requires a path; write buffer to a temp file
                let tmp_path = std::env::temp_dir().join(format!(
                    "excel_loader_{}.xlsx",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                ));
                if let Err(e) = std::fs::write(&tmp_path, &buffer) {
                    yield Err(LoaderError::ExcelError(format!("Failed to write temp file: {}", e)));
                    return;
                }
                let mut workbook = match open_workbook_auto(&tmp_path) {
                    Ok(wb) => wb,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp_path);
                        yield Err(LoaderError::ExcelError(format!("Failed to open workbook: {}", e)));
                        return;
                    }
                };
                let _ = std::fs::remove_file(&tmp_path);

                let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

                for sheet_name_iter in sheet_names {
                    // Filter by sheet name if specified
                    if let Some(ref name) = sheet_name {
                        if &sheet_name_iter != name {
                            continue;
                        }
                    }

                    if let Ok(range) = workbook.worksheet_range(&sheet_name_iter) {
                        let mut content = String::new();
                        let mut row_num = 0;

                        for row in range.rows() {
                            row_num += 1;
                            let mut row_content = Vec::new();

                            for cell in row {
                                let cell_value = match cell {
                                    calamine::Data::Empty => String::new(),
                                    calamine::Data::String(s) => s.clone(),
                                    calamine::Data::Float(f) => f.to_string(),
                                    calamine::Data::Int(i) => i.to_string(),
                                    calamine::Data::Bool(b) => b.to_string(),
                                    calamine::Data::Error(e) => format!("ERROR: {:?}", e),
                                    calamine::Data::DateTime(dt) => format!("{}", dt),
                                    _ => String::new(), // Duration or other variants
                                };
                                row_content.push(cell_value);
                            }

                            if !row_content.is_empty() {
                                content.push_str(&row_content.join("\t"));
                                content.push('\n');
                            }
                        }

                        if !content.trim().is_empty() {
                            let mut metadata = HashMap::new();
                            metadata.insert("source_type".to_string(), Value::from("excel"));
                            metadata.insert("sheet_name".to_string(), Value::from(sheet_name_iter.clone()));
                            metadata.insert("row_count".to_string(), Value::from(row_num));

                            let doc = Document::new(content).with_metadata(metadata);
                            yield Ok(doc);
                        }
                    }
                }
            }
            #[cfg(not(feature = "excel"))]
            {
                yield Err(LoaderError::OtherError("Excel feature not enabled. Add 'excel' feature to use ExcelLoader.".to_string()));
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
#[cfg(feature = "excel")]
mod tests {
    use futures_util::StreamExt;

    use super::*;

    #[tokio::test]
    #[ignore] // Requires Excel file
    async fn test_excel_loader() {
        // This test would require an actual Excel file
        // For now, we'll just verify the loader compiles
    }
}
