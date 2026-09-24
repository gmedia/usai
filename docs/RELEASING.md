# Releasing

The workflow (`.github/workflows/release.yml`) builds the binaries, publishes
the npm packages and pushes both images when a `v*` tag lands. It enforces
two of the things below and **cannot** enforce the rest, which is why this
page exists.

## The version lives in four places

| File | Field | Enforced by CI? |
|---|---|---|
| `Cargo.toml` | `version` | **yes** — must equal the tag |
| `packages/usai/package.json` | `version` | **yes** — must equal the tag |
| `packages/create-usai/package.json` | `sdkVersion` | **yes** — must equal the SDK version |
| `packages/create-usai/package.json` | `version` | **no** — see below |

### The one CI cannot catch

`create-usai` has its own version cadence, and the publish step **skips a
version already on the registry**:

```
if npm view "$name@$version" version >/dev/null 2>&1; then
  echo "$name@$version is already on the registry; skipping"
```

So bumping only `sdkVersion` publishes nothing, and the registry keeps
serving the previous `create-usai` — which pins the previous SDK. npm's caret
is exact in the `0.0.x` range:

```
^0.0.9  matches 0.0.9   -> true
^0.0.9  matches 0.0.10  -> false
```

**A scaffold from the registry would hand a new user the old SDK**, which is
the first command anyone runs. Bump `create-usai`'s own `version` in the same
commit as `sdkVersion`, every time.

## The order

1. `docs/STATUS.md` and `CHANGELOG.md` describe the release. The notes are
   read by operators who plan an upgrade from them alone — round 29 found a
   false claim in them that would have split one application into two
   mid-rollout, so **every claim is something that has been run**, and a
   change that is not in the notes is a change someone is surprised by.
2. Bump the four fields above. `usai --version` and `builtWith` come from
   them, so before the bump every artifact built from `main` records the
   *previous* version (`SUPPORTED.md` → Versioning says so).
3. `make check` **with a database** — the PostgreSQL suites skip silently
   without one, so a green check on a laptop is not a green check:
   ```bash
   eval "$(scripts/dev-postgres.sh start)" && make check
   ```
4. `make verify-envelope` on a box with Docker and a memory limit. It
   **skips and exits 0** when a prerequisite is missing, so read its output:
   a skipped check is not a passed one. It asserts the memory floors
   `SUPPORTED.md` publishes, which is the number an adopter sizes on.
5. Tag and push: `git tag -a v0.0.x -m "…" && git push origin v0.0.x`.
6. **Rehearse from the registry, not from the checkout.** The published
   packages are what a stranger meets:
   ```bash
   pnpm dlx @sakaladev/create-usai /tmp/rehearsal
   cd /tmp/rehearsal && grep '@sakaladev/usai' package.json   # the new version?
   pnpm install && pnpm test
   ```
7. Re-sync anything that mirrors these documents (the website's
   `pnpm sync:docs`), or it publishes the previous release's corrections.

## What the tag message carries

The annotated tag is the release's own summary; GitHub's auto-notes list the
commits. Draft it at the end of the `CHANGELOG.md` section for that version.
