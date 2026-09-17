//! The persistent runtime.
//!
//! Owns the engine, the ownership ledger, resource managers, and the set of
//! application revisions. Worlds come and go; nothing here does.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::admission::{Budget, BudgetExhausted, Permit};
use crate::definition::ApplicationDefinition;
use crate::engine::{Compiled, Engine, EngineError};
use crate::host_ops::{OpExtensions, OpHandler};
use crate::ownership::{GaugeSnapshot, Ledger};
use crate::resource::{BoundResources, ResourceError, ResourceRegistry, ResourceStatus};
use crate::workloads::{cron, tasks};
use crate::world::{WorkResult, WorldDriver, WorldSpec};

/// Where the runtime reads deployment configuration from (`GOAL.md` §30).
pub type EnvSource = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Total worlds the runtime will hold live at once.
    pub max_worlds: u32,
    /// Default per-application world budget when a revision declares none.
    pub default_app_concurrency: u32,
    /// Default per-invocation deadline for finite work.
    pub default_timeout: Duration,
    /// Bound on one uninterrupted synchronous guest run.
    pub cpu_slice: Duration,
    /// How long `drain` waits before giving up on a revision.
    pub drain_timeout: Duration,
    /// Dispatched tasks that may run at once (ADR-0012).
    pub task_concurrency: u32,
    /// Dispatched tasks that may wait in the queue.
    pub task_queue_capacity: usize,
    /// Whether this instance runs cron schedulers. A deployment concern:
    /// only one instance of an application should tick its crons.
    pub cron_scheduler: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_worlds: 256,
            default_app_concurrency: 256,
            default_timeout: Duration::from_secs(30),
            cpu_slice: Duration::from_secs(5),
            drain_timeout: Duration::from_secs(30),
            task_concurrency: 64,
            task_queue_capacity: 10_000,
            cron_scheduler: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct RevisionId(pub u64);

impl std::fmt::Display for RevisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "rev{}", self.0)
    }
}

/// ADR-0006.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RevisionState {
    Installed,
    Active,
    Draining,
    Retired,
}

impl std::fmt::Debug for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Revision")
            .field("id", &self.id)
            .field("state", &self.state())
            .finish()
    }
}

pub struct Revision {
    pub id: RevisionId,
    pub definition: Arc<ApplicationDefinition>,
    pub compiled: Arc<dyn Compiled>,
    state: RwLock<RevisionState>,
    resources: RwLock<Arc<BoundResources>>,
    env: RwLock<Arc<BTreeMap<String, String>>>,
    app_budget: Arc<Budget>,
    workload_budgets: Vec<Option<Arc<Budget>>>,
    in_flight: AtomicU64,
    settled: Notify,
    cron_stop: Mutex<Option<CancellationToken>>,
    pub cron_stats: Arc<cron::CronStats>,
}

impl Revision {
    pub fn state(&self) -> RevisionState {
        *self.state.read().expect("state poisoned")
    }

    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn resources(&self) -> Arc<BoundResources> {
        Arc::clone(&self.resources.read().expect("resources poisoned"))
    }

    /// The declared environment values resolved at activation. Only names
    /// the application declared are present; the world sees nothing else.
    pub fn env(&self) -> Arc<BTreeMap<String, String>> {
        Arc::clone(&self.env.read().expect("env poisoned"))
    }

    fn set_state(&self, state: RevisionState) {
        *self.state.write().expect("state poisoned") = state;
        if matches!(state, RevisionState::Draining | RevisionState::Retired)
            && let Some(stop) = self.cron_stop.lock().expect("cron poisoned").take()
        {
            stop.cancel();
        }
    }

    /// Counts a queued child (a dispatched task) so draining waits for it.
    pub(crate) fn retain_for_child(&self) {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn release_child(&self) {
        if self.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.settled.notify_waiters();
        }
    }
}

/// Tracks one admitted unit of work against its revision so draining can
/// wait for it. Dropped by the work's future on every path.
struct InFlight {
    revision: Arc<Revision>,
    _runtime_permit: Permit,
    _app_permit: Permit,
    _workload_permit: Option<Permit>,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if self.revision.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.revision.settled.notify_waiters();
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Engine(#[from] EngineError),
    #[error(transparent)]
    Resource(#[from] ResourceError),
    #[error(transparent)]
    Admission(#[from] BudgetExhausted),
    #[error("unknown revision {0}")]
    UnknownRevision(RevisionId),
    #[error("revision {0} is {1:?}, not active")]
    NotActive(RevisionId, RevisionState),
    #[error("no active revision")]
    NoActiveRevision,
    #[error("unknown workload {0}")]
    UnknownWorkload(String),
    #[error("missing required environment: {0}")]
    MissingEnv(String),
    #[error("invalid environment: {0}")]
    InvalidEnv(String),
    #[error("revision {0} did not drain within {1:?}")]
    DrainTimeout(RevisionId, Duration),
    #[error("invalid definition: {0}")]
    InvalidDefinition(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub engine: &'static str,
    pub gauges: GaugeSnapshot,
    pub tasks: serde_json::Value,
    pub revisions: Vec<RevisionStatus>,
    pub resources: Vec<ResourceStatus>,
    pub worlds_in_use: u32,
    pub worlds_max: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionStatus {
    pub id: RevisionId,
    pub application: String,
    pub identity: String,
    pub state: RevisionState,
    pub in_flight: u64,
}

pub struct Runtime {
    self_ref: Weak<Runtime>,
    config: RuntimeConfig,
    engine: Arc<dyn Engine>,
    ledger: Arc<Ledger>,
    resources: ResourceRegistry,
    extensions: Mutex<OpExtensions>,
    world_budget: Arc<Budget>,
    revisions: RwLock<BTreeMap<RevisionId, Arc<Revision>>>,
    active: RwLock<Option<RevisionId>>,
    next_revision: AtomicU64,
    env: EnvSource,
    shutdown: CancellationToken,
    tasks: Arc<tasks::TaskQueue>,
}

impl Runtime {
    pub fn new(engine: Arc<dyn Engine>, config: RuntimeConfig) -> Arc<Self> {
        Self::with_env(engine, config, |name| std::env::var(name).ok())
    }

    /// Runtime with an explicit environment source, for tests and for hosts
    /// that inject configuration rather than reading the process env.
    pub fn with_env(
        engine: Arc<dyn Engine>,
        config: RuntimeConfig,
        env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Arc<Self> {
        let shutdown = CancellationToken::new();
        Arc::new_cyclic(|weak: &Weak<Runtime>| {
            let tasks = tasks::TaskQueue::start(
                weak.clone(),
                config.task_queue_capacity,
                config.task_concurrency,
                shutdown.clone(),
            );
            let mut extensions = OpExtensions::default();
            for (kind, handler) in tasks::handlers(weak.clone(), Arc::clone(&tasks)) {
                extensions.handlers.insert(kind.to_owned(), handler);
            }
            Self {
                self_ref: weak.clone(),
                world_budget: Budget::new("runtime.worlds", config.max_worlds),
                config,
                engine,
                ledger: Ledger::new(),
                resources: ResourceRegistry::new(),
                extensions: Mutex::new(extensions),
                revisions: RwLock::new(BTreeMap::new()),
                active: RwLock::new(None),
                next_revision: AtomicU64::new(1),
                env: Box::new(env),
                shutdown,
                tasks,
            }
        })
    }

    pub fn tasks(&self) -> &Arc<tasks::TaskQueue> {
        &self.tasks
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub fn ledger(&self) -> &Arc<Ledger> {
        &self.ledger
    }

    pub fn resources(&self) -> &ResourceRegistry {
        &self.resources
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Registers an operation kind (tasks, cron, …). Takes effect for worlds
    /// created afterwards.
    pub fn register_op(&self, kind: &str, handler: Arc<dyn OpHandler>) {
        self.extensions
            .lock()
            .expect("extensions poisoned")
            .handlers
            .insert(kind.to_owned(), handler);
    }

    fn extensions(&self) -> Arc<OpExtensions> {
        let guard = self.extensions.lock().expect("extensions poisoned");
        Arc::new(OpExtensions {
            handlers: guard.handlers.clone(),
        })
    }

    /// Compiles and installs a definition without serving it.
    pub async fn install(
        &self,
        definition: Arc<ApplicationDefinition>,
    ) -> Result<Arc<Revision>, RuntimeError> {
        let compiled = self.engine.compile(&definition).await?;
        let id = RevisionId(self.next_revision.fetch_add(1, Ordering::SeqCst));
        let app_budget = Budget::new(
            format!("{}.worlds", definition.name()),
            self.config.default_app_concurrency,
        );
        let workload_budgets = definition
            .workloads()
            .iter()
            .map(|w| {
                w.max_concurrency
                    .map(|n| Budget::new(format!("{}.worlds", w.id), n))
            })
            .collect();
        let revision = Arc::new(Revision {
            id,
            definition,
            compiled,
            state: RwLock::new(RevisionState::Installed),
            resources: RwLock::new(Arc::new(BoundResources::default())),
            env: RwLock::new(Arc::new(BTreeMap::new())),
            app_budget,
            workload_budgets,
            in_flight: AtomicU64::new(0),
            settled: Notify::new(),
            cron_stop: Mutex::new(None),
            cron_stats: Arc::new(cron::CronStats::default()),
        });
        cron::validate(&revision).map_err(|e| RuntimeError::InvalidDefinition(e.to_string()))?;
        self.revisions
            .write()
            .expect("revisions poisoned")
            .insert(id, Arc::clone(&revision));
        tracing::info!(revision = %id, application = revision.definition.name(), identity = revision.definition.identity(), "revision installed");
        Ok(revision)
    }

    /// Validates environment, binds resources, and makes the revision the one
    /// that serves work. A failure leaves the previously active revision
    /// untouched (ADR-0006).
    pub async fn activate(&self, id: RevisionId) -> Result<Arc<Revision>, RuntimeError> {
        let revision = self.revision(id)?;
        let mut env = BTreeMap::new();
        for requirement in &revision.definition.manifest().env {
            match (self.env)(&requirement.name).filter(|v| !v.is_empty()) {
                Some(value) => {
                    crate::definition::validate_env(requirement, &value)
                        .map_err(RuntimeError::InvalidEnv)?;
                    env.insert(requirement.name.clone(), value);
                }
                None if requirement.required => {
                    return Err(RuntimeError::MissingEnv(requirement.name.clone()));
                }
                None => {}
            }
        }
        *revision.env.write().expect("env poisoned") = Arc::new(env);
        let mut bound = BoundResources::default();
        for spec in revision.definition.resources() {
            let env = &self.env;
            let manager = self.resources.open(spec, &|name| env(name)).await?;
            bound.bind(&spec.name, manager);
        }
        *revision.resources.write().expect("resources poisoned") = Arc::new(bound);

        let previous = {
            let mut active = self.active.write().expect("active poisoned");
            let previous = active.replace(id);
            revision.set_state(RevisionState::Active);
            previous
        };
        tracing::info!(revision = %id, "revision active");
        if self.config.cron_scheduler {
            let stop = cron::start(
                self.self_ref.clone(),
                Arc::clone(&revision),
                Arc::clone(&revision.cron_stats),
            );
            *revision.cron_stop.lock().expect("cron poisoned") = Some(stop);
        }
        if let Some(previous) = previous
            && previous != id
            && let Ok(old) = self.revision(previous)
        {
            old.set_state(RevisionState::Draining);
            tracing::info!(revision = %previous, "revision draining");
        }
        Ok(revision)
    }

    /// Waits until a draining (or still-active, which it first marks
    /// draining) revision has no in-flight work, then retires it.
    pub async fn drain(&self, id: RevisionId) -> Result<(), RuntimeError> {
        let revision = self.revision(id)?;
        {
            let mut active = self.active.write().expect("active poisoned");
            if *active == Some(id) {
                *active = None;
            }
        }
        if revision.state() == RevisionState::Active {
            revision.set_state(RevisionState::Draining);
        }
        let wait = async {
            loop {
                if revision.in_flight() == 0 {
                    break;
                }
                revision.settled.notified().await;
            }
        };
        if tokio::time::timeout(self.config.drain_timeout, wait)
            .await
            .is_err()
        {
            return Err(RuntimeError::DrainTimeout(id, self.config.drain_timeout));
        }
        revision.set_state(RevisionState::Retired);
        self.revisions
            .write()
            .expect("revisions poisoned")
            .remove(&id);
        tracing::info!(revision = %id, "revision retired");
        Ok(())
    }

    pub fn revision(&self, id: RevisionId) -> Result<Arc<Revision>, RuntimeError> {
        self.revisions
            .read()
            .expect("revisions poisoned")
            .get(&id)
            .cloned()
            .ok_or(RuntimeError::UnknownRevision(id))
    }

    pub fn active(&self) -> Result<Arc<Revision>, RuntimeError> {
        let id = self
            .active
            .read()
            .expect("active poisoned")
            .ok_or(RuntimeError::NoActiveRevision)?;
        self.revision(id)
    }

    /// Admits one unit of work: checks budgets runtime -> application ->
    /// workload and counts it against the revision. No world exists yet.
    pub fn admit(
        &self,
        revision: &Arc<Revision>,
        workload_id: &str,
    ) -> Result<Admission, RuntimeError> {
        if revision.state() != RevisionState::Active {
            return Err(RuntimeError::NotActive(revision.id, revision.state()));
        }
        let (index, _) = revision
            .definition
            .workload(workload_id)
            .ok_or_else(|| RuntimeError::UnknownWorkload(workload_id.to_owned()))?;
        let runtime_permit = self.world_budget.try_acquire()?;
        let app_permit = revision.app_budget.try_acquire()?;
        let workload_permit = match &revision.workload_budgets[index] {
            Some(budget) => Some(budget.try_acquire()?),
            None => None,
        };
        revision.in_flight.fetch_add(1, Ordering::SeqCst);
        Ok(Admission {
            revision: Arc::clone(revision),
            workload_index: index,
            in_flight: InFlight {
                revision: Arc::clone(revision),
                _runtime_permit: runtime_permit,
                _app_permit: app_permit,
                _workload_permit: workload_permit,
            },
        })
    }

    /// Admission for child work (tasks, cron ticks) that belongs to a revision
    /// which may already be draining: the revision still owns it and drain
    /// waits for it. Runtime and application budgets still apply.
    pub fn admit_child(
        &self,
        revision: &Arc<Revision>,
        workload_id: &str,
    ) -> Result<Admission, RuntimeError> {
        match revision.state() {
            RevisionState::Active | RevisionState::Draining => {}
            state => return Err(RuntimeError::NotActive(revision.id, state)),
        }
        let (index, _) = revision
            .definition
            .workload(workload_id)
            .ok_or_else(|| RuntimeError::UnknownWorkload(workload_id.to_owned()))?;
        let runtime_permit = self.world_budget.try_acquire()?;
        let app_permit = revision.app_budget.try_acquire()?;
        let workload_permit = match &revision.workload_budgets[index] {
            Some(budget) => Some(budget.try_acquire()?),
            None => None,
        };
        revision.in_flight.fetch_add(1, Ordering::SeqCst);
        Ok(Admission {
            revision: Arc::clone(revision),
            workload_index: index,
            in_flight: InFlight {
                revision: Arc::clone(revision),
                _runtime_permit: runtime_permit,
                _app_permit: app_permit,
                _workload_permit: workload_permit,
            },
        })
    }

    /// Runs one cron invocation now, in a fresh world, without waiting for
    /// the wall clock (tests, `usai cron run`).
    pub async fn run_cron(&self, name: &str) -> Result<WorkResult, RuntimeError> {
        let revision = self.active()?;
        cron::run_tick(
            self,
            &revision,
            &format!("cron:{name}"),
            chrono::Utc::now(),
            CancellationToken::new(),
        )
        .await
        .map_err(RuntimeError::InvalidDefinition)
    }

    /// Runs a user-defined command in a fresh finite world (`usai app <name>`).
    pub async fn run_command(
        &self,
        name: &str,
        args: Vec<String>,
    ) -> Result<WorkResult, RuntimeError> {
        let revision = self.active()?;
        let admission = self.admit(&revision, &format!("command:{name}"))?;
        let input =
            crate::workloads::input(&revision, "command", serde_json::json!({ "args": args }));
        self.execute(admission, input, CancellationToken::new())
            .await
    }

    /// Runs a task directly (tests, tooling). Equivalent to an owned invoke
    /// with no parent.
    pub async fn run_task(
        &self,
        name: &str,
        input: serde_json::Value,
    ) -> Result<WorkResult, RuntimeError> {
        let revision = self.active()?;
        let admission = self.admit(&revision, &format!("task:{name}"))?;
        let input =
            crate::workloads::input(&revision, "task", serde_json::json!({ "input": input }));
        self.execute(admission, input, CancellationToken::new())
            .await
    }

    /// Creates a world for admitted work and drives it to its terminal state.
    pub async fn execute(
        &self,
        admission: Admission,
        mut input: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<WorkResult, RuntimeError> {
        let Admission {
            revision,
            workload_index,
            in_flight,
        } = admission;
        // The world always sees the revision's resolved environment,
        // whatever the caller put in the envelope.
        if let serde_json::Value::Object(envelope) = &mut input {
            envelope.insert(
                "env".into(),
                serde_json::to_value(&*revision.env()).expect("env serializes"),
            );
        }
        let workload = revision
            .definition
            .workload_by_index(workload_index)
            .expect("admitted index");
        let deadline = match workload.lifetime() {
            crate::definition::LifetimeFamily::Finite => Some(
                workload
                    .timeout_ms
                    .map(Duration::from_millis)
                    .unwrap_or(self.config.default_timeout),
            ),
            _ => None,
        };
        let cancel = {
            let token = self.shutdown.child_token();
            let external = cancel;
            let merged = token.child_token();
            let m = merged.clone();
            tokio::spawn(async move {
                external.cancelled().await;
                m.cancel();
            });
            merged
        };
        let driver = WorldDriver::create(
            self.engine.as_ref(),
            Arc::clone(&self.ledger),
            WorldSpec {
                definition: Arc::clone(&revision.definition),
                compiled: Arc::clone(&revision.compiled),
                workload_index,
                resources: revision.resources(),
                extensions: self.extensions(),
                deadline,
                cpu_slice: self.config.cpu_slice,
                cancel,
                revision: Some(Arc::clone(&revision)),
            },
        )
        .await?;
        let result = driver.run(&input).await;
        drop(in_flight);
        Ok(result)
    }

    /// Convenience for tests and tooling: admit + execute against the active
    /// revision.
    pub async fn invoke(
        &self,
        workload_id: &str,
        input: serde_json::Value,
    ) -> Result<WorkResult, RuntimeError> {
        let revision = self.active()?;
        let admission = self.admit(&revision, workload_id)?;
        self.execute(admission, input, CancellationToken::new())
            .await
    }

    pub fn status(&self) -> RuntimeStatus {
        let revisions = self
            .revisions
            .read()
            .expect("revisions poisoned")
            .values()
            .map(|r| RevisionStatus {
                id: r.id,
                application: r.definition.name().to_owned(),
                identity: r.definition.identity(),
                state: r.state(),
                in_flight: r.in_flight(),
            })
            .collect();
        RuntimeStatus {
            engine: self.engine.name(),
            gauges: self.ledger.gauges.snapshot(),
            tasks: self.tasks.status(),
            revisions,
            resources: self.resources.statuses(),
            worlds_in_use: self.world_budget.in_use(),
            worlds_max: self.world_budget.max(),
        }
    }

    /// Stops admitting, cancels live worlds, drains, and shuts resources.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let ids: Vec<RevisionId> = self
            .revisions
            .read()
            .expect("revisions poisoned")
            .keys()
            .copied()
            .collect();
        for id in ids {
            let _ = self.drain(id).await;
        }
        self.resources.shutdown().await;
    }
}

/// Proof that work was admitted. Must be consumed by `execute` or dropped;
/// either way the budgets are returned.
impl std::fmt::Debug for Admission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Admission")
            .field("revision", &self.revision.id)
            .field("workload_index", &self.workload_index)
            .finish()
    }
}

pub struct Admission {
    revision: Arc<Revision>,
    workload_index: usize,
    in_flight: InFlight,
}

impl Admission {
    pub fn revision(&self) -> &Arc<Revision> {
        &self.revision
    }

    pub fn workload_index(&self) -> usize {
        self.workload_index
    }
}
