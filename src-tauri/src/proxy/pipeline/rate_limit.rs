//! 上游限流（429 / 529）的 `Retry-After` 解析
//!
//! 上游明确告知「多久后再来」时，该 provider 进入冷却（见
//! [`ProviderRouter::start_cooldown`](crate::proxy::provider_router::ProviderRouter::start_cooldown)），
//! 冷却期间 `select_providers` 优先选择其他 provider；这次失败不计入熔断器。

use http::HeaderMap;
use std::time::{Duration, SystemTime};

/// 单次冷却的上限，防止异常的 `Retry-After` 把 provider 长时间挂起
pub(crate) const MAX_COOLDOWN: Duration = Duration::from_secs(600);

/// 429 Too Many Requests / 529 Overloaded：上游限流或过载
pub(crate) fn is_rate_limit_status(status: u16) -> bool {
    matches!(status, 429 | 529)
}

/// 限流响应携带的 `Retry-After`（秒数或 HTTP-date），封顶 [`MAX_COOLDOWN`]
///
/// 非限流状态码、没有该头或无法解析时返回 `None`；已过去的日期返回 `Duration::ZERO`。
pub(crate) fn rate_limit_retry_after(
    status: u16,
    headers: &HeaderMap,
    now: SystemTime,
) -> Option<Duration> {
    if !is_rate_limit_status(status) {
        return None;
    }
    let value = headers.get(http::header::RETRY_AFTER)?.to_str().ok()?;
    parse_retry_after(value, now).map(|delay| delay.min(MAX_COOLDOWN))
}

/// 解析 `Retry-After` 值：`delay-seconds` 或 HTTP-date（IMF-fixdate）
fn parse_retry_after(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        // 超出 u64 的秒数按上限处理
        return Some(value.parse().map_or(MAX_COOLDOWN, Duration::from_secs));
    }
    let at: SystemTime = chrono::DateTime::parse_from_rfc2822(value).ok()?.into();
    Some(at.duration_since(now).unwrap_or(Duration::ZERO))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(http::header::RETRY_AFTER, value.parse().unwrap());
        headers
    }

    fn at(rfc2822: &str) -> SystemTime {
        chrono::DateTime::parse_from_rfc2822(rfc2822)
            .unwrap()
            .into()
    }

    #[test]
    fn parses_delay_seconds_on_429_and_529() {
        let now = SystemTime::now();
        assert_eq!(
            rate_limit_retry_after(429, &headers("2"), now),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            rate_limit_retry_after(529, &headers(" 30 "), now),
            Some(Duration::from_secs(30))
        );
        assert_eq!(
            rate_limit_retry_after(429, &headers("0"), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn parses_http_date_relative_to_now() {
        let now = at("Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(
            rate_limit_retry_after(429, &headers("Wed, 21 Oct 2015 07:28:05 GMT"), now),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            rate_limit_retry_after(429, &headers("Wed, 21 Oct 2015 07:27:00 GMT"), now),
            Some(Duration::ZERO),
            "a date in the past means retry now"
        );
    }

    #[test]
    fn caps_the_cooldown() {
        let now = SystemTime::now();
        assert_eq!(
            rate_limit_retry_after(429, &headers("86400"), now),
            Some(MAX_COOLDOWN)
        );
        assert_eq!(
            rate_limit_retry_after(429, &headers("99999999999999999999999"), now),
            Some(MAX_COOLDOWN)
        );
    }

    #[test]
    fn ignores_other_statuses_missing_and_malformed_values() {
        let now = SystemTime::now();
        assert_eq!(rate_limit_retry_after(503, &headers("2"), now), None);
        assert_eq!(rate_limit_retry_after(429, &HeaderMap::new(), now), None);
        assert_eq!(rate_limit_retry_after(429, &headers("soon"), now), None);
        assert_eq!(rate_limit_retry_after(429, &headers("-1"), now), None);
        assert_eq!(rate_limit_retry_after(429, &headers("1.5"), now), None);
    }
}
