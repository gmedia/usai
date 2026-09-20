//! PostgreSQL: the first serious persistent capability (`GOAL.md` §24,
//! contract C5).
//!
//! ```text
//! normal completion / SQL error   terminal protocol state -> connection returns to the pool
//! cooperative cancel / timeout    CancelRequest -> await the original query's terminal
//!                                 state (57014) -> only then reusable
//! ambiguous abandonment           terminal state unknown -> quarantine: removed from
//!                                 the pool, never reused
//! ```
//!
//! A transaction pins one connection for several statements. It is one
//! owned operation from the world's point of view: `begin` leases the
//! connection into a holder task, `query/one/execute` with the lease route to
//! it, `commit`/`rollback` end it. The holder watches the world's
//! cancellation: a world that dies (or ends) with the transaction open gets
//! `ROLLBACK` issued on its behalf, and the connection returns to the pool
//! only when that rollback reached a terminal state — otherwise quarantine,
//! exactly as for a single statement.
//!
//! Backend identity and prepared statements belong to the physical
//! connection (established once per connection), never to a world. Session
//! state (`SET`, advisory locks, temp tables) is reset on every checkout by
//! default so it cannot leak between worlds either.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_postgres::types::{Kind, ToSql, Type};
use tokio_postgres::{Row, Statement};
use tokio_util::sync::CancellationToken;

use super::{
    ResourceCall, ResourceError, ResourceIdentity, ResourceManager, ResourceProvider,
    ResourceStatus, TerminalProof,
};
use crate::definition::ResourceSpec;

/// How long a cooperative cancellation waits for the original query to
/// reach its terminal state before the connection is declared ambiguous.
const CANCEL_TERMINAL_BOUND: Duration = Duration::from_secs(30);

pub struct PostgresProvider;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PostgresConfig {
    #[serde(default)]
    url_env: Option<String>,
    #[serde(default)]
    pool: PoolConfig,
    #[serde(default)]
    tls: TlsConfig,
}

/// TLS is selected by the URL's `sslmode` (`disable`, `prefer` — the
/// default —, `require`); certificates are always verified against the
/// trust roots (Mozilla's bundle plus `caFile`, PEM). There is no
/// "encrypted but unverified" mode: libpq's `require` without a root is a
/// footgun this runtime does not reproduce.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TlsConfig {
    /// Path to a PEM bundle with additional trust roots (private CAs,
    /// managed-database roots); `PGSSLROOTCERT` in the environment is the
    /// fallback. Relative paths resolve against the CWD.
    ca_file: Option<String>,
}

#[derive(Deserialize, Default)]
struct PoolConfig {
    max: Option<usize>,
    /// `clean` (default) resets session state on every checkout so no
    /// world can observe another's `SET`/advisory locks/temp tables; `fast`
    /// skips that round-trip for applications that never touch session
    /// state and have measured the difference.
    recycling: Option<String>,
}

#[async_trait]
impl ResourceProvider for PostgresProvider {
    fn kind(&self) -> &str {
        "postgres"
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
        let config: PostgresConfig =
            serde_json::from_value(spec.config.clone()).unwrap_or_default();
        let url_env = config.url_env.unwrap_or_else(|| "DATABASE_URL".into());
        // Read the env before the first await: the closure is not `Sync`.
        let resolved = env(&url_env);
        let ca_file = config.tls.ca_file.clone().or_else(|| env("PGSSLROOTCERT"));
        let url = resolved.ok_or_else(|| {
            ResourceError::Startup(
                spec.name.clone(),
                format!("environment variable {url_env} is not set"),
            )
        })?;
        let mut pg_config: tokio_postgres::Config =
            url.parse().map_err(|e: tokio_postgres::Error| {
                ResourceError::Startup(
                    spec.name.clone(),
                    format!("invalid connection URL in {url_env}: {e}"),
                )
            })?;
        if pg_config.get_connect_timeout().is_none() {
            pg_config.connect_timeout(Duration::from_secs(5));
        }
        let recycling_method = match config.pool.recycling.as_deref() {
            None | Some("clean") => RecyclingMethod::Clean,
            Some("fast") => RecyclingMethod::Fast,
            Some(other) => {
                return Err(ResourceError::Startup(
                    spec.name.clone(),
                    format!("unknown pool.recycling {other:?}; use \"clean\" or \"fast\""),
                ));
            }
        };
        let tls = tls_connector(&spec.name, ca_file.as_deref())?;
        let manager = Manager::from_config(
            pg_config.clone(),
            tls.clone(),
            ManagerConfig { recycling_method },
        );
        let max = config.pool.max.unwrap_or(16).max(1);
        let pool = Pool::builder(manager)
            .max_size(max)
            .build()
            .map_err(|e| ResourceError::Startup(spec.name.clone(), e.to_string()))?;
        // Fail activation, not the first request, when the database is
        // unreachable (`GOAL.md` §32).
        let endpoint = pg_config
            .get_hosts()
            .iter()
            .zip(pg_config.get_ports().iter().chain(std::iter::repeat(&5432)))
            .map(|(h, p)| match h {
                tokio_postgres::config::Host::Tcp(h) => format!("{h}:{p}"),
                #[cfg(unix)]
                tokio_postgres::config::Host::Unix(path) => path.display().to_string(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let user = pg_config.get_user().unwrap_or("?").to_owned();
        let probe = pool.get().await.map_err(|e| {
            // Say where and why, without the pool library's framing.
            let hosts = pg_config
                .get_hosts()
                .iter()
                .zip(pg_config.get_ports().iter().chain(std::iter::repeat(&5432)))
                .map(|(h, p)| match h {
                    tokio_postgres::config::Host::Tcp(h) => format!("{h}:{p}"),
                    #[cfg(unix)]
                    tokio_postgres::config::Host::Unix(path) => path.display().to_string(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let reason = match &e {
                deadpool_postgres::PoolError::Backend(err) => {
                    let mut text = err.to_string();
                    if let Some(source) = std::error::Error::source(err) {
                        text = format!("{text}: {source}");
                    }
                    text
                }
                other => other.to_string(),
            };
            ResourceError::Startup(
                spec.name.clone(),
                format!(
                    "cannot connect to {hosts} (from {url_env}) as user {}: {reason}",
                    pg_config.get_user().unwrap_or("?")
                ),
            )
        })?;
        drop(probe);
        Ok(Arc::new(Postgres {
            identity,
            pool,
            health: Health {
                ready: std::sync::atomic::AtomicBool::new(true),
                detail: Mutex::new(HealthDetail {
                    last_error: None,
                    since: std::time::Instant::now(),
                }),
            },
            endpoint,
            user,
            max: max as u32,
            counters: Arc::new(Counters::default()),
            tls,
            transactions: Arc::new(Mutex::new(HashMap::new())),
            next_transaction: AtomicU64::new(1),
        }))
    }
}

#[derive(Default)]
struct Counters {
    operations: AtomicU64,
    returned: AtomicU64,
    quarantined: AtomicU64,
    cancelled: AtomicU64,
    transactions: AtomicU64,
    /// Transactions the world left open; rolled back on its behalf.
    rolled_back_for_world: AtomicU64,
}

/// What the last contact with the server said. `/_usai/status` reports it as
/// `resources[].ready`: true until a connection-level failure (the pool
/// cannot connect, a connection closed under a query, 08xxx/57P0x), true
/// again after the next successful operation or probe. A per-query error
/// (a constraint, a syntax error) is the query's, not the database's.
struct Health {
    /// Read on every successful call (one atomic load — the hot path never
    /// takes the lock while the database is fine).
    ready: std::sync::atomic::AtomicBool,
    detail: Mutex<HealthDetail>,
}

struct HealthDetail {
    last_error: Option<String>,
    since: std::time::Instant,
}

pub struct Postgres {
    identity: ResourceIdentity,
    pool: Pool,
    health: Health,
    /// `host:port[, …]` and user, for messages (never the password).
    endpoint: String,
    user: String,
    max: u32,
    counters: Arc<Counters>,
    tls: Tls,
    /// Open transactions by lease id: the channel to the holder task that
    /// owns the pinned connection.
    transactions: Arc<Mutex<HashMap<u64, mpsc::Sender<TxCommand>>>>,
    next_transaction: AtomicU64,
}

/// What a world may ask of its open transaction.
enum TxCommand {
    Sql {
        method: String,
        request: SqlRequest,
        reply: oneshot::Sender<Result<Value, ResourceError>>,
    },
    End {
        commit: bool,
        reply: oneshot::Sender<Result<Value, ResourceError>>,
    },
}

/// The connector every connection and every cancel request uses.
type Tls = tokio_postgres_rustls::MakeRustlsConnect;

/// Mozilla's roots plus the resource's `tls.caFile`. Verification is
/// always on; `sslmode` in the URL decides whether TLS is attempted.
fn tls_connector(resource: &str, ca_file: Option<&str>) -> Result<Tls, ResourceError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(path) = ca_file {
        let pem = std::fs::read(path).map_err(|e| {
            ResourceError::Startup(resource.into(), format!("tls.caFile {path}: {e}"))
        })?;
        let mut added = 0;
        for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
            let cert = cert.map_err(|e| {
                ResourceError::Startup(resource.into(), format!("tls.caFile {path}: {e}"))
            })?;
            roots.add(cert).map_err(|e| {
                ResourceError::Startup(resource.into(), format!("tls.caFile {path}: {e}"))
            })?;
            added += 1;
        }
        if added == 0 {
            return Err(ResourceError::Startup(
                resource.into(),
                format!("tls.caFile {path}: no certificates found (PEM expected)"),
            ));
        }
    }
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| ResourceError::Startup(resource.into(), format!("tls: {e}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// Owns one pooled connection for the duration of one operation. Dropped
/// without terminal proof, it removes the connection from the pool: an
/// abandoned operation never leaves a reusable-looking connection behind.
struct Lease {
    object: Option<Object>,
    terminal: bool,
    counters: Arc<Counters>,
}

impl Lease {
    fn client(&self) -> &deadpool_postgres::ClientWrapper {
        self.object.as_ref().expect("lease holds its connection")
    }

    /// Only a known terminal protocol outcome may mark the connection
    /// reusable.
    fn mark_terminal(&mut self) {
        self.terminal = true;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.terminal {
            self.counters.returned.fetch_add(1, Ordering::SeqCst);
            return; // Object drops back into the pool.
        }
        self.counters.quarantined.fetch_add(1, Ordering::SeqCst);
        if let Some(object) = self.object.take() {
            // Permanent removal; the pool creates a replacement lazily.
            let wrapper = Object::take(object);
            tracing::warn!(resource = %"postgres", "connection quarantined: original query has no terminal outcome");
            drop(wrapper);
        }
    }
}

#[derive(Deserialize)]
struct SqlRequest {
    sql: String,
    #[serde(default)]
    params: Vec<Value>,
    /// Set when the statement belongs to an open transaction.
    #[serde(default)]
    lease: Option<u64>,
    /// Runtime bookkeeping only (the queue's claim/done marks): the
    /// statement runs in its own transaction with `synchronous_commit = off`,
    /// so a commit does not wait for the WAL fsync. Losing such a mark to a
    /// crash means one redelivery — at-least-once already allows that — and
    /// the mark costs a network round trip instead of a disk flush (the
    /// queue campaign: 215 → ≈1 000 msg/s per instance on a 12 ms-fsync
    /// disk). Never exposed to application code; the guest cannot set it.
    #[serde(default)]
    async_commit: bool,
}

enum Finished<T> {
    Terminal(T),
    Ambiguous,
}

/// SQLSTATEs that mean the connection itself is gone: class 08
/// (connection exception) and 57P01–57P03 (admin shutdown, crash
/// shutdown, cannot connect now). The query has a terminal outcome but
/// the physical connection has none worth reusing.
fn connection_lost(code: &str) -> bool {
    code.starts_with("08") || matches!(code, "57P01" | "57P02" | "57P03")
}

/// Error codes that say the *database* is unreachable, as opposed to a
/// query that failed: they drive `resources[].ready` and are logged as one
/// rate-limited warning instead of a stack per request.
pub(crate) fn connection_level(code: &str) -> bool {
    code == "pool_error"
        || code == "connection_closed"
        || code
            .strip_prefix("sql_")
            .is_some_and(|c| connection_lost(&c.to_ascii_uppercase()))
}

fn sql_error(error: tokio_postgres::Error) -> ResourceError {
    if error.is_closed() {
        return ResourceError::Operation {
            code: "connection_closed".into(),
            message: error.to_string(),
            proof: TerminalProof::Ambiguous,
        };
    }
    if let Some(db) = error.as_db_error() {
        let code = db.code().code();
        ResourceError::Operation {
            code: format!("sql_{}", code.to_ascii_lowercase()),
            message: db.message().to_owned(),
            proof: if connection_lost(code) {
                TerminalProof::Ambiguous
            } else {
                TerminalProof::Terminal
            },
        }
    } else if error.is_closed() {
        ResourceError::Operation {
            code: "connection_closed".into(),
            message: error.to_string(),
            proof: TerminalProof::Ambiguous,
        }
    } else {
        // Client-side failures (parameter conversion, decoding) happen before
        // or after the wire exchange; the protocol state is known.
        let mut message = error.to_string();
        if let Some(source) = std::error::Error::source(&error) {
            message.push_str(": ");
            message.push_str(&source.to_string());
        }
        if message.contains("deserializing column") {
            message.push_str(" (unsupported column type; cast it in SQL, e.g. col::text)");
        }
        ResourceError::Operation {
            code: "postgres_error".into(),
            message,
            proof: TerminalProof::Terminal,
        }
    }
}

/// Runs `query` under cooperative cancellation. Returns `Ambiguous` only
/// when the original query could not be brought to a terminal state.
async fn run_cancellable<T>(
    client: &deadpool_postgres::ClientWrapper,
    cancel: &CancellationToken,
    counters: &Counters,
    tls: &Tls,
    query: impl Future<Output = Result<T, tokio_postgres::Error>>,
) -> Finished<Result<T, ResourceError>> {
    tokio::pin!(query);
    tokio::select! {
        biased;
        result = &mut query => Finished::Terminal(result.map_err(sql_error)),
        _ = cancel.cancelled() => {
            counters.cancelled.fetch_add(1, Ordering::SeqCst);
            // CancelRequest is a request, not proof: the original operation
            // must still reach its terminal state on this connection.
            let cancel_token = client.cancel_token();
            let _ = cancel_token.cancel_query(tls.clone()).await;
            match tokio::time::timeout(CANCEL_TERMINAL_BOUND, &mut query).await {
                Ok(Err(e)) if e.as_db_error().is_some_and(|db| db.code().code() == "57014") => {
                    Finished::Terminal(Err(ResourceError::Cancelled))
                }
                Ok(result) => Finished::Terminal(result.map_err(sql_error).and_then(|_| Err(ResourceError::Cancelled))),
                Err(_) => Finished::Ambiguous,
            }
        }
    }
}

/// ISO-8601 / RFC 3339, plus the forms PostgreSQL itself prints
/// (`2026-09-18 19:48:41.507406+07`, `2026-09-18 19:48:41+00:00`), so a
/// value read back with `::text` can be a parameter again.
fn parse_timestamptz(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let s = s.trim();
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&chrono::Utc));
    }
    let normalized = s.replacen(' ', "T", 1);
    for candidate in [normalized.clone(), format!("{normalized}:00")] {
        if let Ok(d) = chrono::DateTime::parse_from_rfc3339(&candidate) {
            return Some(d.with_timezone(&chrono::Utc));
        }
        for fmt in [
            "%Y-%m-%dT%H:%M:%S%.f%#z",
            "%Y-%m-%dT%H:%M:%S%.f%:z",
            "%Y-%m-%dT%H:%M:%S%#z",
        ] {
            if let Ok(d) = chrono::DateTime::parse_from_str(&candidate, fmt) {
                return Some(d.with_timezone(&chrono::Utc));
            }
        }
    }
    // No zone at all: PostgreSQL would assume the session time zone; the
    // runtime assumes UTC and says so in the docs.
    chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|n| n.and_utc())
}

fn param_error(index: usize, ty: &Type, detail: impl std::fmt::Display) -> ResourceError {
    ResourceError::Operation {
        code: "invalid_param".into(),
        message: format!("parameter ${} ({}): {detail}", index + 1, ty.name()),
        proof: TerminalProof::Terminal,
    }
}

/// Converts one JSON parameter to the type the prepared statement expects.
fn int_param(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
}

fn to_sql(
    index: usize,
    ty: &Type,
    value: &Value,
) -> Result<Box<dyn ToSql + Sync + Send>, ResourceError> {
    macro_rules! typed {
        ($t:ty, $conv:expr) => {{
            if value.is_null() {
                Ok(Box::new(None::<$t>) as Box<dyn ToSql + Sync + Send>)
            } else {
                let converted: $t = $conv
                    .ok_or_else(|| param_error(index, ty, format!("cannot encode {value}")))?;
                Ok(Box::new(Some(converted)) as Box<dyn ToSql + Sync + Send>)
            }
        }};
    }
    match *ty {
        // Integers arrive as JSON numbers, or as strings when the guest kept
        // them exact (a bigint result beyond 2^53 comes back as a string and
        // must bind again as it came).
        Type::INT2 => typed!(i16, int_param(value).and_then(|n| i16::try_from(n).ok())),
        Type::INT4 => typed!(i32, int_param(value).and_then(|n| i32::try_from(n).ok())),
        Type::INT8 => typed!(i64, int_param(value)),
        Type::FLOAT4 => typed!(f32, value.as_f64().map(|f| f as f32)),
        Type::FLOAT8 => typed!(f64, value.as_f64()),
        Type::NUMERIC => typed!(
            rust_decimal::Decimal,
            value.as_str().and_then(|s| s.parse().ok()).or_else(|| value
                .as_f64()
                .and_then(rust_decimal::Decimal::from_f64_retain))
        ),
        Type::BOOL => typed!(bool, value.as_bool()),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => {
            typed!(
                String,
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(
                        || (!value.is_object() && !value.is_array()).then(|| value.to_string())
                    )
            )
        }
        Type::UUID => typed!(uuid::Uuid, value.as_str().and_then(|s| s.parse().ok())),
        // Bytes cross the boundary as base64 (the SDK encodes a `Uint8Array`).
        Type::BYTEA => typed!(
            Vec<u8>,
            value.as_str().and_then(|s| base64::Engine::decode(
                &base64::engine::general_purpose::STANDARD,
                s
            )
            .ok())
        ),
        Type::JSON | Type::JSONB => Ok(Box::new(value.clone())),
        Type::TIMESTAMPTZ => typed!(
            chrono::DateTime<chrono::Utc>,
            value.as_str().and_then(parse_timestamptz)
        ),
        Type::TIMESTAMP => typed!(
            chrono::NaiveDateTime,
            value.as_str().and_then(|s| s.parse().ok())
        ),
        Type::DATE => typed!(
            chrono::NaiveDate,
            value.as_str().and_then(|s| s.parse().ok())
        ),
        Type::TEXT_ARRAY | Type::VARCHAR_ARRAY => typed!(
            Vec<String>,
            value.as_array().map(|a| a
                .iter()
                .map(|v| v
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()))
                .collect())
        ),
        Type::INT4_ARRAY => typed!(
            Vec<i32>,
            value.as_array().and_then(|a| a
                .iter()
                .map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()))
                .collect())
        ),
        Type::INT8_ARRAY => typed!(
            Vec<i64>,
            value
                .as_array()
                .and_then(|a| a.iter().map(Value::as_i64).collect())
        ),
        // User-defined enums travel as their label; arrays of them too
        // (`status = any($1::invoice_status[])`).
        _ if matches!(ty.kind(), Kind::Enum(_)) => {
            typed!(EnumLabel, value.as_str().map(|s| EnumLabel(s.to_owned())))
        }
        _ if matches!(ty.kind(), Kind::Array(inner) if matches!(inner.kind(), Kind::Enum(_))) => {
            typed!(
                Vec<EnumLabel>,
                value.as_array().and_then(|a| a
                    .iter()
                    .map(|v| v.as_str().map(|s| EnumLabel(s.to_owned())))
                    .collect())
            )
        }
        // Anything else (interval, inet, macaddr, money, ranges, domains,
        // composite types …) travels in PostgreSQL's *text* format from a
        // string, and the server parses it exactly as `'…'::type` would —
        // what a `pg`/ActiveRecord user expects.
        _ => match value {
            Value::Null => Ok(Box::new(None::<TextParam>) as Box<dyn ToSql + Sync + Send>),
            Value::String(text) => {
                Ok(Box::new(Some(TextParam(text.clone()))) as Box<dyn ToSql + Sync + Send>)
            }
            other if !other.is_object() && !other.is_array() => {
                Ok(Box::new(Some(TextParam(other.to_string()))) as Box<dyn ToSql + Sync + Send>)
            }
            _ => Err(param_error(
                index,
                ty,
                "expected a string in the type's text form (or cast the parameter in SQL, e.g. $1::text)",
            )),
        },
    }
}

/// A value sent in text format for a type the runtime has no binary
/// encoder for; PostgreSQL parses it server-side.
#[derive(Debug)]
struct TextParam(String);

impl ToSql for TextParam {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> Result<tokio_postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        out.extend_from_slice(self.0.as_bytes());
        Ok(tokio_postgres::types::IsNull::No)
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> tokio_postgres::types::Format {
        tokio_postgres::types::Format::Text
    }

    tokio_postgres::types::to_sql_checked!();
}

/// A PostgreSQL enum value, sent as its label in text format.
#[derive(Debug)]
struct EnumLabel(String);

impl ToSql for EnumLabel {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> Result<tokio_postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        out.extend_from_slice(self.0.as_bytes());
        Ok(tokio_postgres::types::IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(ty.kind(), Kind::Enum(_))
    }

    tokio_postgres::types::to_sql_checked!();
}

/// A PostgreSQL enum value read back as its label.
struct EnumLabelOut(String);

impl<'a> tokio_postgres::types::FromSql<'a> for EnumLabelOut {
    fn from_sql(
        _ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(EnumLabelOut(std::str::from_utf8(raw)?.to_owned()))
    }

    fn accepts(ty: &Type) -> bool {
        matches!(ty.kind(), Kind::Enum(_))
    }
}

fn column_to_json(row: &Row, index: usize, ty: &Type) -> Result<Value, tokio_postgres::Error> {
    macro_rules! get {
        ($t:ty) => {
            row.try_get::<_, Option<$t>>(index).map(|v| json!(v))
        };
    }
    // A bigint beyond ±2^53 cannot survive a JSON number (the guest's Number
    // would round it): those travel as strings, like numeric does; ids and
    // counts in the safe range stay numbers.
    fn safe_i64(n: i64) -> Value {
        const SAFE: i64 = 9_007_199_254_740_992;
        if (-SAFE..=SAFE).contains(&n) {
            json!(n)
        } else {
            json!(n.to_string())
        }
    }
    match *ty {
        Type::INT2 => get!(i16),
        Type::INT4 => get!(i32),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(index)
            .map(|v| v.map(safe_i64).unwrap_or(Value::Null)),
        Type::OID => get!(u32),
        Type::FLOAT4 => get!(f32),
        Type::FLOAT8 => get!(f64),
        Type::NUMERIC => row
            .try_get::<_, Option<rust_decimal::Decimal>>(index)
            .map(|v| json!(v.map(|d| d.to_string()))),
        Type::BOOL => get!(bool),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::CHAR => get!(String),
        Type::UUID => row
            .try_get::<_, Option<uuid::Uuid>>(index)
            .map(|v| json!(v.map(|u| u.to_string()))),
        Type::JSON | Type::JSONB => row
            .try_get::<_, Option<Value>>(index)
            .map(|v| v.unwrap_or(Value::Null)),
        Type::TIMESTAMPTZ => row
            .try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(index)
            .map(|v| json!(v.map(|d| d.to_rfc3339()))),
        Type::TIMESTAMP => row
            .try_get::<_, Option<chrono::NaiveDateTime>>(index)
            .map(|v| json!(v.map(|d| d.to_string()))),
        Type::DATE => row
            .try_get::<_, Option<chrono::NaiveDate>>(index)
            .map(|v| json!(v.map(|d| d.to_string()))),
        Type::BYTEA => row.try_get::<_, Option<Vec<u8>>>(index).map(|v| {
            json!(v.map(|b| base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b)))
        }),
        Type::TEXT_ARRAY | Type::VARCHAR_ARRAY => get!(Vec<String>),
        Type::INT4_ARRAY => get!(Vec<i32>),
        Type::INT8_ARRAY => row.try_get::<_, Option<Vec<i64>>>(index).map(|v| {
            v.map(|a| Value::Array(a.into_iter().map(safe_i64).collect()))
                .unwrap_or(Value::Null)
        }),
        Type::VOID => Ok(Value::Null),
        _ if matches!(ty.kind(), Kind::Enum(_)) => row
            .try_get::<_, Option<EnumLabelOut>>(index)
            .map(|v| json!(v.map(|e| e.0))),
        _ => row.try_get::<_, Option<String>>(index).map(|v| json!(v)),
    }
}

fn row_to_json(row: &Row) -> Result<Value, tokio_postgres::Error> {
    let mut object = serde_json::Map::new();
    for (i, column) in row.columns().iter().enumerate() {
        object.insert(
            column.name().to_owned(),
            column_to_json(row, i, column.type_())?,
        );
    }
    Ok(Value::Object(object))
}

async fn prepare_and_bind(
    client: &deadpool_postgres::ClientWrapper,
    request: &SqlRequest,
) -> Result<(Statement, Vec<Box<dyn ToSql + Sync + Send>>), ResourceError> {
    let statement = client
        .prepare_cached(&request.sql)
        .await
        .map_err(sql_error)?;
    let types = statement.params();
    if types.len() != request.params.len() {
        return Err(ResourceError::Operation {
            code: "invalid_params".into(),
            message: format!(
                "statement expects {} parameters, got {}",
                types.len(),
                request.params.len()
            ),
            proof: TerminalProof::Terminal,
        });
    }
    let mut params = Vec::with_capacity(types.len());
    for (i, (ty, value)) in types.iter().zip(&request.params).enumerate() {
        params.push(to_sql(i, ty, value)?);
    }
    Ok((statement, params))
}

#[async_trait]
impl ResourceManager for Postgres {
    fn identity(&self) -> &ResourceIdentity {
        &self.identity
    }

    async fn call(
        &self,
        call: ResourceCall,
        cancel: CancellationToken,
    ) -> Result<Value, ResourceError> {
        let result = self.call_inner(call, cancel).await;
        self.observe(&result);
        result
    }

    fn status(&self) -> ResourceStatus {
        self.status_inner()
    }

    /// `SELECT 1` on a leased connection: the pool can hand one out and the
    /// server answers. Failures name the reason.
    async fn probe(&self) -> Result<(), String> {
        let result = self.probe_inner().await;
        match &result {
            Ok(()) => self.observe::<()>(&Ok(())),
            Err(e) => self.mark_unready(e),
        }
        result
    }

    async fn shutdown(&self) {
        self.pool.close();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Postgres {
    /// Folds one outcome into `Health`: a connection-level failure makes the
    /// resource unready, any success makes it ready again.
    fn observe<T>(&self, result: &Result<T, ResourceError>) {
        match result {
            Ok(_) => {
                if !self.health.ready.load(Ordering::Relaxed) {
                    let mut detail = self.health.detail.lock().expect("health poisoned");
                    if !self.health.ready.swap(true, Ordering::SeqCst) {
                        tracing::info!(resource = %self.identity.name, "database reachable again");
                        detail.last_error = None;
                        detail.since = std::time::Instant::now();
                    }
                }
            }
            Err(ResourceError::Operation { code, message, .. }) if connection_level(code) => {
                self.mark_unready(&format!("{code}: {message}"));
            }
            Err(_) => {}
        }
    }

    fn mark_unready(&self, error: &str) {
        let mut detail = self.health.detail.lock().expect("health poisoned");
        if self.health.ready.swap(false, Ordering::SeqCst) {
            detail.since = std::time::Instant::now();
        }
        detail.last_error = Some(error.to_owned());
    }

    async fn call_inner(
        &self,
        call: ResourceCall,
        cancel: CancellationToken,
    ) -> Result<Value, ResourceError> {
        match call.method.as_str() {
            "begin" => return self.begin(cancel).await,
            "commit" | "rollback" => {
                let lease = call
                    .args
                    .get("lease")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| ResourceError::Operation {
                        code: "invalid_args".into(),
                        message: format!("{} needs a lease", call.method),
                        proof: TerminalProof::Terminal,
                    })?;
                return self
                    .transaction_command(lease, |reply| TxCommand::End {
                        commit: call.method == "commit",
                        reply,
                    })
                    .await;
            }
            _ => {}
        }
        let request: SqlRequest =
            serde_json::from_value(call.args).map_err(|e| ResourceError::Operation {
                code: "invalid_args".into(),
                message: e.to_string(),
                proof: TerminalProof::Terminal,
            })?;
        self.counters.operations.fetch_add(1, Ordering::SeqCst);
        if let Some(lease) = request.lease {
            let method = call.method.clone();
            return self
                .transaction_command(lease, move |reply| TxCommand::Sql {
                    method,
                    request,
                    reply,
                })
                .await;
        }
        let object = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ResourceError::Cancelled),
            got = self.pool.get() => got.map_err(|e| match e {
                deadpool_postgres::PoolError::Timeout(_) => ResourceError::Exhausted { resource: self.identity.name.clone() },
                other => ResourceError::Operation { code: "pool_error".into(), message: self.pool_error_text(&other), proof: TerminalProof::Terminal },
            })?,
        };
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        let finished = {
            let client = lease.client();
            let (statement, params) = match prepare_and_bind(client, &request).await {
                Ok(bound) => bound,
                Err(e) => {
                    lease.mark_terminal(); // nothing was sent
                    return Err(e);
                }
            };
            let refs: Vec<&(dyn ToSql + Sync)> = params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect();
            match call.method.as_str() {
                "query" => {
                    run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                        let rows = client.query(&statement, &refs).await?;
                        rows.iter()
                            .map(row_to_json)
                            .collect::<Result<Vec<_>, _>>()
                            .map(Value::Array)
                    })
                    .await
                }
                "one" if request.async_commit => {
                    run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                        client
                            .batch_execute("BEGIN; SET LOCAL synchronous_commit = off")
                            .await?;
                        let rows = match client.query(&statement, &refs).await {
                            Ok(rows) => rows,
                            Err(e) => {
                                let _ = client.batch_execute("ROLLBACK").await;
                                return Err(e);
                            }
                        };
                        client.batch_execute("COMMIT").await?;
                        Ok(rows
                            .first()
                            .map(row_to_json)
                            .transpose()?
                            .unwrap_or(Value::Null))
                    })
                    .await
                }
                "execute" if request.async_commit => {
                    run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                        client
                            .batch_execute("BEGIN; SET LOCAL synchronous_commit = off")
                            .await?;
                        let n = match client.execute(&statement, &refs).await {
                            Ok(n) => n,
                            Err(e) => {
                                let _ = client.batch_execute("ROLLBACK").await;
                                return Err(e);
                            }
                        };
                        client.batch_execute("COMMIT").await?;
                        Ok(json!(n))
                    })
                    .await
                }
                "one" => {
                    run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                        let rows = client.query(&statement, &refs).await?;
                        Ok(rows
                            .first()
                            .map(row_to_json)
                            .transpose()?
                            .unwrap_or(Value::Null))
                    })
                    .await
                }
                "execute" => {
                    run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                        client.execute(&statement, &refs).await.map(|n| json!(n))
                    })
                    .await
                }
                other => {
                    lease.mark_terminal();
                    return Err(ResourceError::UnknownMethod {
                        resource: self.identity.name.clone(),
                        method: other.to_owned(),
                    });
                }
            }
        };
        match finished {
            Finished::Terminal(result) => {
                // A closed connection is terminal for the query but the
                // object is dead; quarantine it rather than return it.
                let reusable = !matches!(&result, Err(ResourceError::Operation { proof: TerminalProof::Ambiguous, .. }));
                if reusable {
                    lease.mark_terminal();
                }
                result
            }
            Finished::Ambiguous => Err(ResourceError::Operation {
                code: "cancel_unconfirmed".into(),
                message: "the query did not reach a terminal state after cancellation; connection quarantined".into(),
                proof: TerminalProof::Ambiguous,
            }),
        }
    }

    fn status_inner(&self) -> ResourceStatus {
        let status = self.pool.status();
        let mut detail = BTreeMap::new();
        detail.insert(
            "operations".into(),
            json!(self.counters.operations.load(Ordering::SeqCst)),
        );
        detail.insert(
            "returned".into(),
            json!(self.counters.returned.load(Ordering::SeqCst)),
        );
        detail.insert(
            "cancelled".into(),
            json!(self.counters.cancelled.load(Ordering::SeqCst)),
        );
        detail.insert(
            "transactions".into(),
            json!(self.counters.transactions.load(Ordering::SeqCst)),
        );
        detail.insert(
            "openTransactions".into(),
            json!(
                self.transactions
                    .lock()
                    .expect("transactions poisoned")
                    .len()
            ),
        );
        detail.insert(
            "rolledBackForWorld".into(),
            json!(self.counters.rolled_back_for_world.load(Ordering::SeqCst)),
        );
        detail.insert("poolSize".into(), json!(status.size));
        detail.insert("available".into(), json!(status.available));
        detail.insert("waiting".into(), json!(status.waiting));
        let ready = {
            let health = self.health.detail.lock().expect("health poisoned");
            if let Some(error) = &health.last_error {
                detail.insert("lastError".into(), json!(error));
                detail.insert(
                    "unreadyForSeconds".into(),
                    json!(health.since.elapsed().as_secs()),
                );
            }
            self.health.ready.load(Ordering::SeqCst)
        };
        ResourceStatus {
            identity: self.identity.clone(),
            ready,
            in_use: (status.size.saturating_sub(status.available)) as u32,
            max: self.max,
            quarantined: self.counters.quarantined.load(Ordering::SeqCst),
            detail,
        }
    }

    async fn probe_inner(&self) -> Result<(), String> {
        let object = self
            .pool
            .get()
            .await
            .map_err(|e| format!("no connection: {e}"))?;
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        match lease.client().simple_query("SELECT 1").await {
            Ok(_) => {
                lease.mark_terminal();
                Ok(())
            }
            Err(e) => Err(sql_error(e).to_string()),
        }
    }
}

/// A migration applied (or to apply) against this database.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedMigration {
    pub name: String,
    pub checksum: String,
    pub applied_at: String,
}

/// Runs one statement of an open transaction on its pinned connection.
async fn transaction_statement(
    client: &deadpool_postgres::ClientWrapper,
    cancel: &CancellationToken,
    counters: &Counters,
    tls: &Tls,
    method: &str,
    request: &SqlRequest,
) -> Finished<Result<Value, ResourceError>> {
    let (statement, params) = match prepare_and_bind(client, request).await {
        Ok(bound) => bound,
        Err(e) => return Finished::Terminal(Err(e)),
    };
    let refs: Vec<&(dyn ToSql + Sync)> = params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    match method {
        "query" => {
            run_cancellable(client, cancel, counters, tls, async {
                let rows = client.query(&statement, &refs).await?;
                rows.iter()
                    .map(row_to_json)
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            })
            .await
        }
        "one" => {
            run_cancellable(client, cancel, counters, tls, async {
                let rows = client.query(&statement, &refs).await?;
                Ok(rows
                    .first()
                    .map(row_to_json)
                    .transpose()?
                    .unwrap_or(Value::Null))
            })
            .await
        }
        "execute" => {
            run_cancellable(client, cancel, counters, tls, async {
                client.execute(&statement, &refs).await.map(|n| json!(n))
            })
            .await
        }
        other => Finished::Terminal(Err(ResourceError::UnknownMethod {
            resource: "postgres".into(),
            method: other.to_owned(),
        })),
    }
}

fn ambiguous_after_cancel() -> ResourceError {
    ResourceError::Operation {
        code: "cancel_unconfirmed".into(),
        message: "the statement did not reach a terminal state after cancellation; connection quarantined".into(),
        proof: TerminalProof::Ambiguous,
    }
}

impl Postgres {
    /// A pool failure with the facts an operator needs — where and as whom —
    /// instead of the pool library's framing.
    fn pool_error_text(&self, error: &deadpool_postgres::PoolError) -> String {
        let reason = match error {
            deadpool_postgres::PoolError::Backend(err) => {
                let mut text = err.to_string();
                if let Some(source) = std::error::Error::source(err) {
                    text = format!("{text}: {source}");
                }
                text
            }
            other => other.to_string(),
        };
        format!(
            "cannot connect to {} as user {}: {reason}",
            self.endpoint, self.user
        )
    }

    /// `BEGIN` on a leased connection, then hand the connection to a holder
    /// task that serves the world's statements until `commit`/`rollback` —
    /// or rolls back for the world when its cancellation fires first.
    async fn begin(&self, cancel: CancellationToken) -> Result<Value, ResourceError> {
        self.counters.operations.fetch_add(1, Ordering::SeqCst);
        let object = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ResourceError::Cancelled),
            got = self.pool.get() => got.map_err(|e| match e {
                deadpool_postgres::PoolError::Timeout(_) => ResourceError::Exhausted { resource: self.identity.name.clone() },
                other => ResourceError::Operation { code: "pool_error".into(), message: self.pool_error_text(&other), proof: TerminalProof::Terminal },
            })?,
        };
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        let began = {
            let client = lease.client();
            run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                client.batch_execute("BEGIN").await
            })
            .await
        };
        match began {
            Finished::Terminal(Ok(())) => {}
            Finished::Terminal(Err(e)) => {
                if !matches!(
                    &e,
                    ResourceError::Operation {
                        proof: TerminalProof::Ambiguous,
                        ..
                    }
                ) {
                    lease.mark_terminal();
                }
                return Err(e);
            }
            Finished::Ambiguous => return Err(ambiguous_after_cancel()),
        }
        self.counters.transactions.fetch_add(1, Ordering::SeqCst);
        let id = self.next_transaction.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel::<TxCommand>(1);
        self.transactions
            .lock()
            .expect("transactions poisoned")
            .insert(id, tx);
        let transactions = Arc::clone(&self.transactions);
        let counters = Arc::clone(&self.counters);
        let tls = self.tls.clone();
        tokio::spawn(async move {
            hold_transaction(lease, rx, cancel, &counters, &tls).await;
            transactions
                .lock()
                .expect("transactions poisoned")
                .remove(&id);
        });
        Ok(json!({ "lease": id }))
    }

    async fn transaction_command(
        &self,
        lease: u64,
        make: impl FnOnce(oneshot::Sender<Result<Value, ResourceError>>) -> TxCommand,
    ) -> Result<Value, ResourceError> {
        let sender = self
            .transactions
            .lock()
            .expect("transactions poisoned")
            .get(&lease)
            .cloned();
        let closed = || {
            ResourceError::Operation {
            code: "transaction_closed".into(),
            message: "this transaction is no longer open (committed, rolled back, or ended with its world)".into(),
            proof: TerminalProof::Terminal,
        }
        };
        let Some(sender) = sender else {
            return Err(closed());
        };
        let (reply_tx, reply_rx) = oneshot::channel();
        if sender.send(make(reply_tx)).await.is_err() {
            return Err(closed());
        }
        reply_rx.await.unwrap_or_else(|_| Err(closed()))
    }

    async fn lease(&self, cancel: &CancellationToken) -> Result<Object, ResourceError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ResourceError::Cancelled),
            got = self.pool.get() => got.map_err(|e| ResourceError::Operation { code: "pool_error".into(), message: self.pool_error_text(&e), proof: TerminalProof::Terminal }),
        }
    }

    /// Runs a migration under runtime ownership: one connection, one
    /// transaction, ledger row in the same transaction; the lease follows
    /// C5 exactly like a query does. Two migrators at once
    /// (two replicas' `migrate` jobs, an operator and CI) serialize on an
    /// advisory lock, and the ledger row is inserted *before* the SQL runs
    /// so the loser's transaction fails on the primary key and its SQL
    /// never executes; that case returns `Ok(false)` (applied by another
    /// migrator), `Ok(true)` means this call applied it.
    pub async fn apply_migration(
        &self,
        name: &str,
        sql: &str,
        checksum: &str,
        cancel: CancellationToken,
    ) -> Result<bool, ResourceError> {
        let object = self.lease(&cancel).await?;
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        let script = format!(
            "BEGIN;\nSELECT pg_advisory_xact_lock({MIGRATION_LOCK});\nINSERT INTO usai_migrations (name, checksum) VALUES ({}, {});\n{sql}\n;COMMIT;",
            quote_literal(name),
            quote_literal(checksum)
        );
        let finished = {
            let client = lease.client();
            run_cancellable(client, &cancel, &self.counters, &self.tls, async {
                match client.batch_execute(&script).await {
                    Ok(()) => Ok(true),
                    Err(e) => {
                        // The transaction is aborted; roll back explicitly so the
                        // connection is clean before it is judged reusable.
                        let _ = client.batch_execute("ROLLBACK").await;
                        if e.code() == Some(&tokio_postgres::error::SqlState::UNIQUE_VIOLATION)
                            && e.as_db_error()
                                .and_then(|d| d.table())
                                .is_some_and(|t| t == "usai_migrations")
                        {
                            Ok(false)
                        } else {
                            Err(e)
                        }
                    }
                }
            })
            .await
        };
        match finished {
            Finished::Terminal(result) => {
                if !matches!(&result, Err(ResourceError::Operation { proof: TerminalProof::Ambiguous, .. })) {
                    lease.mark_terminal();
                }
                result
            }
            Finished::Ambiguous => Err(ResourceError::Operation {
                code: "cancel_unconfirmed".into(),
                message: "the migration did not reach a terminal state after cancellation; connection quarantined".into(),
                proof: TerminalProof::Ambiguous,
            }),
        }
    }

    pub async fn ensure_migration_ledger(
        &self,
        cancel: CancellationToken,
    ) -> Result<(), ResourceError> {
        let object = self.lease(&cancel).await?;
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        // Serialized on the migration lock: `CREATE TABLE IF NOT EXISTS`
        // is not race-free on its own (two sessions pass the existence
        // check; the loser gets 42P07 / a 23505 on pg_type).
        let result = lease
            .client()
            .batch_execute(&format!("BEGIN; SELECT pg_advisory_xact_lock({MIGRATION_LOCK}); CREATE TABLE IF NOT EXISTS usai_migrations (name text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now()); COMMIT;"))
            .await
            .map_err(sql_error);
        if result.is_err() {
            let _ = lease.client().batch_execute("ROLLBACK").await;
        }
        if !matches!(
            &result,
            Err(ResourceError::Operation {
                proof: TerminalProof::Ambiguous,
                ..
            })
        ) {
            lease.mark_terminal();
        }
        result
    }

    pub async fn applied_migrations(
        &self,
        cancel: CancellationToken,
    ) -> Result<Vec<AppliedMigration>, ResourceError> {
        let object = self.lease(&cancel).await?;
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: Arc::clone(&self.counters),
        };
        let result = lease
            .client()
            .query(
                "SELECT name, checksum, applied_at FROM usai_migrations ORDER BY applied_at, name",
                &[],
            )
            .await
            .map_err(sql_error)
            .map(|rows| {
                rows.iter()
                    .map(|r| AppliedMigration {
                        name: r.get(0),
                        checksum: r.get(1),
                        applied_at: r.get::<_, chrono::DateTime<chrono::Utc>>(2).to_rfc3339(),
                    })
                    .collect()
            });
        if !matches!(
            &result,
            Err(ResourceError::Operation {
                proof: TerminalProof::Ambiguous,
                ..
            })
        ) {
            lease.mark_terminal();
        }
        result
    }
}

/// Owns a pinned connection for the life of one transaction. Exits on
/// commit/rollback, on a lost connection, or on the world's cancellation —
/// in that last case after rolling back for the world.
async fn hold_transaction(
    mut lease: Lease,
    mut commands: mpsc::Receiver<TxCommand>,
    cancel: CancellationToken,
    counters: &Counters,
    tls: &Tls,
) {
    loop {
        let command = tokio::select! {
            biased;
            _ = cancel.cancelled() => None,
            command = commands.recv() => command,
        };
        match command {
            None => {
                // The world is gone (or dropped its handle) with the
                // transaction open: roll back on its behalf. Only a
                // rollback that reached the server proves the connection
                // clean.
                counters
                    .rolled_back_for_world
                    .fetch_add(1, Ordering::SeqCst);
                let client = lease.client();
                if let Ok(Ok(())) =
                    tokio::time::timeout(CANCEL_TERMINAL_BOUND, client.batch_execute("ROLLBACK"))
                        .await
                {
                    lease.mark_terminal();
                }
                return;
            }
            Some(TxCommand::Sql {
                method,
                request,
                reply,
            }) => {
                let finished = transaction_statement(
                    lease.client(),
                    &cancel,
                    counters,
                    tls,
                    &method,
                    &request,
                )
                .await;
                match finished {
                    Finished::Terminal(result) => {
                        let lost = matches!(
                            &result,
                            Err(ResourceError::Operation {
                                proof: TerminalProof::Ambiguous,
                                ..
                            })
                        );
                        let _ = reply.send(result);
                        if lost {
                            return; // quarantined by the lease's drop
                        }
                    }
                    Finished::Ambiguous => {
                        let _ = reply.send(Err(ambiguous_after_cancel()));
                        return;
                    }
                }
            }
            Some(TxCommand::End { commit, reply }) => {
                let statement = if commit { "COMMIT" } else { "ROLLBACK" };
                let client = lease.client();
                let finished = run_cancellable(client, &cancel, counters, tls, async {
                    client.batch_execute(statement).await
                })
                .await;
                let result = match finished {
                    Finished::Terminal(Ok(())) => {
                        lease.mark_terminal();
                        Ok(Value::Bool(true))
                    }
                    Finished::Terminal(Err(e)) => {
                        // A failed COMMIT leaves the connection in a known
                        // state (the transaction is aborted or gone); only a
                        // lost connection is ambiguous.
                        if !matches!(
                            &e,
                            ResourceError::Operation {
                                proof: TerminalProof::Ambiguous,
                                ..
                            }
                        ) {
                            let _ = client.batch_execute("ROLLBACK").await;
                            lease.mark_terminal();
                        }
                        Err(e)
                    }
                    Finished::Ambiguous => Err(ambiguous_after_cancel()),
                };
                let _ = reply.send(result);
                return;
            }
        }
    }
}

/// Advisory lock key that serializes migrators on one database
/// (`SELECT pg_advisory_xact_lock(…)`); arbitrary, stable.
const MIGRATION_LOCK: i64 = 0x7573_6169_6d69_6772; // "usaimigr"

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
