//! 异步操作优化工具
//!
//! 提供并行化、批量处理等异步优化功能。

use std::future::Future;

/// 并行执行多个 Future，返回所有结果
///
/// # 参数
/// - `futures`: Future 列表
///
/// # 返回
/// 所有 Future 的结果向量
///
/// # 示例
/// ```rust,no_run
/// use oris::utils::join_all;
///
/// # async fn example() {
/// let futures = vec![
///     async { 1 },
///     async { 2 },
///     async { 3 },
/// ];
/// let results = join_all(futures).await;
/// assert_eq!(results, vec![1, 2, 3]);
/// # }
/// ```
pub async fn join_all<T, F>(futures: Vec<F>) -> Vec<T>
where
    F: Future<Output = T> + Send,
    T: Send,
{
    futures::future::join_all(futures).await
}

/// 并行执行多个 Future，返回第一个成功的结果或所有错误
///
/// 类似于 `futures::future::try_join_all`，但提供更清晰的错误处理。
pub async fn try_join_all<T, E, F>(futures: Vec<F>) -> Result<Vec<T>, E>
where
    F: Future<Output = Result<T, E>> + Send,
    T: Send,
    E: Send,
{
    futures::future::try_join_all(futures).await
}

/// 批量处理数据，并行执行
///
/// # 参数
/// - `items`: 要处理的数据项
/// - `batch_size`: 每批处理的数量
/// - `processor`: 处理函数
///
/// # 返回
/// 所有处理结果的向量
///
/// # 示例
/// ```rust,no_run
/// use oris::utils::batch_process;
///
/// # async fn example() {
/// let items = vec![1, 2, 3, 4, 5];
/// let results = batch_process(items, 2, |item| async move {
///     item * 2
/// }).await;
/// assert_eq!(results, vec![2, 4, 6, 8, 10]);
/// # }
/// ```
pub async fn batch_process<T, R, F, Fut>(items: Vec<T>, batch_size: usize, processor: F) -> Vec<R>
where
    T: Send + Sync + Clone,
    R: Send,
    F: Fn(T) -> Fut + Send + Sync,
    Fut: Future<Output = R> + Send,
{
    let mut results = Vec::with_capacity(items.len());

    for chunk in items.chunks(batch_size) {
        let futures: Vec<_> = chunk.iter().map(|item| processor(item.clone())).collect();
        let chunk_results = join_all(futures).await;
        results.extend(chunk_results);
    }

    results
}

/// 批量处理数据，并行执行（带错误处理）
pub async fn batch_process_result<T, R, E, F, Fut>(
    items: Vec<T>,
    batch_size: usize,
    processor: F,
) -> Result<Vec<R>, E>
where
    T: Send + Sync + Clone,
    R: Send,
    E: Send,
    F: Fn(T) -> Fut + Send + Sync,
    Fut: Future<Output = Result<R, E>> + Send,
{
    let mut results = Vec::with_capacity(items.len());

    for chunk in items.chunks(batch_size) {
        let futures: Vec<_> = chunk.iter().map(|item| processor(item.clone())).collect();
        let chunk_results = try_join_all(futures).await?;
        results.extend(chunk_results);
    }

    Ok(results)
}

/// 并行执行多个 Future，使用 tokio::spawn
///
/// 适用于 CPU 密集型或长时间运行的任务。
pub async fn spawn_all<T, F>(futures: Vec<F>) -> Vec<Result<T, tokio::task::JoinError>>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = futures.into_iter().map(|f| tokio::spawn(f)).collect();
    join_all(handles).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_join_all() {
        let futures = vec![async { 1 }, async { 2 }, async { 3 }];
        let results = join_all(futures).await;
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_try_join_all() {
        let futures = vec![async { Ok::<i32, &str>(1) }, async { Ok(2) }, async {
            Ok(3)
        }];
        let results = try_join_all(futures).await.unwrap();
        assert_eq!(results, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_batch_process() {
        let items = vec![1, 2, 3, 4, 5];
        let results = batch_process(items, 2, |item| async move { item * 2 }).await;
        assert_eq!(results, vec![2, 4, 6, 8, 10]);
    }

    #[tokio::test]
    async fn test_batch_process_result() {
        let items = vec![1, 2, 3];
        let results =
            batch_process_result(items, 2, |item| async move { Ok::<i32, &str>(item * 2) })
                .await
                .unwrap();
        assert_eq!(results, vec![2, 4, 6]);
    }
}
