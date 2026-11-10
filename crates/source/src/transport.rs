//! Connector-local HTTP client construction.

use anyhow::{bail, Context, Result};
use sol_validator_watcher_config::TransportHeaders;
use std::time::Duration;

const HTTP_RESERVED_HEADERS: &[&str] = &["content-length", "host", "transfer-encoding"];

/// Builds one Rustls-backed client from validated connector transport settings.
pub(crate) fn build_http_client(
    request_timeout_sec: u64,
    connect_timeout_sec: u64,
    configured_headers: &TransportHeaders,
) -> Result<reqwest::Client> {
    if request_timeout_sec == 0 {
        bail!("HTTP transport request timeout must be greater than 0");
    }
    if connect_timeout_sec == 0 {
        bail!("HTTP transport connect timeout must be greater than 0");
    }
    if connect_timeout_sec > request_timeout_sec {
        bail!("HTTP transport connect timeout must not exceed request timeout");
    }

    let mut headers = reqwest::header::HeaderMap::with_capacity(configured_headers.iter().count());
    for (name, value) in configured_headers.iter() {
        let name = reqwest::header::HeaderName::from_bytes(name.trim().as_bytes())
            .context("invalid HTTP header name")?;
        if HTTP_RESERVED_HEADERS.contains(&name.as_str()) {
            bail!(
                "custom HTTP headers must not override protocol-owned header {:?}",
                name.as_str()
            );
        }
        let mut value = value
            .parse::<reqwest::header::HeaderValue>()
            .with_context(|| format!("invalid value for HTTP header {:?}", name.as_str()))?;
        value.set_sensitive(true);
        if headers.insert(name.clone(), value).is_some() {
            bail!("duplicate HTTP header name {:?}", name.as_str());
        }
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .connect_timeout(Duration::from_secs(connect_timeout_sec))
        .timeout(Duration::from_secs(request_timeout_sec))
        .build()
        .context("failed to build HTTP client")
}
