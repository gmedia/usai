//! `cache.local`: shared across worlds, local to the runtime, not durable,
//! may disappear on restart (`GOAL.md` §25). The first explicit persistence
//! class, and the one D1 tests use to prove that persistent state is reached
//! only through a declared resource.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{
    ResourceCall, ResourceError, ResourceIdentity, ResourceManager, ResourceProvider,
    ResourceStatus, TerminalProof,
};
use crate::definition::ResourceSpec;

pub struct CacheLocalProvider;

#[async_trait]
impl ResourceProvider for CacheLocalProvider {
    fn kind(&self) -> &str {
        "cache.local"
    }

    fn compat(&self) -> u32 {
        1
    }

    async fn open(
        &self,
        spec: &ResourceSpec,
        identity: ResourceIdentity,
        _env: &(dyn for<'a> Fn(&'a str) -> Option<String> + Sync),
    ) -> Result<Arc<dyn ResourceManager>, ResourceError> {
        let max_entries = spec
            .config
            .get("maxEntries")
            .and_then(Value::as_u64)
            .unwrap_or(10_000) as usize;
        Ok(Arc::new(CacheLocal {
            identity,
            max_entries,
            entries: Mutex::new(HashMap::new()),
        }))
    }
}

struct Entry {
    value: Value,
    expires: Option<Instant>,
}

pub struct CacheLocal {
    identity: ResourceIdentity,
    max_entries: usize,
    entries: Mutex<HashMap<String, Entry>>,
}

fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a Value, ResourceError> {
    args.get(name).ok_or_else(|| ResourceError::Operation {
        code: "invalid_args".into(),
        message: format!("missing argument {name}"),
        proof: TerminalProof::Terminal,
    })
}

#[async_trait]
impl ResourceManager for CacheLocal {
    fn identity(&self) -> &ResourceIdentity {
        &self.identity
    }

    async fn call(
        &self,
        call: ResourceCall,
        _cancel: CancellationToken,
    ) -> Result<Value, ResourceError> {
        // Every operation here is synchronous and in-memory, so it is always
        // terminal; there is no ambiguous path for a local map.
        let mut entries = self.entries.lock().expect("cache poisoned");
        let now = Instant::now();
        match call.method.as_str() {
            "get" => {
                let key = arg(&call.args, "key")?
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                Ok(match entries.get(&key) {
                    Some(entry) if entry.expires.is_none_or(|t| t > now) => entry.value.clone(),
                    Some(_) => {
                        entries.remove(&key);
                        Value::Null
                    }
                    None => Value::Null,
                })
            }
            "set" => {
                let key = arg(&call.args, "key")?
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let value = arg(&call.args, "value")?.clone();
                let ttl = call.args.get("ttlMs").and_then(Value::as_u64);
                if entries.len() >= self.max_entries && !entries.contains_key(&key) {
                    entries.retain(|_, e| e.expires.is_none_or(|t| t > now));
                    if entries.len() >= self.max_entries {
                        return Err(ResourceError::Operation {
                            code: "cache_full".into(),
                            message: format!(
                                "cache.local {} holds {} entries",
                                self.identity.name, self.max_entries
                            ),
                            proof: TerminalProof::Terminal,
                        });
                    }
                }
                entries.insert(
                    key,
                    Entry {
                        value,
                        expires: ttl.map(|ms| now + Duration::from_millis(ms)),
                    },
                );
                Ok(Value::Bool(true))
            }
            "delete" => {
                let key = arg(&call.args, "key")?
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                Ok(Value::Bool(entries.remove(&key).is_some()))
            }
            "increment" => {
                let key = arg(&call.args, "key")?
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let by = call.args.get("by").and_then(Value::as_i64).unwrap_or(1);
                let current = entries
                    .get(&key)
                    .filter(|e| e.expires.is_none_or(|t| t > now))
                    .and_then(|e| e.value.as_i64())
                    .unwrap_or(0);
                let next = current + by;
                entries.insert(
                    key,
                    Entry {
                        value: json!(next),
                        expires: None,
                    },
                );
                Ok(json!(next))
            }
            "clear" => {
                entries.clear();
                Ok(Value::Bool(true))
            }
            other => Err(ResourceError::UnknownMethod {
                resource: self.identity.name.clone(),
                method: other.to_owned(),
            }),
        }
    }

    fn status(&self) -> ResourceStatus {
        let entries = self.entries.lock().expect("cache poisoned");
        let mut detail = BTreeMap::new();
        detail.insert("entries".into(), json!(entries.len()));
        ResourceStatus {
            identity: self.identity.clone(),
            ready: true,
            in_use: 0,
            max: self.max_entries as u32,
            quarantined: 0,
            detail,
        }
    }

    async fn shutdown(&self) {
        self.entries.lock().expect("cache poisoned").clear();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
