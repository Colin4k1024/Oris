# RAG 与向量存储：知识检索实战

> 让 Agent 拥有"知识"——从私有文档中检索答案。

## 0. RAG 的价值

```
没有 RAG 的 Agent：
┌─────────────────────────────────────┐
│  User: 我们的退款政策是什么？       │
│  Agent: 我不知道，你的产品文档里应该有 │
└─────────────────────────────────────┘

有 RAG 的 Agent：
┌─────────────────────────────────────┐
│  User: 我们的退款政策是什么？       │
│  Agent: 根据我们的退款政策...        │
│  (检索自: docs/refund-policy.md)   │
└─────────────────────────────────────┘
```

**RAG = 检索增强生成 = 让 Agent 读懂你的文档。**

## 1. Oris RAG 架构

### 1.1 核心组件

```
┌─────────────────────────────────────────────────────────────┐
│                       Oris RAG 架构                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐  │
│  │  Document   │ ──▶ │  Ingestion  │ ──▶ │   Vector    │  │
│  │  Loaders   │     │  Pipeline   │     │   Store     │  │
│  └─────────────┘     └─────────────┘     └──────┬──────┘  │
│                                                  │          │
│  ┌──────────────────────────────────────────────▼──────┐  │
│  │                   Retrieval                         │  │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐    │  │
│  │  │  Semantic │  │  Keyword   │  │   Hybrid  │    │  │
│  │  └────────────┘  └────────────┘  └────────────┘    │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                  │          │
│  ┌──────────────────────────────────────────────▼──────┐  │
│  │                 Generation                         │  │
│  │           (LLM + Retrieved Context)                │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### 1.2 RAG 模式

| 模式 | 说明 |
|------|------|
| **Standard** | 基础语义检索 |
| **Agentic** | Agent 自主决定检索策略 |
| **Hybrid** | 语义 + 关键词混合 |
| **Two-Step** | 先粗排再精排 |

## 2. 文档加载

### 2.1 支持的格式

| 格式 | 支持 | 说明 |
|------|------|------|
| PDF | ✅ | 文本 + 表格 |
| HTML | ✅ | 结构提取 |
| Markdown | ✅ | 保留结构 |
| JSON | ✅ | 结构化 |
| CSV | ✅ | 表格 |
| Git | ✅ | 代码仓库 |
| S3 | ✅ | 云存储 |

### 2.2 加载器

```rust
use oris_runtime::document_loaders::{
    PDFLoader, HTMLLoader, MarkdownLoader, 
    JSONLoader, CSVLoader, GitLoader,
};

let pdf = PDFLoader::new("document.pdf")
    .extract_tables(true)
    .extract_images(false)
    .load()
    .await?;

for page in pdf.pages {
    println!("Page {}: {}", page.number, page.content);
}
```

### 2.3 文档结构

```rust
#[derive(Debug)]
pub struct Document {
    /// 文档 ID
    pub id: String,
    
    /// 页面/章节内容
    pub content: String,
    
    /// 元数据
    pub metadata: DocumentMetadata,
    
    /// 嵌入向量
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Default)]
pub struct DocumentMetadata {
    pub source: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<i64>,
    pub page: Option<u32>,
    pub custom: HashMap<String, Value>,
}
```

## 3. 文本分割

### 3.1 分割策略

```rust
use oris_runtime::text_splitter::{
    TextSplitter, RecursiveCharacterSplitter,
    TokenSplitter, SemanticSplitter,
};

// 递归字符分割（常用）
let splitter = RecursiveCharacterSplitter::new()
    .chunk_size(1000)
    .chunk_overlap(200)
    .separators(vec!["\n\n", "\n", "。", " ", ""])
    .build()?;

// Token 分割（精确控制 token 数）
let splitter = TokenSplitter::new()
    .chunk_size(512)
    .model("cl100k_base")  // tiktoken 编码
    .build()?;
```

### 3.2 语义分割

```rust
// 基于语义的智能分割
let splitter = SemanticSplitter::new()
    .embedder(OpenAIEmbedder::default())
    .threshold(0.5)  // 语义相似度阈值
    .min_chunk_size(200)
    .max_chunk_size(1000)
    .build()?;
```

### 3.3 结构感知分割

```rust
// 保持文档结构
let splitter = RecursiveCharacterSplitter::new()
    .chunk_size(1000)
    // 优先按段落分割
    .separators(vec!["\n\n## ", "\n\n### ", "\n\n", "\n", "。", " ", ""])
    .keep_separators_in_chunk(true)
    .build()?;
```

## 4. 向量存储

### 4.1 Qdrant

```rust
use oris_runtime::vectorstore::qdrant::Qdrant;

let qdrant = Qdrant::builder()
    .url("http://localhost:6334")
    .collection("my_docs")
    .vector_size(1536)
    .distance(Distance::Cosine)
    .on_disk_payload(true)  // 存储在磁盘
    .build()?;

// 添加文档
qdrant.add_documents(docs, Some(embedder.clone())).await?;
```

### 4.2 pgvector

```rust
use oris_runtime::vectorstore::pgvector::PgVector;

let pg = PgVector::builder()
    .pool(pool)
    .table("document_embeddings")
    .vector_column("embedding")
    .distance(Distance::Cosine)
    .build()?;
```

### 4.3 检索

```rust
// 语义检索
let results = qdrant
    .similarity_search(
        "退款政策是什么？",
        5,           // 返回 5 条
        Some(&embedder),
        Some(Filter::new()
            .field_eq("source", "docs/refund.md"))
    )
    .await?;

for doc in results {
    println!("Score: {:.3}", doc.score);
    println!("Content: {}", doc.content);
    println!();
}
```

## 5. 检索策略

### 5.1 语义检索

```rust
use oris_runtime::rag::retriever::SemanticRetriever;

let retriever = SemanticRetriever::new(vector_store)
    .embedder(embedder)
    .top_k(5)
    .min_score(0.7)
    .build()?;
```

### 5.2 关键词检索

```rust
use oris_runtime::rag::retriever::KeywordRetriever;

let retriever = KeywordRetriever::new(vector_store)
    .fields(vec!["content", "title"])
    .algorithm(BM25)
    .top_k(5)
    .build()?;
```

### 5.3 混合检索

```rust
use oris_runtime::rag::retriever::HybridRetriever;

let retriever = HybridRetriever::builder()
    .semantic(SemanticRetriever::new(vs.clone()))
    .keyword(KeywordRetriever::new(vs.clone()))
    .weights(vec![0.7, 0.3])  // 语义 70%, 关键词 30%
    .fusion(RRF)  // Reciprocal Rank Fusion
    .build()?;
```

### 5.4 两阶段检索

```rust
use oris_runtime::rag::retriever::TwoStageRetriever;

let retriever = TwoStageRetriever::builder()
    .粗排(EmbeddingRetriever::new(vs.clone()).top_k(50))
    .精排(CrossEncoderReranker::new(cross_encoder).top_k(10))
    .build()?;
```

## 6. RAG 管道

### 6.1 标准 RAG

```rust
use oris_runtime::rag::{RAGPipeline, Config};

let rag = RAGPipeline::builder()
    .retriever(semantic_retriever)
    .llm(OpenAI::gpt4())
    .prompt_template(r#"Based on the following context, answer the question.

Context:
{ context }

Question: { question }

Answer:"#)
    .build()?;

// 执行
let answer = rag.answer("退款政策是什么？").await?;
```

### 6.2 Agentic RAG

```rust
use oris_runtime::rag::agentic::AgenticRAG;

let rag = AgenticRAG::builder()
    .llm(OpenAI::gpt4())
    .retriever(semantic_retriever)
    // Agent 决定何时检索、检索什么
    .build()?;

// Agent 会：
// 1. 分析问题
// 2. 决定是否需要检索
// 3. 制定检索策略
// 4. 执行检索
// 5. 评估结果
// 6. 生成答案
```

### 6.3 可配置 RAG

```rust
let rag = RAGPipeline::builder()
    .config(RAGConfig::default()
        .max_context_tokens(4000)
        .citation_enabled(true)
        .rerank_enabled(true)
        .hyde_enabled(true)  // Hypothetical Document Embeddings
    )
    .build()?;
```

## 7. 实战示例

### 7.1 加载文档并索引

```rust
use oris_runtime::document_loaders::PDFLoader;
use oris_runtime::text_splitter::RecursiveCharacterSplitter;
use oris_runtime::vectorstore::qdrant::Qdrant;
use oris_runtime::embedding::openai::OpenAIEmbedder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 加载 PDF
    let loader = PDFLoader::new("docs/产品手册.pdf");
    let docs = loader.load().await?;
    
    // 2. 分割
    let splitter = RecursiveCharacterSplitter::new()
        .chunk_size(1000)
        .chunk_overlap(200)
        .build()?;
    let chunks = splitter.split_documents(&docs)?;
    
    // 3. 嵌入
    let embedder = OpenAIEmbedder::default();
    let vectors = embedder.embed_documents(
        &chunks.iter().map(|c| c.content.clone()).collect::<Vec<_>>()
    ).await?;
    
    // 4. 存入向量数据库
    let qdrant = Qdrant::new("localhost:6334", "product_docs")?;
    qdrant.add_vectors(chunks, vectors).await?;
    
    println!("Indexed {} chunks", chunks.len());
    
    Ok(())
}
```

### 7.2 问答

```rust
async fn answer_question(
    question: &str,
    qdrant: &Qdrant,
    embedder: &OpenAIEmbedder,
    llm: &OpenAI,
) -> Result<String, Box<dyn std::error::Error>> {
    // 1. 检索
    let results = qdrant.similarity_search(question, 5, Some(embedder), None).await?;
    
    // 2. 构建上下文
    let context: String = results.iter()
        .map(|r| r.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    
    // 3. 生成答案
    let prompt = format!(
        "Based on the following context, answer the question.\n\nContext:\n{}\n\nQuestion: {}\n\nAnswer:",
        context, question
    );
    
    let answer = llm.invoke(&prompt).await?;
    
    Ok(answer)
}
```

## 8. 优化技巧

### 8.1 HyDE（Hypothetical Document Embeddings）

```rust
let rag = RAGPipeline::builder()
    .hyde_enabled(true)
    .hyde_llm(llm.clone())
    .build()?;
```

### 8.2 查询扩展

```rust
let rag = RAGPipeline::builder()
    .query_expansion(true)
    .expansion_llm(llm.clone())
    .build()?;
```

### 8.3 重新排序

```rust
use oris_runtime::rag::reranker::CrossEncoderReranker;

let reranker = CrossEncoderReranker::new("cross-encoder/ms-marco-MiniLM-L-6-v2");

let results = reranker.rerank(query, initial_results).await?;
```

### 8.4 缓存

```rust
let rag = RAGPipeline::builder()
    .cache_enabled(true)
    .cache_ttl(Duration::from_hours(24))
    .build()?;
```

## 9. 与 LangChain 对比

| 特性 | Oris RAG | LangChain RAG |
|------|----------|---------------|
| 文档加载 | 多格式 | 多格式 |
| 分割策略 | 多种 | 多种 |
| 向量存储 | Qdrant, pgvector, Chroma, Milvus | 多种 |
| 检索策略 | 语义/关键词/混合/两阶段 | 有限 |
| Agentic RAG | ✅ | 有限 |
| Rust 性能 | 高 | 中 |

## 10. 小结

Oris 的 RAG 系统：

1. **文档加载** — PDF、HTML、Markdown、JSON、CSV、Git、S3
2. **文本分割** — 递归、Token、语义、结构感知
3. **向量存储** — Qdrant、pgvector、Chroma、Milvus
4. **检索策略** — 语义、关键词、混合、两阶段
5. **RAG 管道** — 标准、Agentic、可配置
6. **优化** — HyDE、查询扩展、重排序、缓存

**RAG = 给 Agent 配备了一个"知识库"——让它能回答关于你文档的问题。**

---

*下篇预告：自进化系统 (Evolution)——Gene、Capsule、Pipeline*
