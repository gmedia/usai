# Memory pressure / OOM

The runtime's resident set is dominated by the world pool: each of the
`--max-worlds` slots keeps up to `USAI_WASM_KEEP_RESIDENT` (8 MB) of memory
resident after use, so the plateau is roughly `max_worlds × keep_resident`
plus the compiled images (~30 MB per held revision) plus the host itself.
The P6 soak reports the actual plateau for a workload (`rssMax` in the
soak samples).

## What you see when the limit is too low

`docker inspect` shows `OOMKilled: true`; the log has no error before a
new `wasm engine` line (the kernel killed the process); the proxy answers
502 while it restarts, and with a restart policy this repeats. Measured: a
96 MB limit on a 256-world runtime → restart loop every ~3 s.

## What to do

Raise the limit to ≥ 512 MB for 256 worlds, or lower `--max-worlds`. A
memory-limited instance with fewer worlds refuses with 503
`capacity_exhausted` instead of dying — the right failure. Watch
`usai_worlds_live` and the container's RSS together: RSS that keeps rising
while `usai_worlds_live` does not is a leak — report it with the soak
samples.
