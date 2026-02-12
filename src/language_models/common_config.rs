//! LLM 客户端通用配置和接口
//!
//! 提供所有 LLM 客户端共享的配置模式和辅助函数。

use crate::language_models::{llm::LLM, options::CallOptions};

/// LLM 客户端通用配置 trait
///
/// 为所有 LLM 客户端提供统一的配置接口。
pub trait LLMConfig: Send + Sync {
    /// 获取模型名称
    fn model(&self) -> &str;

    /// 获取调用选项
    fn options(&self) -> &CallOptions;

    /// 设置模型
    fn set_model(&mut self, model: String);

    /// 设置调用选项
    fn set_options(&mut self, options: CallOptions);
}

/// LLM 客户端构建器 trait
///
/// 为所有 LLM 客户端提供统一的构建器模式。
///
/// 注意：这是一个标记 trait，具体的实现由各个 LLM 客户端提供。
/// 这个 trait 主要用于文档和类型约束。
pub trait LLMBuilder: Sized {
    /// 创建新的客户端实例
    fn new() -> Self;

    /// 设置模型
    fn with_model<S: Into<String>>(self, model: S) -> Self;

    /// 设置调用选项
    fn with_options(self, options: CallOptions) -> Self;
}

/// LLM 客户端辅助函数
pub struct LLMHelpers;

impl LLMHelpers {
    /// 验证模型名称格式
    ///
    /// 检查模型名称是否符合基本格式要求。
    pub fn validate_model_name(model: &str) -> Result<(), String> {
        if model.is_empty() {
            return Err("Model name cannot be empty".to_string());
        }
        if model.len() > 256 {
            return Err("Model name too long (max 256 characters)".to_string());
        }
        Ok(())
    }

    /// 从环境变量获取 API key
    ///
    /// # 参数
    /// - `env_var`: 环境变量名称
    /// - `default`: 如果环境变量不存在时的默认值
    ///
    /// # 返回
    /// API key 字符串
    pub fn get_api_key_from_env(env_var: &str, default: &str) -> String {
        std::env::var(env_var).unwrap_or_else(|_| default.to_string())
    }

    /// 合并调用选项
    ///
    /// 将两个 CallOptions 合并，第二个选项的字段会覆盖第一个。
    pub fn merge_options(_base: CallOptions, override_opts: CallOptions) -> CallOptions {
        // 注意：这需要根据 CallOptions 的实际结构来实现
        // 目前返回 override_opts，实际实现应该合并字段
        override_opts
    }

    /// 创建默认的调用选项
    pub fn default_options() -> CallOptions {
        CallOptions::default()
    }
}

/// LLM 客户端初始化配置
///
/// 包含所有 LLM 客户端共享的初始化参数。
#[derive(Debug, Clone)]
pub struct LLMInitConfig {
    /// 模型名称
    pub model: Option<String>,
    /// API key（如果适用）
    pub api_key: Option<String>,
    /// Base URL（如果适用）
    pub base_url: Option<String>,
    /// 调用选项
    pub options: Option<CallOptions>,
}

impl LLMInitConfig {
    /// 创建新的配置
    pub fn new() -> Self {
        Self {
            model: None,
            api_key: None,
            base_url: None,
            options: None,
        }
    }

    /// 设置模型
    pub fn with_model<S: Into<String>>(mut self, model: S) -> Self {
        self.model = Some(model.into());
        self
    }

    /// 设置 API key
    pub fn with_api_key<S: Into<String>>(mut self, api_key: S) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// 设置 base URL
    pub fn with_base_url<S: Into<String>>(mut self, base_url: S) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// 设置调用选项
    pub fn with_options(mut self, options: CallOptions) -> Self {
        self.options = Some(options);
        self
    }
}

impl Default for LLMInitConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// 流式响应处理 trait
///
/// 为所有支持流式响应的 LLM 客户端提供统一接口。
pub trait StreamingLLM: LLM {
    /// 检查是否支持流式响应
    fn supports_streaming(&self) -> bool {
        true // 大多数现代 LLM 都支持流式响应
    }

    /// 获取流式响应的默认配置
    fn default_streaming_config(&self) -> CallOptions {
        CallOptions::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_model_name() {
        assert!(LLMHelpers::validate_model_name("gpt-4").is_ok());
        assert!(LLMHelpers::validate_model_name("").is_err());
    }

    #[test]
    fn test_llm_init_config() {
        let config = LLMInitConfig::new()
            .with_model("gpt-4")
            .with_api_key("test-key");

        assert_eq!(config.model, Some("gpt-4".to_string()));
        assert_eq!(config.api_key, Some("test-key".to_string()));
    }
}
