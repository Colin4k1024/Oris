//! Builder 模式抽象和工具
//!
//! 提供通用的 Builder trait 和宏，减少重复代码。

/// Builder trait 定义
///
/// 为所有 Builder 类型提供统一的接口。
pub trait Builder<T> {
    /// 构建最终对象
    fn build(self) -> Result<T, Box<dyn std::error::Error>>;
}

/// Builder trait with validation
///
/// 支持验证的 Builder trait。
pub trait ValidatedBuilder<T> {
    /// 验证构建参数
    fn validate(&self) -> Result<(), String>;

    /// 构建最终对象（带验证）
    fn build(self) -> Result<T, Box<dyn std::error::Error>>
    where
        Self: Sized,
    {
        self.validate()?;
        self.build_unchecked()
    }

    /// 构建最终对象（不验证）
    fn build_unchecked(self) -> Result<T, Box<dyn std::error::Error>>;
}

/// 宏：创建简单的 Builder 结构
///
/// # 示例
///
/// ```rust,ignore
/// use oris::utils::simple_builder;
///
/// simple_builder! {
///     pub struct MyBuilder {
///         field1: Option<String>,
///         field2: Option<i32>,
///     }
///     impl {
///         pub fn with_field1(mut self, value: String) -> Self {
///             self.field1 = Some(value);
///             self
///         }
///         pub fn with_field2(mut self, value: i32) -> Self {
///             self.field2 = Some(value);
///             self
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! simple_builder {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $($field:ident: $type:ty),* $(,)?
        }
        impl {
            $($method:item)*
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($field: $type),*
        }

        impl $name {
            pub fn new() -> Self {
                Self {
                    $($field: None),*
                }
            }

            $($method)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStruct {
        value: String,
    }

    struct TestBuilder {
        value: Option<String>,
    }

    impl TestBuilder {
        fn new() -> Self {
            Self { value: None }
        }

        fn with_value(mut self, value: String) -> Self {
            self.value = Some(value);
            self
        }
    }

    impl Builder<TestStruct> for TestBuilder {
        fn build(self) -> Result<TestStruct, Box<dyn std::error::Error>> {
            Ok(TestStruct {
                value: self.value.unwrap_or_else(|| "default".to_string()),
            })
        }
    }

    #[test]
    fn test_builder() {
        let builder = TestBuilder::new().with_value("test".to_string());
        let result = builder.build().unwrap();
        assert_eq!(result.value, "test");
    }
}
