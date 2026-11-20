//! Hourly rate limits and the client address they are keyed on.

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{request::Parts, HeaderMap, StatusCode},
};
use sqlx::{PgPool, Postgres, Transaction};
use std::{
    convert::Infallible,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::LazyLock,
};
use uuid::Uuid;

use crate::error::{db, error, ApiError};

// Hourly limits. Per-IP limits come first so that one client cannot use up a shared budget.
pub(crate) const CREATE_PER_IP: i32 = 20;
pub(crate) const CREATE_PER_ORGANIZER: i32 = 5;
pub(crate) const VOTES_PER_IP: i32 = 60;
pub(crate) const VOTES_PER_PARTICIPANT: i32 = 30;
pub(crate) const RECOVERY_PER_IP: i32 = 30;
pub(crate) const RECOVERY_PER_EMAIL: i32 = 3;
pub(crate) const RECOVERY_PER_EVENT: i32 = 30;
pub(crate) const MESSAGES_PER_IP: i32 = 10;
pub(crate) const MESSAGES_PER_EVENT: i32 = 5;
pub(crate) const STATUS_WRITES_PER_EVENT: i32 = 20;
pub(crate) const DETAILS_WRITES_PER_EVENT: i32 = 20;
/// Status and detail changes that email every participant.
const NOTIFICATIONS_PER_EVENT: i32 = 10;

/// `FRIZZI_RATE_LIMIT_SCALE` multiplies the per-IP limits, for example behind a shared NAT.
static IP_LIMIT_SCALE: LazyLock<i32> = LazyLock::new(|| {
    std::env::var("FRIZZI_RATE_LIMIT_SCALE")
        .ok()
        .and_then(|s| s.trim().parse::<i32>().ok())
        .filter(|scale| *scale > 0)
        .unwrap_or(1)
});
pub(crate) fn per_ip(limit: i32) -> i32 {
    limit.saturating_mul(*IP_LIMIT_SCALE)
}

/// `FRIZZI_TRUST_PROXY=1` always trusts forwarded headers, `0` never does, and when unset they are
/// trusted from a loopback peer, such as a reverse proxy on the same host.
static TRUST_PROXY: LazyLock<Option<bool>> = LazyLock::new(|| {
    match std::env::var("FRIZZI_TRUST_PROXY")
        .as_deref()
        .map(str::trim)
    {
        Ok("1") => Some(true),
        Ok("0") => Some(false),
        _ => None,
    }
});

/// Where the request came from, which decides how far forwarded headers can be trusted.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Origin {
    /// Vercel overwrites `X-Real-IP` and `X-Forwarded-For` with the client address.
    Vercel,
    /// A proxy we trust appends the address it saw to `X-Forwarded-For`.
    TrustedProxy,
    Direct,
}
fn origin(on_vercel: bool, trust_proxy: Option<bool>, peer: Option<IpAddr>) -> Origin {
    let loopback = peer.is_some_and(|ip| ip.to_canonical().is_loopback());
    if on_vercel {
        Origin::Vercel
    } else if trust_proxy.unwrap_or(loopback) {
        Origin::TrustedProxy
    } else {
        Origin::Direct
    }
}

/// The client address used for rate limiting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ClientIp(Option<IpAddr>);
impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Infallible> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0.ip());
        let on_vercel = parts.extensions.get::<vercel_runtime::AppState>().is_some();
        let origin = origin(on_vercel, *TRUST_PROXY, peer);
        Ok(ClientIp(client_ip(&parts.headers, origin, peer)))
    }
}
impl ClientIp {
    pub(crate) fn key(&self, scope: &str) -> String {
        format!("{scope}-ip:{}", ip_bucket(self.0))
    }
}
fn client_ip(headers: &HeaderMap, origin: Origin, peer: Option<IpAddr>) -> Option<IpAddr> {
    let real_ip = || {
        headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_ip)
    };
    // Entries in the order proxies appended them; the leading ones may come from the client.
    let forwarded = || -> Vec<&str> {
        headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .collect()
    };
    match origin {
        Origin::Vercel => real_ip().or_else(|| forwarded().first().copied().and_then(parse_ip)),
        Origin::TrustedProxy => {
            let forwarded = forwarded();
            if forwarded.is_empty() {
                real_ip()
            } else {
                forwarded.last().copied().and_then(parse_ip)
            }
        }
        Origin::Direct => None,
    }
    .or(peer)
}
fn parse_ip(value: &str) -> Option<IpAddr> {
    let value = value.trim();
    value
        .parse()
        .ok()
        .or_else(|| value.parse::<SocketAddr>().ok().map(|addr| addr.ip()))
}
/// IPv6 clients usually control a whole /64, so they share one bucket.
fn ip_bucket(ip: Option<IpAddr>) -> String {
    match ip.map(|ip| ip.to_canonical()) {
        Some(IpAddr::V4(ip)) => ip.to_string(),
        Some(IpAddr::V6(ip)) => {
            let s = ip.segments();
            format!("{}/64", Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        None => "unknown".into(),
    }
}

/// Counts a request against `key` in the current hour. Callers pass the pool so the increment
/// commits on its own and still counts when the request later fails and rolls back.
pub(crate) async fn rate_limit<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    key: &str,
    limit: i32,
) -> Result<(), ApiError> {
    let count: i32 = sqlx::query_scalar("INSERT INTO request_limits (key) VALUES ($1) ON CONFLICT (key, hour) DO UPDATE SET count=request_limits.count+1 RETURNING count")
        .bind(key).fetch_one(executor).await.map_err(db)?;
    if count > limit {
        return Err(error(StatusCode::TOO_MANY_REQUESTS, "rate_limited"));
    }
    Ok(())
}
/// Checks the limits in order and stops at the first one exceeded.
pub(crate) async fn rate_limits(pool: &PgPool, limits: &[(String, i32)]) -> Result<(), ApiError> {
    for (key, limit) in limits {
        rate_limit(pool, key, *limit).await?;
    }
    Ok(())
}
/// Counts a notification to every participant inside the transaction that enqueues it, so only
/// committed notifications count. The write limits above bound failed attempts.
pub(crate) async fn limit_notifications(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
) -> Result<(), ApiError> {
    rate_limit(
        &mut **tx,
        &format!("notify:{event}"),
        NOTIFICATIONS_PER_EVENT,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_headers_are_trusted_only_from_vercel_or_a_proxy() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        let loopback = ip("127.0.0.1");
        let remote = ip("192.0.2.1");
        assert_eq!(origin(true, Some(false), remote), Origin::Vercel);
        assert_eq!(origin(false, None, loopback), Origin::TrustedProxy);
        assert_eq!(
            origin(false, None, ip("::ffff:127.0.0.1")),
            Origin::TrustedProxy
        );
        assert_eq!(origin(false, None, ip("::1")), Origin::TrustedProxy);
        assert_eq!(origin(false, Some(false), loopback), Origin::Direct);
        assert_eq!(origin(false, None, remote), Origin::Direct);
        assert_eq!(origin(false, None, None), Origin::Direct);
        assert_eq!(origin(false, Some(true), remote), Origin::TrustedProxy);
    }
    #[test]
    fn client_ip_resists_spoofed_forwarded_entries() {
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        let peer = ip("127.0.0.1");
        let headers = |pairs: &[(&'static str, &str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.append(*name, value.parse().unwrap());
            }
            map
        };
        // nginx's $proxy_add_x_forwarded_for appends the real client to whatever it was sent.
        let spoofed = headers(&[
            ("x-forwarded-for", "198.51.100.99, 10.9.9.9, 203.0.113.7"),
            ("x-real-ip", "198.51.100.99"),
        ]);
        let honest = headers(&[("x-forwarded-for", "203.0.113.7")]);
        let proxied = |h: &HeaderMap| ip_bucket(client_ip(h, Origin::TrustedProxy, peer));
        assert_eq!(proxied(&spoofed), "203.0.113.7");
        assert_eq!(proxied(&spoofed), proxied(&honest));
        let split = headers(&[
            ("x-forwarded-for", "198.51.100.99"),
            ("x-forwarded-for", "203.0.113.7"),
        ]);
        assert_eq!(proxied(&split), "203.0.113.7");
        assert_eq!(
            client_ip(
                &headers(&[("x-real-ip", " 203.0.113.8 ")]),
                Origin::TrustedProxy,
                peer
            ),
            ip("203.0.113.8")
        );
        assert_eq!(
            client_ip(
                &headers(&[("x-forwarded-for", "[2001:db8::1]:443")]),
                Origin::TrustedProxy,
                None
            ),
            ip("2001:db8::1")
        );
        assert_eq!(
            client_ip(
                &headers(&[("x-forwarded-for", "garbage")]),
                Origin::TrustedProxy,
                peer
            ),
            peer
        );
        assert_eq!(
            client_ip(&HeaderMap::new(), Origin::TrustedProxy, peer),
            peer
        );
        // Vercel overwrites both headers, so its X-Real-IP and first entry are the client.
        assert_eq!(
            client_ip(&spoofed, Origin::Vercel, None),
            ip("198.51.100.99")
        );
        assert_eq!(client_ip(&honest, Origin::Vercel, None), ip("203.0.113.7"));
        assert_eq!(client_ip(&HeaderMap::new(), Origin::Vercel, None), None);
        // A direct client cannot choose its bucket.
        assert_eq!(
            client_ip(&spoofed, Origin::Direct, ip("192.0.2.1")),
            ip("192.0.2.1")
        );
    }
    #[tokio::test]
    async fn client_ip_extractor_tolerates_missing_connect_info() {
        let request = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.7")
            .body(())
            .unwrap();
        let (mut parts, ()) = request.into_parts();
        assert_eq!(
            ClientIp::from_request_parts(&mut parts, &()).await,
            Ok(ClientIp(None))
        );
        let peer: SocketAddr = "192.0.2.1:5000".parse().unwrap();
        parts.extensions.insert(ConnectInfo(peer));
        assert_eq!(
            ClientIp::from_request_parts(&mut parts, &()).await,
            Ok(ClientIp(Some(peer.ip())))
        );
        if std::env::var_os("FRIZZI_TRUST_PROXY").is_none() {
            parts
                .extensions
                .insert(ConnectInfo::<SocketAddr>("127.0.0.1:5000".parse().unwrap()));
            assert_eq!(
                ClientIp::from_request_parts(&mut parts, &()).await,
                Ok(ClientIp(Some("203.0.113.7".parse().unwrap())))
            );
        }
    }
    #[test]
    fn ip_buckets_group_ipv6_networks() {
        let bucket = |ip: &str| ip_bucket(Some(ip.parse().unwrap()));
        assert_eq!(bucket("203.0.113.7"), "203.0.113.7");
        assert_eq!(bucket("::ffff:203.0.113.7"), "203.0.113.7");
        assert_eq!(bucket("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2::/64");
        assert_eq!(bucket("2001:db8:1:2:ffff::1"), "2001:db8:1:2::/64");
        assert_ne!(bucket("2001:db8:1:3::1"), bucket("2001:db8:1:2::1"));
        assert_eq!(ip_bucket(None), "unknown");
        assert_eq!(ClientIp(None).key("vote"), "vote-ip:unknown");
    }
}
