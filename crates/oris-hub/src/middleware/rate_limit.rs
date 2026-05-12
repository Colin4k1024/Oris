use axum::{extract::Request, middleware::Next, response::Response};
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::error::HubError;

pub type GlobalLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

pub fn create_limiter(per_second: u32) -> GlobalLimiter {
    let quota = Quota::per_second(NonZeroU32::new(per_second).unwrap());
    Arc::new(RateLimiter::direct(quota))
}

pub async fn check_rate_limit(req: Request, next: Next) -> Result<Response, HubError> {
    if let Some(limiter) = req.extensions().get::<GlobalLimiter>() {
        if limiter.check().is_err() {
            return Err(HubError::RateLimited);
        }
    }
    Ok(next.run(req).await)
}
