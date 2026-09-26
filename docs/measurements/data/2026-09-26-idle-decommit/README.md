# Raw runs for `../../2026-09-26-idle-decommit.md`

VM 47, 2026-09-26, release build at `07073c0`, contract-heavy class
(`POST /orders/quote`), 32 keep-alive clients, 8 s, `--max-worlds 48`.

| file | run |
|---|---|
| `smoke2.txt` | `keep_resident=8m`, release on with the 64 MiB floor — never fires |
| `floor0.txt` | `keep_resident=8m`, floor 0, 5 s quiet period — fires |
| `floor0b.txt` | the same again (reproducibility: within 0.4 MiB) |
| `kr0.txt` | `keep_resident=0` control — the 60 MiB the plateau is made of |

`idle-plateau.sh` runs one cell: start the app, burst, sample
`/_usai/status` every 10 s while idle, then time the first requests after the
quiet period. `burst.mjs` is the load client (N keep-alive clients on one
path; a curl-per-request loop cannot reach N in flight, so it cannot touch N
slots). `wake.mjs` times the three requests after the idle window on one
connection. `quote.json` is the class body.

```bash
PORT=3341 DUR=8 IDLE=40 FLOOR=0 bash idle-plateau.sh 5000 server.log
PORT=3351 DUR=8 IDLE=20 KEEP_RESIDENT=0 bash idle-plateau.sh 0 server.log
```

`DATABASE_URL` defaults to the bench PostgreSQL on the VM; the class measured
here does not touch it, but the app declares the resource and activation
requires it.
