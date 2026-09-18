# Acceptance audit — `GOAL.md` §53, item by item

**Audited:** 2026-09-18 at commit a506dda. Legend: ✓ evidence exists and is
automated; ◐ implemented, evidence partial or manual; ✗ gap. Evidence names
are test functions under `crates/usai-runtime/tests/` unless stated. Update
this file when an item moves.

## D0 — Repository foundation

| Acceptance | Status | Evidence |
|---|---|---|
| clean clone builds | ✓ | CI `make check` on ubuntu-24.04 (`.github/workflows/ci.yml`) |
| tests pass with documented commands | ✓ | `make test`; commands in `CLAUDE.md`/`AGENTS.md` |
| no runtime/build dependency on the research repository | ✓ | `Cargo.toml`/`package.json` have none; `vendor/` is Wasmtime, not research code |
| production-oriented, not experiment-oriented | ✓ | ADR-0001; `docs/RESEARCH-REFERENCE.md` maps contracts to evidence only |

## D1 — Lifecycle core

| Acceptance | Status | Evidence |
|---|---|---|
| immutable definition creates multiple worlds | ✓ | `concurrent_worlds_do_not_share_state`, `fresh_world_per_request_and_persistent_resource` |
| world A state invisible in world B | ✓ | `world_a_state_is_not_visible_in_world_b` |
| persistent runtime infrastructure survives world destruction | ✓ | `persistent_resource_survives_world_destruction` |
| ownership returns to baseline | ✓ | `assert_baseline` in every lifecycle test; `shutdown_cancels_live_work_and_returns_to_baseline` |
| abnormal destruction leaves no stale execution rights | ✓ | `cancelled_world_leaves_no_stale_execution_rights`, `deadline_ends_the_world_and_ownership_returns`, `runaway_synchronous_code_is_interrupted` |
| semantics testable without HTTP | ✓ | all of `tests/lifecycle.rs` drives `Runtime::invoke` with a hand-written `__usai_sdk` |

## D2 — HTTP contract workload

| Acceptance | Status | Evidence |
|---|---|---|
| contract-aware endpoint end to end | ✓ | `contract_endpoint_end_to_end`, `query_and_headers_reach_the_handler` |
| invalid boundary input fails before world creation | ✓ | `invalid_boundary_input_fails_before_any_world_exists` |
| fresh world per finite request | ✓ | `fresh_world_per_request_and_persistent_resource` |
| correct response ownership | ✓ | `created_and_no_content_helpers`, `raw_escape_hatch_sees_exact_bytes`, `detached_work_is_reported_and_the_response_still_commits` |
| concurrent requests do not leak state | ✓ | `concurrent_requests_do_not_share_state` |
| cancellation signal / deadline | ✓ | `client_disconnect_cancels_the_world`, `deadline_maps_to_gateway_timeout` |

## D3 — Developer loop

| Acceptance | Status | Evidence |
|---|---|---|
| new developer can clone an example and run it | ✓ | `examples/hello` test via `usai/test` (`examples/hello/test`), `create-usai` test |
| editing a handler reloads safely | ✓ | `crates/usai-cli/tests/dev_reload.rs` — new revision lands, no failed request during the swap, a broken edit keeps the previous revision |
| invalid application definition fails clearly | ✓ | `malformed_artifacts_are_refused_with_clear_errors`; build errors surfaced by `dev_reload` ("build failed … previous revision keeps serving") |
| effective config can be inspected | ✓ | `config_and_module_metadata_compose_deterministically` (`usai config --json` values + sources) |
| topology matches runtime behaviour | ✓ | `manifest_describes_the_application`; inspect/graph/OpenAPI read the executed definition (C8) |

## D4 — Tasks + ownership transfer

| Acceptance | Status | Evidence |
|---|---|---|
| HTTP request can dispatch a task and end safely | ✓ | `http_dispatches_a_task_and_ends_before_it_runs` |
| task receives a separate world; parent state does not leak | ✓ | same test (parent global invisible in the child) |
| ownership transfer explicit and observable | ✓ | `WorkResult.children` relation; `draining_waits_for_dispatched_tasks` |
| dangling async work rejected/cancelled/diagnosed | ✓ | `detached_timer_is_detected_cancelled_and_explained`, `detached_interval_is_detected`, `detached_work_is_reported_and_the_response_still_commits` |
| owned invocation semantics | ✓ | `owned_task_failure_reaches_the_parent_as_a_contract`, `cancelling_the_parent_cancels_the_owned_child`, `task_input_contract_is_validated` |

## D5 — Cron + commands

| Acceptance | Status | Evidence |
|---|---|---|
| cron runs in fresh worlds | ✓ | `cron_runs_in_fresh_worlds_and_can_be_invoked_deterministically` |
| command runs in a fresh finite world | ✓ | `command_runs_in_a_fresh_finite_world` |
| no permanent mutable application process | ✓ | both above (worlds retire; counters do not carry over) |
| tests invoke both without wall clock | ✓ | `Runtime::run_cron` / `run_command`; `cron_scheduler_ticks_and_skips_overlap`, `invalid_cron_schedule_fails_at_install` |

## D6 — PostgreSQL

| Acceptance | Status | Evidence |
|---|---|---|
| reuse only after terminal knowledge | ✓ | `normal_completion_returns_the_connection`, `sql_error_is_terminal_and_the_connection_is_reused`, `ambiguous_abandonment_quarantines_the_connection` |
| no connection-state leak across worlds | ✓ | `no_connection_state_leaks_across_worlds` |
| cancellation covered | ✓ | `cooperative_cancellation_awaits_terminal_state_then_reuses` |
| timeout | ✓ | `cooperative_cancellation_awaits_terminal_state_then_reuses` (`/slow`: 300 ms workload timeout over `pg_sleep(30)` → 504, cancel confirmed by 57014, connection reused) |
| abandoned-world behaviour | ✓ | `ambiguous_abandonment_quarantines_the_connection`, `database_backend_loss_is_quarantined_and_recovered` |
| recovery after abnormal cases | ✓ | `database_backend_loss_is_quarantined_and_recovered`, `pool_exhaustion_is_resource_aware_backpressure` |
| unreachable database → activation fails | ✓ | `unreachable_database_fails_activation_not_the_first_request` |
| TLS | ✓ | `tls_connections_verify_the_server_certificate` (refused without root; `pg_stat_ssl` with it) |

## D7 — Project model

| Acceptance | Status | Evidence |
|---|---|---|
| centralized and colocated layouts | ✓ | `migrations_seeders_and_typed_env_end_to_end` (fixture uses both) |
| organization does not change runtime semantics | ✓ | same fixture runs the same workloads either way |
| module metadata composes deterministically | ✓ | `config_and_module_metadata_compose_deterministically` |
| config declarative and inspectable | ✓ | same; `usai config` |
| missing env fails before serving | ✓ | `typed_env_fails_activation_on_shape_not_first_request` |

## D8 — API metadata + OpenAPI

| Acceptance | Status | Evidence |
|---|---|---|
| docs match runtime behaviour | ✓ | `openapi_is_generated_from_the_definition` (served == generated, from the executed definition) |
| no duplicate endpoint-definition system | ✓ | by construction (`openapi.rs` reads `ApplicationDefinition`) |
| raw endpoints represented as opaque | ✓ | `x-usai-raw`, `x-usai-stream`, `x-usai-socket` in the same test |

## D9 — Service workload

| Acceptance | Status | Evidence |
|---|---|---|
| state persists for the service lifetime | ✓ | `service_state_persists_for_the_service_lifetime_and_stops_gracefully` |
| service lifetime separate from runtime lifetime | ✓ | same (drain stops it; runtime continues) |
| graceful stop returns ownership to baseline | ✓ | same + `a_failing_service_restarts_per_policy_and_then_settles` |
| does not weaken finite-work isolation | ✓ | finite worlds cannot see service state (`persistent_resource_survives_world_destruction` pattern; services are their own worlds) |

## D10 — Queue

| Acceptance | Status | Evidence |
|---|---|---|
| message contract validation (no world for invalid) | ✓ | `queue_messages_run_in_fresh_worlds_with_explicit_retry` |
| bounded concurrency | ✓ | `queue_concurrency_is_bounded` (6 × 300 ms messages at concurrency 2: ≥ 3 rounds, peak live worlds ≤ 2 + probe) |
| per-message world; no state leak | ✓ | same test (three fresh worlds across retries) |
| retry/error semantics explicit | ✓ | same test; ADR-0014 |
| resource reuse follows ownership rules | ✓ | consumer worlds lease through the same `ResourceManager` (`pg_status` baseline assertions) |

## D11 — WebSocket + stream

| Acceptance | Status | Evidence |
|---|---|---|
| connection-local state survives messages, ends with the connection | ✓ | `socket_state_is_connection_local_and_ends_with_the_connection` |
| stream lifetime distinct from request completion | ✓ | `stream_lives_until_the_handler_returns` |
| cancellation/drain correct | ✓ | `client_disconnect_ends_the_stream_world`, `drain_stops_an_endless_stream_gracefully`, `drain_closes_sockets_and_runs_close_handlers`, `idle_sockets_are_closed_by_the_runtime` |
| no process-global connection state | ✓ | per-revision `connections_stop`; sockets are worlds |

## D12 — Observability + graph

| Acceptance | Status | Evidence |
|---|---|---|
| observability derives from runtime truth | ✓ | `status_and_metrics_derive_from_runtime_truth` |
| detailed tracing can be disabled cheaply | ✓ | measured (`USAI_PROFILE_TRACING`): none 0.265 ms / info 0.279 / debug 0.279 / trace 0.286 per empty world |
| ownership/lifetime failures diagnosable | ✓ | teaching diagnostics in `LifecycleViolation`; `usai graph`; world trace record |

## D13 — Production hardening

| Exercise | Status | Evidence |
|---|---|---|
| long-duration soak | ◐ | 1 h at c=16 on the research VM running at audit time (`docs/measurements/…` §8 when complete); RSS flat at 290 MB after 10 min |
| sustained concurrency | ✓ | `usai bench` c=16/64 on the VM: 13.6k / 14.1k req/s, 0 errors |
| overload/backpressure | ✓ | `budget_exhaustion_refuses_promptly_and_recovers`, `pool_exhaustion_is_resource_aware_backpressure`, `admission_is_refused_at_the_boundary_when_budget_is_exhausted` |
| graceful shutdown | ✓ | `shutdown_drains_then_cancels_live_work_and_returns_to_baseline` (found and fixed during this audit: shutdown used to cancel every world before draining) |
| forced shutdown | ✓ | `crates/usai-cli/tests/forced_shutdown.rs` — a service that ignores stop keeps the drain open; a second SIGINT exits 130 and reports live worlds |
| crash/restart recovery | ◐ | queue messages are PostgreSQL-backed (survive a process loss); dispatched in-memory tasks are counted as lost at shutdown (ADR-0010); process supervision is the orchestrator's (D15) |
| bounded memory | ✓ | `a_world_exceeding_its_memory_limit_fails_cleanly`; pooling slots are fixed-size |
| database loss/recovery | ✓ | `database_backend_loss_is_quarantined_and_recovered` |
| malformed artifacts | ✓ | `malformed_artifacts_are_refused_with_clear_errors` |
| artifact/version compatibility | ✓ | unsupported manifest version refused (same test); `MANIFEST_VERSION` |
| revision activation/draining | ✓ | `revision_replacement_under_load_loses_no_request`, `revision_replacement_drains_the_old_revision`, `failed_activation_leaves_the_active_revision_untouched` |
| resource limits | ✓ | memory limit, CPU slice (`runaway_synchronous_code_is_interrupted`), pool max, budgets |
| security/threat model | ◐ | `docs/THREAT-MODEL.md`; per-world CPU accounting still open |
| upgrade/rollback | ✓ | `orchestrator_lifecycle_install_activate_drain_remove` (rollback = reinstall + activate) |

## D14 — Developer preview

| Item | Status | Evidence |
|---|---|---|
| `usai dev` coherent | ✓ | `dev_reload` |
| `usai build` produces a runnable artifact; `usai run` serves it | ✓ | `usai/test` harness spawns `usai run` on the built artifact (`examples/hello/test`) |
| HTTP contract model works | ✓ | D2 |
| task/cron prove multiple finite lifetimes | ✓ | D4/D5 |
| PostgreSQL ownership correct | ✓ | D6 |
| config/project model usable | ✓ | D7 |
| API docs from runtime truth | ✓ | D8 |
| lifecycle integration tests green | ✓ | 79 Rust acceptance tests + 7 TS, both engines |
| observability sufficient to debug failures | ✓ | D12 |
| docs let a new developer build a real application | ◐ | `docs/GUIDE.md` covers every workload kind; no end-to-end tutorial application beyond `examples/hello` and `examples/postgres` |
| published packages / binaries | ◐ | release workflow in place (`.github/workflows/release.yml`); no tag cut yet, npm token not configured |

## D15 — Control surface

| Item | Status | Evidence |
|---|---|---|
| install / activate / inspect / health / drain / stop / remove | ✓ | `orchestrator_lifecycle_install_activate_drain_remove`, `non_loopback_bind_requires_a_token` |
| generally useful protocol | ✓ | plain JSON over HTTP with a bearer token; `usai/test` is its second client |

## Open items (in priority order)

1. Soak result to record (§8 of the measurements doc) — running.
2. Per-world CPU accounting (threat model).
3. First tagged release (`v0.0.1`) and npm token.
4. A tutorial application beyond the examples.
