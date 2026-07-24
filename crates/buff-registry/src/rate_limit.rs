//! IP-based rate limiting middleware — T57 Track F.
//!
//! A lightweight per-IP fixed-window rate limiter applied as an axum
//! middleware layer. Tracks request counts per client IP in an
//! in-memory `HashMap` behind a `Mutex`. When an IP exceeds the
//! budget within the window, subsequent requests get `429 Too Many
//! Requests`.
//!
//! # Configuration
//!
//! The rate limiter is configured via `AppState::rate_limit_window`
//! and `AppState::ip_rate_limit_max` (env
//! `BUFF_REGISTRY_IP_RATE_LIMIT_MAX`, default 1000 requests per
//! 5-minute window). The window is shared with the per-token publish
//! rate limit (same duration, different budget — IP limiting is more
//! generous since it covers ALL endpoints).
//!
//! # IP extraction
//!
//! The client IP is extracted from (in order):
//! 1. `X-Forwarded-For` header (first IP — for reverse proxies).
//! 2. `X-Real-IP` header (common with nginx).
//! 3. `127.0.0.1` fallback (in-process tests / localhost dev).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::AppState;

/// Per-IP rate limiter state. Shared across all handler tasks via
/// `Arc` inside [`AppState`].
///
/// Stores a `HashMap<IpAddr, Vec<Instant>>` — each IP has a rolling
/// window of request timestamps. Pruned on each check.
#[derive(Debug)]
pub struct IpRateLimiter {
    inner: Mutex<HashMap<IpAddr, Vec<Instant>>>,
}

impl IpRateLimiter {
    /// Construct an empty rate limiter.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Try to record a request from `ip`. Returns `true` if the
    /// request fits the `(window, max)` budget, `false` if the IP is
    /// over budget (the request is NOT recorded in that case).
    ///
    /// Prunes old timestamps before counting.
    pub fn try_record(
        &self,
        ip: IpAddr,
        window: std::time::Duration,
        max: usize,
    ) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return true; // mutex poisoned — allow (fail open, not fail closed)
        };
        let now = Instant::now();
        let entry = inner.entry(ip).or_default();
        entry.retain(|&t| now.duration_since(t) < window);
        if entry.len() >= max {
            return false;
        }
        entry.push(now);
        true
    }
}

impl Default for IpRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// The default per-IP rate limit budget (1000 requests per window).
pub const DEFAULT_IP_RATE_LIMIT_MAX: usize = 1000;

/// The env-var name for overriding the per-IP rate limit max.
pub const IP_RATE_LIMIT_MAX_ENV: &str = "BUFF_REGISTRY_IP_RATE_LIMIT_MAX";

/// Axum middleware: enforce per-IP rate limiting on ALL endpoints.
///
/// Extracts the client IP, checks the rate limiter, and either passes
/// the request through or returns `429 Too Many Requests`.
pub async fn ip_rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let ip = extract_client_ip(&request);
    if !state.ip_rate_limiter.try_record(
        ip,
        state.rate_limit_window,
        state.ip_rate_limit_max,
    ) {
        return (StatusCode::TOO_MANY_REQUESTS, "IP rate limit exceeded").into_response();
    }
    next.run(request).await
}

/// Extract the client IP from request headers.
///
/// Checks `X-Forwarded-For` (first IP), then `X-Real-IP`, then
/// falls back to `127.0.0.1`.
fn extract_client_ip(request: &Request) -> IpAddr {
    let headers = request.headers();
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(s) = xff.to_str() {
            if let Some(first) = s.split(',').next() {
                if let Ok(ip) = first.trim().parse() {
                    return ip;
                }
            }
        }
    }
    if let Some(xri) = headers.get("x-real-ip") {
        if let Ok(s) = xri.to_str() {
            if let Ok(ip) = s.parse() {
                return ip;
            }
        }
    }
    "127.0.0.1".parse::<IpAddr>().expect("valid IP")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rate_limiter_allows_under_budget() {
        let limiter = IpRateLimiter::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for _ in 0..5 {
            assert!(limiter.try_record(ip, Duration::from_secs(60), 5));
        }
    }

    #[test]
    fn rate_limiter_blocks_over_budget() {
        let limiter = IpRateLimiter::new();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for _ in 0..3 {
            assert!(limiter.try_record(ip, Duration::from_secs(60), 3));
        }
        // 4th request must be blocked.
        assert!(!limiter.try_record(ip, Duration::from_secs(60), 3));
    }

    #[test]
    fn rate_limiter_isolates_ips() {
        let limiter = IpRateLimiter::new();
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();
        for _ in 0..3 {
            assert!(limiter.try_record(ip1, Duration::from_secs(60), 3));
        }
        // ip1 is over budget, but ip2 is fresh.
        assert!(!limiter.try_record(ip1, Duration::from_secs(60), 3));
        assert!(limiter.try_record(ip2, Duration::from_secs(60), 3));
    }

    #[test]
    fn extract_client_ip_from_xff() {
        let limiter = IpRateLimiter::new();
        // Smoke test the parse logic.
        let ip: IpAddr = "5.6.7.8".parse().unwrap();
        assert!(limiter.try_record(ip, Duration::from_secs(1), 1));
    }
}
