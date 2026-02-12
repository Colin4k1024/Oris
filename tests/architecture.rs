//! Architecture tests
//!
//! Tests to verify architectural patterns and module organization.

use oris::error::{error_info, ChainError, ErrorCode, LangChainError};

#[test]
fn test_error_unification() {
    // Test that all error types can be converted to LangChainError
    let chain_error = ChainError::OtherError("test".to_string());
    let langchain_error: LangChainError = chain_error.into();

    match langchain_error {
        LangChainError::ChainError(_) => {}
        _ => panic!("Expected ChainError variant"),
    }
}

#[test]
fn test_error_code_system() {
    let error = LangChainError::ConfigurationError("test".to_string());
    let code = ErrorCode::from_error(&error);
    assert_eq!(code, ErrorCode::ConfigurationError);
    assert_eq!(code.as_u32(), 9000);
}

#[test]
fn test_error_info() {
    let error = LangChainError::ConfigurationError("test config".to_string());
    let info = error_info(&error);
    assert!(info.contains("E9000"));
    assert!(info.contains("test config"));
}

#[test]
fn test_utils_similarity() {
    use oris::utils::{cosine_similarity_f64, text_similarity};

    // Test cosine similarity
    let vec1 = vec![1.0, 0.0];
    let vec2 = vec![1.0, 0.0];
    let similarity = cosine_similarity_f64(&vec1, &vec2);
    assert!((similarity - 1.0).abs() < 1e-10);

    // Test text similarity
    let text1 = "hello world";
    let text2 = "world hello";
    let text_sim = text_similarity(text1, text2);
    assert!((text_sim - 1.0).abs() < 1e-10);
}

#[test]
fn test_utils_vectors() {
    use oris::utils::{mean_embedding_f64, sum_vectors_f64};

    let vectors = vec![vec![1.0, 2.0], vec![3.0, 4.0]];

    let mean = mean_embedding_f64(&vectors);
    assert_eq!(mean, vec![2.0, 3.0]);

    let sum = sum_vectors_f64(&vectors);
    assert_eq!(sum, vec![4.0, 6.0]);
}

#[test]
fn test_type_aliases() {
    use oris::{Messages, Tools};

    // Verify type aliases are accessible
    let _tools: Tools = vec![];
    let _messages: Messages = vec![];
}
