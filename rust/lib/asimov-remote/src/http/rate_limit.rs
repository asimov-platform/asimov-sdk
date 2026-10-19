// This is free and unencumbered software released into the public domain.

use super::Error;
use core::time::Duration;
use reqwest::{Response, StatusCode, header::RETRY_AFTER};
use std::time::{Instant, SystemTime};

pub(super) fn check(response: &Response) -> Result<(), Error> {
    if response.status() != StatusCode::TOO_MANY_REQUESTS {
        return Ok(());
    }
    let mut values = response.headers().get_all(RETRY_AFTER).iter();
    let retry_after = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| parse(value, SystemTime::now()))
        .filter(|_| values.next().is_none());
    Err(Error::RateLimited { retry_after })
}

fn parse(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    let delay = if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Duration::from_secs(value.parse().ok()?)
    } else {
        httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or_default()
    };
    // Never truncate a server's delay, or pass an overflowing delay to a timer.
    Instant::now().checked_add(delay).map(|_| delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_seconds_and_http_dates_without_shortening_the_wait() {
        let now = httpdate::parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        for (value, seconds) in [
            ("0", 0),
            ("120", 120),
            (" 00120\t", 120),
            ("Sun, 06 Nov 1994 08:50:37 GMT", 60),
            ("Sunday, 06-Nov-94 08:50:37 GMT", 60),
            ("Sun Nov  6 08:50:37 1994", 60),
            ("Sun, 06 Nov 1994 08:49:37 GMT", 0),
            ("Sun, 06 Nov 1994 08:48:37 GMT", 0),
        ] {
            assert_eq!(
                parse(value, now),
                Some(Duration::from_secs(seconds)),
                "{value}"
            );
        }
        assert_eq!(
            parse(
                "Sun, 06 Nov 1994 08:50:37 GMT",
                now + Duration::from_millis(500)
            ),
            Some(Duration::from_millis(59_500))
        );
    }

    #[test]
    fn rejects_invalid_or_overflowing_delays() {
        for value in [
            "",
            " ",
            "-1",
            "+1",
            "1.5",
            "later",
            "1, 2",
            "18446744073709551615",
            "18446744073709551616",
        ] {
            assert_eq!(parse(value, SystemTime::now()), None, "{value}");
        }
    }
}
