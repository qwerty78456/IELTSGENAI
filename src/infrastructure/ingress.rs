//! Where a request comes from: this computer, the app's published port (the
//! Cloudflare Tunnel), or anywhere else.
//!
//! What changes the operator's Google project or key is decided per request:
//! a key typed in the browser and deleting a designed voice only for `Local`
//! requests, designing a voice also for `Published` ones. A request is
//! `Local` only when the server listens on a loopback address, it arrived on
//! the main port, no proxy forwarded it, no other site's page made it, and its
//! `Host` (and `Origin`, if any) name this computer. It is `Published` only
//! when it arrived on the published port and its `Host` (and `Origin`, if
//! any) name the tunnel (`PUBLIC_HOST`). Everything unknown is `Remote`.

use dioxus::server::axum::http::{Extensions, HeaderMap, header};
use std::net::{Ipv4Addr, Ipv6Addr};

/// The three kinds of request the server tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A browser on the server's own computer, on the main port.
    Local,
    /// Through `PUBLIC_PORT`, where the tunnel (behind Cloudflare Access)
    /// delivers internet users. Never "this computer".
    Published,
    /// Anything else: another computer, a proxy, a web page of another site.
    Remote,
}

impl Origin {
    /// May enter an API key in the browser and delete designed voices.
    pub fn is_local(self) -> bool {
        self == Origin::Local
    }

    /// May design a voice (Voice Design): on the server's own computer or
    /// through the tunnel.
    pub fn may_design_voices(self) -> bool {
        matches!(self, Origin::Local | Origin::Published)
    }
}

/// Request extension added by the `PUBLIC_PORT` listener to every request it
/// accepts (`startup` layers it over the whole router).
#[derive(Debug, Clone, Copy)]
pub struct PublishedListener;

/// Headers a proxy or tunnel adds. A request carrying any of them did not come
/// straight from a browser on this computer, whatever its `Host` says.
pub const FORWARDING_HEADERS: &[&str] = &[
    "cf-ray",
    "cf-connecting-ip",
    "cf-warp-tag-id",
    "cdn-loop",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "forwarded",
    "x-real-ip",
    "true-client-ip",
    "via",
];

/// Sent by current browsers: `cross-site` when a page of another site made
/// the request (a link, a form, a `no-cors` fetch).
const SEC_FETCH_SITE: &str = "sec-fetch-site";

/// Classifies one request; the first rule that matches wins:
/// 1. the server is not bound to a loopback address → `Remote` (a LAN or
///    `0.0.0.0` bind is never local, and its published port is reachable
///    without Cloudflare Access, so it is not trusted either);
/// 2. the browser says a page of another site made it
///    (`Sec-Fetch-Site: cross-site`) → `Remote`: the published port listens
///    on this computer, so without this any web page open here could design
///    voices through it without passing Cloudflare Access;
/// 3. it arrived on the published listener → `Published` when its `Host`
///    (port aside) is one of `public_hosts` (`PUBLIC_HOST`) and its
///    `Origin`, if present, names one of them too; otherwise `Remote` (a
///    page whose name was rebound to this computer, or anything else that
///    did not come through the tunnel's hostname);
/// 4. a proxy forwarded it → `Remote`;
/// 5. `Host` is missing or does not name this computer → `Remote` (DNS
///    rebinding);
/// 6. `Origin` is present and does not name this computer (or is `null`) →
///    `Remote` (a web page of another site posting to the app);
/// 7. otherwise → `Local`.
pub fn classify(
    published: bool,
    loopback_bind: bool,
    public_hosts: &[String],
    headers: &HeaderMap,
) -> Origin {
    if !loopback_bind {
        return Origin::Remote;
    }
    if headers
        .get_all(SEC_FETCH_SITE)
        .iter()
        .any(|site| site.as_bytes().eq_ignore_ascii_case(b"cross-site"))
    {
        return Origin::Remote;
    }
    if published {
        let named = |authority: &str| {
            host_part(authority).is_some_and(|host| {
                public_hosts
                    .iter()
                    .any(|public| public.eq_ignore_ascii_case(host))
            })
        };
        let host_named = single_value(headers, header::HOST).is_some_and(named);
        let origin_named = !headers.contains_key(header::ORIGIN)
            || single_value(headers, header::ORIGIN)
                .and_then(origin_authority)
                .is_some_and(named);
        return match host_named && origin_named {
            true => Origin::Published,
            false => Origin::Remote,
        };
    }
    if FORWARDING_HEADERS
        .iter()
        .any(|name| headers.contains_key(*name))
    {
        return Origin::Remote;
    }
    if !single_value(headers, header::HOST).is_some_and(loopback_authority) {
        return Origin::Remote;
    }
    if headers.contains_key(header::ORIGIN)
        && !single_value(headers, header::ORIGIN).is_some_and(loopback_origin)
    {
        return Origin::Remote;
    }
    Origin::Local
}

/// The origin of a request to a plain axum route (no request context there).
pub fn of_parts(extensions: &Extensions, headers: &HeaderMap) -> Origin {
    classify(
        extensions.get::<PublishedListener>().is_some(),
        loopback_bind(),
        &super::config::config().public_hosts,
        headers,
    )
}

/// The origin of the request a `#[server]` function is answering. Call it at
/// the start of the body: the request context does not follow `tokio::spawn`.
/// Without a request (a server-side call, a background task) it is `Remote`.
pub fn current() -> Origin {
    let Some(context) = dioxus::fullstack::FullstackContext::current() else {
        return Origin::Remote;
    };
    // Dioxus offers only a write lock on the request parts; it is held just
    // for this read.
    let parts = context.parts_mut();
    classify(
        parts.extensions.get::<PublishedListener>().is_some(),
        loopback_bind(),
        &super::config::config().public_hosts,
        &parts.headers,
    )
}

/// Whether the server listens on a loopback address only (127.0.0.1, ::1).
fn loopback_bind() -> bool {
    super::config::config().address.ip().is_loopback()
}

/// The header's value when it appears exactly once and is plain text.
fn single_value(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    value.to_str().ok()
}

/// `Origin: scheme://host[:port]` naming this computer. `null` never does.
fn loopback_origin(origin: &str) -> bool {
    origin_authority(origin).is_some_and(loopback_authority)
}

/// The `host[:port]` of `Origin: scheme://host[:port]`; `None` for `null`
/// or anything without a scheme.
fn origin_authority(origin: &str) -> Option<&str> {
    let (scheme, rest) = origin.split_once("://")?;
    (!scheme.is_empty()).then(|| rest.split('/').next().unwrap_or_default())
}

/// The host of `host[:port]` (a name or an IPv4 address; never a bracketed
/// IPv6 address, which is no tunnel hostname), when the port, if any, is a
/// port number.
fn host_part(authority: &str) -> Option<&str> {
    let authority = authority.trim();
    // `rest` is empty or `:` and what follows.
    let (host, rest) = authority.split_at(authority.find(':').unwrap_or(authority.len()));
    (!host.is_empty() && !host.starts_with('[') && valid_port(rest)).then_some(host)
}

/// `host[:port]` (as in `Host`) where the host is `localhost`, a name under
/// `.localhost`, a 127.x.y.z address or `[::1]`. Case-insensitive.
fn loopback_authority(authority: &str) -> bool {
    let authority = authority.trim();
    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((address, rest)) = bracketed.split_once(']') else {
            return false;
        };
        return valid_port(rest) && address.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback());
    }
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, format!(":{port}")),
        None => (authority, String::new()),
    };
    if !valid_port(&port) {
        return false;
    }
    let host = host.to_ascii_lowercase();
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return ip.is_loopback();
    }
    host == "localhost"
        || host.strip_suffix(".localhost").is_some_and(|name| {
            name.split('.').all(|label| {
                !label.is_empty() && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
        })
}

/// Nothing, or `:` and a port number.
fn valid_port(rest: &str) -> bool {
    match rest.strip_prefix(':') {
        None => rest.is_empty(),
        Some(port) => {
            !port.is_empty()
                && port.chars().all(|c| c.is_ascii_digit())
                && port.parse::<u16>().is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::server::axum::http::HeaderValue;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    /// The tunnel's hostname in these tests (`PUBLIC_HOST`).
    fn tunnel() -> Vec<String> {
        vec!["exams.example.org".into()]
    }

    fn local(pairs: &[(&str, &str)]) -> Origin {
        classify(false, true, &tunnel(), &headers(pairs))
    }

    fn published(pairs: &[(&str, &str)]) -> Origin {
        classify(true, true, &tunnel(), &headers(pairs))
    }

    #[test]
    fn origin_powers() {
        assert!(Origin::Local.is_local());
        assert!(!Origin::Published.is_local());
        assert!(!Origin::Remote.is_local());
        assert!(Origin::Local.may_design_voices());
        assert!(Origin::Published.may_design_voices());
        assert!(!Origin::Remote.may_design_voices());
    }

    #[test]
    fn a_network_bind_is_never_local_nor_published() {
        let host = [("host", "127.0.0.1:8080")];
        assert_eq!(
            classify(false, false, &tunnel(), &headers(&host)),
            Origin::Remote
        );
        // PUBLIC_PORT on a LAN address is reachable without Access.
        assert_eq!(
            classify(true, false, &tunnel(), &headers(&host)),
            Origin::Remote
        );
        let through_tunnel = [("host", "exams.example.org")];
        assert_eq!(
            classify(true, false, &tunnel(), &headers(&through_tunnel)),
            Origin::Remote
        );
    }

    #[test]
    fn the_published_listener_wins_over_forwarding_headers() {
        let tunnel = [
            ("host", "exams.example.org"),
            ("cf-connecting-ip", "203.0.113.7"),
            ("origin", "https://exams.example.org"),
        ];
        assert_eq!(published(&tunnel), Origin::Published);
    }

    #[test]
    fn the_published_listener_needs_the_tunnel_hostname() {
        // The tunnel's own requests, in any case and with or without a port.
        for host in [
            "exams.example.org",
            "EXAMS.Example.ORG",
            "exams.example.org:443",
            " exams.example.org ",
        ] {
            assert_eq!(published(&[("host", host)]), Origin::Published, "{host}");
        }
        // Another of several names.
        let several = vec!["a.example.com".to_string(), "exams.example.org".into()];
        let host = headers(&[("host", "a.example.com")]);
        assert_eq!(classify(true, true, &several, &host), Origin::Published);
        // DNS rebinding: a page of another name resolved to this computer,
        // making same-origin requests to the published port.
        let rebinding = [
            ("host", "attacker.example:8081"),
            ("origin", "http://attacker.example:8081"),
            ("sec-fetch-site", "same-origin"),
        ];
        assert_eq!(published(&rebinding), Origin::Remote);
        for host in [
            "attacker.example",
            "127.0.0.1:8081",
            "localhost:8081",
            "exams.example.org.attacker.example",
            "xexams.example.org",
            "exams.example.org:http",
            "exams.example.org:70000",
            "[::1]:8081",
            "",
        ] {
            assert_eq!(published(&[("host", host)]), Origin::Remote, "{host}");
        }
        // No Host, or two.
        assert_eq!(published(&[]), Origin::Remote);
        assert_eq!(
            published(&[("host", "exams.example.org"), ("host", "evil.example")]),
            Origin::Remote
        );
        // The right Host from a page of another origin.
        let host = ("host", "exams.example.org");
        for origin in [
            "https://attacker.example",
            "http://attacker.example:8081",
            "null",
            "",
            "exams.example.org",
            "https://exams.example.org.attacker.example",
        ] {
            assert_eq!(
                published(&[host, ("origin", origin)]),
                Origin::Remote,
                "{origin}"
            );
        }
        for origin in [
            "https://exams.example.org",
            "https://Exams.Example.org:443",
            "http://exams.example.org/",
        ] {
            assert_eq!(
                published(&[host, ("origin", origin)]),
                Origin::Published,
                "{origin}"
            );
        }
        // Without PUBLIC_HOST nothing is published.
        assert_eq!(classify(true, true, &[], &headers(&[host])), Origin::Remote);
    }

    #[test]
    fn pages_of_other_sites_are_remote_on_both_ports() {
        let host = ("host", "127.0.0.1:8081");
        for site in ["cross-site", "Cross-Site"] {
            // A web page open on this computer cannot use the published port
            // to get past Cloudflare Access.
            assert_eq!(
                published(&[("host", "exams.example.org"), ("sec-fetch-site", site)]),
                Origin::Remote,
                "{site}"
            );
            assert_eq!(
                local(&[host, ("sec-fetch-site", site)]),
                Origin::Remote,
                "{site}"
            );
        }
        for site in ["same-origin", "same-site", "none"] {
            assert_eq!(
                published(&[("host", "exams.example.org"), ("sec-fetch-site", site)]),
                Origin::Published,
                "{site}"
            );
            assert_eq!(
                local(&[host, ("sec-fetch-site", site)]),
                Origin::Local,
                "{site}"
            );
        }
        // The tunnel's own requests: same-origin fetches of the app's page.
        let tunnel = [
            ("host", "exams.example.org"),
            ("cf-connecting-ip", "203.0.113.7"),
            ("origin", "https://exams.example.org"),
            ("sec-fetch-site", "same-origin"),
        ];
        assert_eq!(published(&tunnel), Origin::Published);
    }

    #[test]
    fn forwarded_requests_are_remote() {
        for name in FORWARDING_HEADERS {
            assert_eq!(
                local(&[("host", "127.0.0.1:8080"), (name, "1")]),
                Origin::Remote,
                "{name}"
            );
        }
        // Header names are case-insensitive.
        assert_eq!(
            local(&[("host", "localhost"), ("CF-Ray", "abc")]),
            Origin::Remote
        );
    }

    #[test]
    fn host_must_name_this_computer() {
        for host in [
            "localhost",
            "localhost:8080",
            "LOCALHOST:8080",
            "app.localhost:8080",
            "a.b-c.localhost",
            "127.0.0.1",
            "127.0.0.1:8080",
            "127.1.2.3:80",
            "[::1]",
            "[::1]:8080",
            "[0:0:0:0:0:0:0:1]:8080",
        ] {
            assert_eq!(local(&[("host", host)]), Origin::Local, "{host}");
        }
        for host in [
            "",
            "evil.example",
            "evil.example:8080",
            "localhost.evil.example",
            "evillocalhost",
            ".localhost",
            "a..localhost",
            "a..b.localhost",
            "a_b.localhost",
            "localhost.",
            "192.168.1.10:8080",
            "0.0.0.0:8080",
            "128.0.0.1",
            "127.1",
            "::1",
            "[::1",
            "[::2]:8080",
            "[::ffff:127.0.0.1]:8080",
            "[::1]8080",
            "localhost:",
            "localhost:http",
            "localhost:70000",
            "localhost:80:80",
        ] {
            assert_eq!(local(&[("host", host)]), Origin::Remote, "{host}");
        }
        assert_eq!(local(&[]), Origin::Remote, "no Host");
        assert_eq!(
            local(&[("host", "localhost"), ("host", "evil.example")]),
            Origin::Remote,
            "two Host headers"
        );
    }

    #[test]
    fn origin_when_present_must_name_this_computer() {
        let host = ("host", "127.0.0.1:8080");
        for origin in [
            "http://127.0.0.1:8080",
            "http://localhost:8080",
            "http://LocalHost",
            "https://app.localhost",
            "http://[::1]:8080",
            "http://127.0.0.2:9000",
        ] {
            assert_eq!(
                local(&[host, ("origin", origin)]),
                Origin::Local,
                "{origin}"
            );
        }
        for origin in [
            "null",
            "",
            "https://evil.example",
            "http://evil.example:8080",
            "http://192.168.1.10:8080",
            "http://[::2]:8080",
            "127.0.0.1:8080",
            "://127.0.0.1",
        ] {
            assert_eq!(
                local(&[host, ("origin", origin)]),
                Origin::Remote,
                "{origin}"
            );
        }
        // The probe and plain downloads send no Origin.
        assert_eq!(local(&[host]), Origin::Local);
    }

    #[test]
    fn no_request_context_is_remote() {
        assert_eq!(current(), Origin::Remote);
    }
}
