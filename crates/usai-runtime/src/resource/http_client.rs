//! Outbound HTTP as a declared resource (`http.client`), not a global
//! `fetch` (ADR-0017).
//!
//! A world never talks to the network by itself: it declares *which* HTTP
//! endpoint it depends on, the runtime owns the client (connection pool, TLS
//! roots, timeouts, a concurrency bound) at runtime lifetime, and each
//! request is one bounded operation with an owner. Cancelling the world drops
//! the request; a dropped HTTP request is terminal for the client side (the
//! connection is closed or the stream reset, nothing is reused), so the
//! reuse question C5 asks has a trivial answer here and is recorded as such.
//!
//! What this buys over a global `fetch`: egress is visible in `usai graph`,
//! `inspect` and the API docs; a `baseUrl` pins the destination so a handler
//! cannot be talked into calling somewhere else; secrets reach the client
//! through the declared environment instead of string concatenation in the
//! handler.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::{
    ResourceCall, ResourceError, ResourceIdentity, ResourceManager, ResourceProvider,
    ResourceStatus, TerminalProof,
};
use crate::definition::ResourceSpec;

pub struct HttpClientProvider;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct HttpClientConfig {
    /// When set, every request is relative to it and an absolute URL to
    /// another origin is refused: the resource *is* the destination.
    base_url: Option<String>,
    /// Environment variable holding the base URL instead (staging and
    /// production differ; the declaration does not).
    base_url_env: Option<String>,
    /// Per-request timeout; default 10 s. A request may lower it.
    timeout_ms: Option<u64>,
    /// Bound on in-flight requests; the next one is refused with
    /// `resource_exhausted` (503) rather than queued without limit.
    max_concurrent: Option<usize>,
    /// Static headers sent with every request.
    #[serde(default)]
    headers: BTreeMap<String, String>,
    /// Environment variable whose value becomes `Authorization: Bearer …`.
    bearer_token_env: Option<String>,
    /// Let a client **without** a `baseUrl` reach loopback, private,
    /// link-local and unique-local addresses. Off by default: the only
    /// reason such a client exists is that the destination comes from the
    /// application's data — a tenant-configured webhook — and that is
    /// exactly the input that turns `http://169.254.169.254/…` or the
    /// runtime's own status listener into a request the application makes
    /// on the caller's behalf. A client that names its destination
    /// (`baseUrl`/`baseUrlEnv`) is already pinned to one origin and is not
    /// affected.
    #[serde(default)]
    allow_private_network: bool,
}

#[async_trait]
impl ResourceProvider for HttpClientProvider {
    fn kind(&self) -> &str {
        "http.client"
    }

    fn compat(&self) -> u32 {
        1
    }

    async fn open(
        &self,
        spec: &ResourceSpec,
        identity: ResourceIdentity,
        env: &(dyn for<'a> Fn(&'a str) -> Option<String> + Sync),
    ) -> Result<Arc<dyn ResourceManager>, ResourceError> {
        let config: HttpClientConfig =
            serde_json::from_value(spec.config.clone()).map_err(|e| {
                ResourceError::Startup(spec.name.clone(), format!("invalid config: {e}"))
            })?;
        let base_url = match (&config.base_url, &config.base_url_env) {
            (Some(raw), _) => Some(raw.clone()),
            (None, Some(var)) => Some(env(var).ok_or_else(|| {
                ResourceError::Startup(
                    spec.name.clone(),
                    format!("environment variable {var} is not set"),
                )
            })?),
            (None, None) => None,
        };
        let base = match &base_url {
            Some(raw) => {
                let url = reqwest::Url::parse(raw).map_err(|e| {
                    ResourceError::Startup(spec.name.clone(), format!("baseUrl {raw:?}: {e}"))
                })?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(ResourceError::Startup(
                        spec.name.clone(),
                        format!("baseUrl {raw:?}: only http and https are supported"),
                    ));
                }
                Some(url)
            }
            None => None,
        };
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in &config.headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
                ResourceError::Startup(spec.name.clone(), format!("header {name:?}: {e}"))
            })?;
            let value = reqwest::header::HeaderValue::from_str(value).map_err(|e| {
                ResourceError::Startup(spec.name.clone(), format!("header {name:?}: {e}"))
            })?;
            headers.insert(name, value);
        }
        if let Some(var) = &config.bearer_token_env {
            let token = env(var).ok_or_else(|| {
                ResourceError::Startup(
                    spec.name.clone(),
                    format!("environment variable {var} is not set"),
                )
            })?;
            let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| ResourceError::Startup(spec.name.clone(), format!("{var}: {e}")))?;
            value.set_sensitive(true);
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        let timeout = Duration::from_millis(config.timeout_ms.unwrap_or(10_000).max(1));
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(timeout)
            .user_agent(format!("usai/{}", crate::definition::RUNTIME_VERSION))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| ResourceError::Startup(spec.name.clone(), e.to_string()))?;
        let max = config.max_concurrent.unwrap_or(32).max(1);
        Ok(Arc::new(HttpClient {
            identity,
            client,
            base,
            allow_private_network: config.allow_private_network,
            timeout,
            max: max as u32,
            slots: Arc::new(Semaphore::new(max)),
            counters: Counters::default(),
        }))
    }
}

#[derive(Default)]
struct Counters {
    requests: AtomicU64,
    failures: AtomicU64,
    cancelled: AtomicU64,
    refused: AtomicU64,
}

pub struct HttpClient {
    identity: ResourceIdentity,
    client: reqwest::Client,
    base: Option<reqwest::Url>,
    allow_private_network: bool,
    timeout: Duration,
    max: u32,
    slots: Arc<Semaphore>,
    counters: Counters,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchRequest {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// Addresses an application must not reach *by accident* on behalf of a
/// caller who chose the URL: the host's own loopback (which includes this
/// runtime's status listener), the private ranges every cloud puts its
/// internal services on, the link-local range that carries instance
/// metadata (`169.254.169.254`), and their IPv6 equivalents.
///
/// This is the classic SSRF shape, and it is not hypothetical here: the
/// generic client exists *because* the destination comes from the
/// application's data (a tenant-configured webhook), so the destination is
/// attacker-influenced by construction.
fn is_internal(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // Carrier-grade NAT, 100.64.0.0/10: a cloud's own fabric.
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                // 0.0.0.0/8 — "this network"; 0.0.0.0 itself reaches localhost
                // on Linux, which is a well-worn bypass.
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            // An IPv4-mapped address is an IPv4 address wearing a hat.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_internal(IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local and fe80::/10 link-local; neither
                // predicate is stable, so they are written out.
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn terminal(code: &str, message: impl Into<String>) -> ResourceError {
    ResourceError::Operation {
        code: code.into(),
        message: message.into(),
        proof: TerminalProof::Terminal,
    }
}

impl HttpClient {
    fn resolve(&self, raw: &str) -> Result<reqwest::Url, ResourceError> {
        match &self.base {
            Some(base) => {
                let url = base
                    .join(raw)
                    .map_err(|e| terminal("invalid_url", format!("{raw:?}: {e}")))?;
                if url.origin() != base.origin() {
                    return Err(terminal(
                        "origin_refused",
                        format!(
                            "{raw:?} leaves the declared baseUrl origin {}; declare another http client for that destination",
                            base.origin().ascii_serialization()
                        ),
                    ));
                }
                Ok(url)
            }
            None => {
                let url = reqwest::Url::parse(raw)
                    .map_err(|e| terminal("invalid_url", format!("{raw:?}: {e}")))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(terminal(
                        "invalid_url",
                        format!("{raw:?}: only http and https are supported"),
                    ));
                }
                Ok(url)
            }
        }
    }
}

impl HttpClient {
    /// Refuses a destination inside the host's own network for a client
    /// that did not name its destination. A client with `baseUrl` is
    /// already pinned to one origin — the operator chose it — and is never
    /// checked; `allowPrivateNetwork: true` opts a generic client out, for
    /// the deployment that really does call internal services by dynamic
    /// URL.
    ///
    /// Resolution happens here, and the connection resolves again: a name
    /// that answers differently between the two would slip past (DNS
    /// rebinding). Refusing when *any* resolved address is internal closes
    /// the common case; a deployment that must be airtight against a
    /// hostile URL puts an egress proxy in front and points `baseUrlEnv` at
    /// it (`docs/THREAT-MODEL.md`).
    async fn check_destination(&self, url: &reqwest::Url) -> Result<(), ResourceError> {
        if self.base.is_some() || self.allow_private_network {
            return Ok(());
        }
        let Some(host) = url.host_str() else {
            return Ok(());
        };
        let port = url.port_or_known_default().unwrap_or(80);
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
            return if is_internal(ip) {
                Err(refused(host, ip))
            } else {
                Ok(())
            };
        }
        let resolved = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| terminal("http_connect", format!("{host}: {e}")))?;
        for address in resolved {
            if is_internal(address.ip()) {
                return Err(refused(host, address.ip()));
            }
        }
        Ok(())
    }
}

fn refused(host: &str, ip: std::net::IpAddr) -> ResourceError {
    terminal(
        "destination_refused",
        format!(
            "{host} resolves to {ip}, which is inside this host's own network. \
             A client declared without a `baseUrl` takes its destination from the \
             application's data, so it is refused there by default — that is the \
             request an attacker-chosen webhook URL would make on your behalf \
             (instance metadata, the runtime's own status listener, an internal \
             service). Name the destination with `baseUrl`/`baseUrlEnv`, or, if \
             this client really does call internal addresses chosen at runtime, \
             declare `allowPrivateNetwork: true`."
        ),
    )
}

#[async_trait]
impl ResourceManager for HttpClient {
    fn identity(&self) -> &ResourceIdentity {
        &self.identity
    }

    async fn call(
        &self,
        call: ResourceCall,
        cancel: CancellationToken,
    ) -> Result<Value, ResourceError> {
        if call.method != "fetch" {
            return Err(ResourceError::UnknownMethod {
                resource: self.identity.name.clone(),
                method: call.method,
            });
        }
        let request: FetchRequest = serde_json::from_value(call.args)
            .map_err(|e| terminal("invalid_args", e.to_string()))?;
        let url = self.resolve(&request.url)?;
        self.check_destination(&url).await?;
        let method = request
            .method
            .as_deref()
            .unwrap_or("GET")
            .parse::<reqwest::Method>()
            .map_err(|_| terminal("invalid_method", format!("{:?}", request.method)))?;
        // The bound is a refusal, not a queue: waiting here would hide
        // saturation from the caller and from admission.
        let Ok(_slot) = Arc::clone(&self.slots).try_acquire_owned() else {
            self.counters.refused.fetch_add(1, Ordering::SeqCst);
            return Err(ResourceError::Exhausted {
                resource: self.identity.name.clone(),
            });
        };
        self.counters.requests.fetch_add(1, Ordering::SeqCst);
        let mut builder = self.client.request(method, url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        if let Some(ms) = request.timeout_ms {
            builder = builder.timeout(Duration::from_millis(ms.max(1)).min(self.timeout));
        }
        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // Dropping the future closes the connection: terminal for
                // the client side, nothing to quarantine.
                self.counters.cancelled.fetch_add(1, Ordering::SeqCst);
                return Err(ResourceError::Cancelled);
            }
            sent = builder.send() => sent,
        };
        let response = match response {
            Ok(r) => r,
            Err(e) => {
                self.counters.failures.fetch_add(1, Ordering::SeqCst);
                let code = if e.is_timeout() {
                    "http_timeout"
                } else if e.is_connect() {
                    "http_connect"
                } else {
                    "http_error"
                };
                return Err(terminal(code, e.to_string()));
            }
        };
        let status = response.status().as_u16();
        let mut headers = serde_json::Map::new();
        for (name, value) in response.headers() {
            if let Ok(text) = value.to_str() {
                headers.insert(name.as_str().to_owned(), json!(text));
            }
        }
        let bytes = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                self.counters.cancelled.fetch_add(1, Ordering::SeqCst);
                return Err(ResourceError::Cancelled);
            }
            body = response.bytes() => body,
        }
        .map_err(|e| {
            self.counters.failures.fetch_add(1, Ordering::SeqCst);
            terminal(
                if e.is_timeout() {
                    "http_timeout"
                } else {
                    "http_error"
                },
                e.to_string(),
            )
        })?;
        // Text bodies travel as text; anything else as base64 so the guest
        // can still see it without guessing an encoding.
        let body = match std::str::from_utf8(&bytes) {
            Ok(text) => json!({ "text": text }),
            Err(_) => {
                use base64::Engine;
                json!({ "base64": base64::engine::general_purpose::STANDARD.encode(&bytes) })
            }
        };
        Ok(json!({
            "status": status,
            "ok": (200..300).contains(&status),
            "headers": Value::Object(headers),
            "body": body,
        }))
    }

    fn status(&self) -> ResourceStatus {
        let mut detail = BTreeMap::new();
        detail.insert(
            "requests".into(),
            json!(self.counters.requests.load(Ordering::SeqCst)),
        );
        detail.insert(
            "failures".into(),
            json!(self.counters.failures.load(Ordering::SeqCst)),
        );
        detail.insert(
            "cancelled".into(),
            json!(self.counters.cancelled.load(Ordering::SeqCst)),
        );
        detail.insert(
            "refused".into(),
            json!(self.counters.refused.load(Ordering::SeqCst)),
        );
        if let Some(base) = &self.base {
            detail.insert("baseUrl".into(), json!(base.as_str()));
        }
        ResourceStatus {
            identity: self.identity.clone(),
            ready: true,
            in_use: self.max - self.slots.available_permits() as u32,
            max: self.max,
            quarantined: 0,
            detail,
        }
    }

    async fn shutdown(&self) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::is_internal;
    use std::net::IpAddr;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("address")
    }

    /// The addresses a tenant-chosen webhook URL must not reach. Each line
    /// here is a documented SSRF target, not a hypothetical: the metadata
    /// service, the runtime's own listeners, a cloud's internal fabric, and
    /// the two spellings people forget (`0.0.0.0`, which reaches localhost
    /// on Linux, and an IPv4-mapped IPv6 address).
    #[test]
    fn the_hosts_own_network_is_internal() {
        for address in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "100.127.255.255",
            "255.255.255.255",
            "::1",
            "::",
            "fd00::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_internal(ip(address)), "{address} must be refused");
        }
    }

    #[test]
    fn ordinary_public_addresses_are_not() {
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "172.32.0.1",
            // TEST-NET-3 is reserved, not internal: refusing it would be a
            // policy about documentation rather than about this host's
            // network, and it is the address a test reaches for when it
            // wants "public and not connectable".
            "198.51.100.7",
            "100.128.0.1",
            "100.63.255.255",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_internal(ip(address)), "{address} must be allowed");
        }
    }
}
