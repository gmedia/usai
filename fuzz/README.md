# Coverage-guided fuzzing

libFuzzer targets (`cargo fuzz`, nightly) for the boundaries where trust
changes hands and a parser is hand-written or the input comes from disk or
from the guest:

| Target | Input | Exercises |
|---|---|---|
| `manifest` | `manifest.json` bytes | `Manifest` deserialization → `ApplicationDefinition::new` → OpenAPI (both profiles) → graph → router path conversion and insertion → cron schedule parsing → env validation |
| `http_boundary` | `query␟schema␟guest-output` (US-separated) | query string → JSON, scalar coercion against a schema, the guest's HTTP output rendered as a response (headers, body encodings) |
| `sourcemap` | `app.js.map` bytes | source map parsing (VLQ), position lookup, stack mapping |

The control API and artifact loading are covered by the mutation suite in
`crates/usai-runtime/tests/robustness.rs` (600 malformed artifacts, 600
malformed requests); these targets go deeper on the parsers.

```bash
rustup toolchain install nightly && cargo +nightly install cargo-fuzz
cargo +nightly fuzz run manifest -- -max_total_time=300      # seeds in fuzz/corpus/<target> (tracked; what a run adds there is ignored — `git add -f` a new seed)
cargo +nightly fuzz run http_boundary -- -max_total_time=300
cargo +nightly fuzz run sourcemap -- -max_total_time=300
```

CI (`.github/workflows/fuzz.yml`) runs each target for two minutes on
every push that touches `crates/` or `fuzz/`, and for ten minutes nightly;
a crash is uploaded as an artifact with the reproducer.
