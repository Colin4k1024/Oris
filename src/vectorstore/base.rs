//! VectorStore 基础抽象层
//!
//! 提供 VectorStore 实现的通用功能和辅助方法。

use std::sync::Arc;

use crate::embedding::embedder_trait::Embedder;
use crate::schemas::Document;
use crate::vectorstore::{VecStoreOptions, VectorStore, VectorStoreError};

/// VectorStore 基础配置
///
/// 包含所有 VectorStore 实现共享的配置项。
#[derive(Clone)]
pub struct VectorStoreBaseConfig {
    /// Embedder 用于生成向量
    pub embedder: Arc<dyn Embedder>,
    /// Collection/Table 名称
    pub collection_name: String,
    /// 向量维度（如果已知）
    pub vector_dimensions: Option<usize>,
}

impl std::fmt::Debug for VectorStoreBaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VectorStoreBaseConfig")
            .field("embedder", &"<dyn Embedder>")
            .field("collection_name", &self.collection_name)
            .field("vector_dimensions", &self.vector_dimensions)
            .finish()
    }
}

impl VectorStoreBaseConfig {
    /// 创建新的配置
    pub fn new(embedder: Arc<dyn Embedder>, collection_name: String) -> Self {
        Self {
            embedder,
            collection_name,
            vector_dimensions: None,
        }
    }

    /// 设置向量维度
    pub fn with_vector_dimensions(mut self, dimensions: usize) -> Self {
        self.vector_dimensions = Some(dimensions);
        self
    }

    /// 获取或计算向量维度
    pub async fn get_vector_dimensions(&self) -> Result<usize, VectorStoreError> {
        if let Some(dims) = self.vector_dimensions {
            Ok(dims)
        } else {
            // 通过 embedding 一个测试文本来获取维度
            let test_embedding =
                self.embedder.embed_query("test").await.map_err(|e| {
                    VectorStoreError::InternalError(format!("Embedding error: {}", e))
                })?;
            Ok(test_embedding.len())
        }
    }
}

/// VectorStore 辅助函数
pub struct VectorStoreHelpers;

impl VectorStoreHelpers {
    /// 从文档中提取文本内容
    pub fn extract_texts(docs: &[Document]) -> Vec<String> {
        docs.iter().map(|d| d.page_content.clone()).collect()
    }

    /// 验证文档和向量的数量匹配
    pub fn validate_documents_vectors(
        docs: &[Document],
        vectors: &[Vec<f64>],
    ) -> Result<(), VectorStoreError> {
        if docs.len() != vectors.len() {
            return Err(VectorStoreError::InternalError(format!(
                "Number of documents ({}) and vectors ({}) do not match",
                docs.len(),
                vectors.len()
            )));
        }
        Ok(())
    }

    /// 从选项或配置中获取 embedder
    pub fn get_embedder<F>(
        opt: &VecStoreOptions<F>,
        default: &Arc<dyn Embedder>,
    ) -> Arc<dyn Embedder> {
        opt.embedder.as_ref().unwrap_or(default).clone()
    }

    /// 应用分数阈值过滤
    pub fn apply_score_threshold(mut docs: Vec<Document>, threshold: Option<f32>) -> Vec<Document> {
        if let Some(threshold) = threshold {
            docs.retain(|doc| doc.score >= threshold as f64);
        }
        docs
    }

    /// 按分数排序文档（降序）
    pub fn sort_by_score(mut docs: Vec<Document>) -> Vec<Document> {
        docs.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        docs
    }
}

/// VectorStore 初始化 trait
///
/// 为需要初始化的 VectorStore 实现提供统一接口。
#[async_trait::async_trait]
pub trait VectorStoreInitializable: VectorStore {
    /// 初始化 VectorStore（创建表、集合等）
    async fn initialize(&self) -> Result<(), VectorStoreError>;
}

/// VectorStore 批量操作 trait
///
/// 为支持批量操作的 VectorStore 提供优化接口。
#[async_trait::async_trait]
pub trait VectorStoreBatch: VectorStore {
    /// 批量添加文档（可能比逐个添加更高效）
    async fn add_documents_batch(
        &self,
        docs: &[Document],
        batch_size: usize,
        opt: &Self::Options,
    ) -> Result<Vec<String>, VectorStoreError> {
        // 默认实现：分批调用 add_documents
        let mut all_ids = Vec::new();
        for chunk in docs.chunks(batch_size) {
            let ids = self.add_documents(chunk, opt).await?;
            all_ids.extend(ids);
        }
        Ok(all_ids)
    }

    /// 批量删除文档
    async fn delete_batch(
        &self,
        ids: &[String],
        batch_size: usize,
        opt: &Self::Options,
    ) -> Result<(), VectorStoreError> {
        // 默认实现：分批调用 delete
        for chunk in ids.chunks(batch_size) {
            self.delete(chunk, opt).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_texts() {
        let docs = vec![Document::new("text1"), Document::new("text2")];
        let texts = VectorStoreHelpers::extract_texts(&docs);
        assert_eq!(texts, vec!["text1", "text2"]);
    }

    #[test]
    fn test_validate_documents_vectors() {
        let docs = vec![Document::new("text1"), Document::new("text2")];
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        assert!(VectorStoreHelpers::validate_documents_vectors(&docs, &vectors).is_ok());
    }

    #[test]
    fn test_validate_documents_vectors_mismatch() {
        let docs = vec![Document::new("text1")];
        let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];
        assert!(VectorStoreHelpers::validate_documents_vectors(&docs, &vectors).is_err());
    }

    #[test]
    fn test_apply_score_threshold() {
        let mut docs = vec![
            Document {
                page_content: "text1".to_string(),
                metadata: Default::default(),
                score: Some(0.8),
            },
            Document {
                page_content: "text2".to_string(),
                metadata: Default::default(),
                score: Some(0.3),
            },
        ];
        let filtered = VectorStoreHelpers::apply_score_threshold(docs, Some(0.5));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].score, Some(0.8));
    }

    #[test]
    fn test_sort_by_score() {
        let docs = vec![
            Document {
                page_content: "text1".to_string(),
                metadata: Default::default(),
                score: Some(0.3),
            },
            Document {
                page_content: "text2".to_string(),
                metadata: Default::default(),
                score: Some(0.8),
            },
        ];
        let sorted = VectorStoreHelpers::sort_by_score(docs);
        assert_eq!(sorted[0].score, Some(0.8));
        assert_eq!(sorted[1].score, Some(0.3));
    }
}
