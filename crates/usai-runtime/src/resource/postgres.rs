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
//! Backend identity and prepared statements belong to the physical
//! connection (established once per connection), never to a world. Session
//! state (`SET`, advisory locks, temp tables) is reset on every checkout by
//! default so it cannot leak between worlds either.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use deadpool_postgres::{Manager, ManagerConfig, Object, Pool, RecyclingMethod};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{NoTls, Row, Statement};
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
        let manager = Manager::from_config(pg_config, NoTls, ManagerConfig { recycling_method });
        let max = config.pool.max.unwrap_or(16).max(1);
        let pool = Pool::builder(manager)
            .max_size(max)
            .build()
            .map_err(|e| ResourceError::Startup(spec.name.clone(), e.to_string()))?;
        // Fail activation, not the first request, when the database is
        // unreachable (`GOAL.md` §32).
        let probe = pool.get().await.map_err(|e| {
            ResourceError::Startup(spec.name.clone(), format!("cannot connect: {e}"))
        })?;
        drop(probe);
        Ok(Arc::new(Postgres {
            identity,
            pool,
            max: max as u32,
            counters: Counters::default(),
        }))
    }
}

#[derive(Default)]
struct Counters {
    operations: AtomicU64,
    returned: AtomicU64,
    quarantined: AtomicU64,
    cancelled: AtomicU64,
}

pub struct Postgres {
    identity: ResourceIdentity,
    pool: Pool,
    max: u32,
    counters: Counters,
}

/// Owns one pooled connection for the duration of one operation. Dropped
/// without terminal proof, it removes the connection from the pool: an
/// abandoned operation never leaves a reusable-looking connection behind.
struct Lease<'a> {
    object: Option<Object>,
    terminal: bool,
    counters: &'a Counters,
}

impl Lease<'_> {
    fn client(&self) -> &deadpool_postgres::ClientWrapper {
        self.object.as_ref().expect("lease holds its connection")
    }

    /// Only a known terminal protocol outcome may mark the connection
    /// reusable.
    fn mark_terminal(&mut self) {
        self.terminal = true;
    }
}

impl Drop for Lease<'_> {
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
}

enum Finished<T> {
    Terminal(T),
    Ambiguous,
}

fn sql_error(error: tokio_postgres::Error) -> ResourceError {
    if let Some(db) = error.as_db_error() {
        ResourceError::Operation {
            code: format!("sql_{}", db.code().code().to_ascii_lowercase()),
            message: db.message().to_owned(),
            proof: TerminalProof::Terminal,
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
            let _ = cancel_token.cancel_query(NoTls).await;
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

fn param_error(index: usize, ty: &Type, detail: impl std::fmt::Display) -> ResourceError {
    ResourceError::Operation {
        code: "invalid_param".into(),
        message: format!("parameter ${} ({}): {detail}", index + 1, ty.name()),
        proof: TerminalProof::Terminal,
    }
}

/// Converts one JSON parameter to the type the prepared statement expects.
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
        Type::INT2 => typed!(i16, value.as_i64().and_then(|n| i16::try_from(n).ok())),
        Type::INT4 => typed!(i32, value.as_i64().and_then(|n| i32::try_from(n).ok())),
        Type::INT8 => typed!(i64, value.as_i64()),
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
        Type::JSON | Type::JSONB => Ok(Box::new(value.clone())),
        Type::TIMESTAMPTZ => typed!(
            chrono::DateTime<chrono::Utc>,
            value
                .as_str()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&chrono::Utc))
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
        _ => Err(param_error(
            index,
            ty,
            "unsupported parameter type; cast it in SQL, e.g. $1::text",
        )),
    }
}

fn column_to_json(row: &Row, index: usize, ty: &Type) -> Result<Value, tokio_postgres::Error> {
    macro_rules! get {
        ($t:ty) => {
            row.try_get::<_, Option<$t>>(index).map(|v| json!(v))
        };
    }
    match *ty {
        Type::INT2 => get!(i16),
        Type::INT4 => get!(i32),
        Type::INT8 => get!(i64),
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
        Type::INT8_ARRAY => get!(Vec<i64>),
        Type::VOID => Ok(Value::Null),
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
        let request: SqlRequest =
            serde_json::from_value(call.args).map_err(|e| ResourceError::Operation {
                code: "invalid_args".into(),
                message: e.to_string(),
                proof: TerminalProof::Terminal,
            })?;
        self.counters.operations.fetch_add(1, Ordering::SeqCst);
        let object = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(ResourceError::Cancelled),
            got = self.pool.get() => got.map_err(|e| match e {
                deadpool_postgres::PoolError::Timeout(_) => ResourceError::Exhausted { resource: self.identity.name.clone() },
                other => ResourceError::Operation { code: "pool_error".into(), message: other.to_string(), proof: TerminalProof::Terminal },
            })?,
        };
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: &self.counters,
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
                    run_cancellable(client, &cancel, &self.counters, async {
                        let rows = client.query(&statement, &refs).await?;
                        rows.iter()
                            .map(row_to_json)
                            .collect::<Result<Vec<_>, _>>()
                            .map(Value::Array)
                    })
                    .await
                }
                "one" => {
                    run_cancellable(client, &cancel, &self.counters, async {
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
                    run_cancellable(client, &cancel, &self.counters, async {
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

    fn status(&self) -> ResourceStatus {
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
        detail.insert("poolSize".into(), json!(status.size));
        detail.insert("available".into(), json!(status.available));
        detail.insert("waiting".into(), json!(status.waiting));
        ResourceStatus {
            identity: self.identity.clone(),
            ready: true,
            in_use: (status.size.saturating_sub(status.available)) as u32,
            max: self.max,
            quarantined: self.counters.quarantined.load(Ordering::SeqCst),
            detail,
        }
    }

    async fn shutdown(&self) {
        self.pool.close();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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

impl Postgres {
    async fn lease(&self, cancel: &CancellationToken) -> Result<Object, ResourceError> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err(ResourceError::Cancelled),
            got = self.pool.get() => got.map_err(|e| ResourceError::Operation { code: "pool_error".into(), message: e.to_string(), proof: TerminalProof::Terminal }),
        }
    }

    /// Runs a multi-statement script under runtime ownership (migrations):
    /// one connection, one transaction, ledger row in the same transaction.
    /// The lease follows C5 exactly like a query does.
    pub async fn apply_migration(
        &self,
        name: &str,
        sql: &str,
        checksum: &str,
        cancel: CancellationToken,
    ) -> Result<(), ResourceError> {
        let object = self.lease(&cancel).await?;
        let mut lease = Lease {
            object: Some(object),
            terminal: false,
            counters: &self.counters,
        };
        let script = format!(
            "BEGIN;\n{sql}\n;INSERT INTO usai_migrations (name, checksum) VALUES ({}, {});\nCOMMIT;",
            quote_literal(name),
            quote_literal(checksum)
        );
        let finished = {
            let client = lease.client();
            run_cancellable(client, &cancel, &self.counters, async {
                match client.batch_execute(&script).await {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        // The transaction is aborted; roll back explicitly so the
                        // connection is clean before it is judged reusable.
                        let _ = client.batch_execute("ROLLBACK").await;
                        Err(e)
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
            counters: &self.counters,
        };
        let result = lease
            .client()
            .batch_execute("CREATE TABLE IF NOT EXISTS usai_migrations (name text PRIMARY KEY, checksum text NOT NULL, applied_at timestamptz NOT NULL DEFAULT now())")
            .await
            .map_err(sql_error);
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
            counters: &self.counters,
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

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
