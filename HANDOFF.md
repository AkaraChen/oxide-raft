# Handoff

Read `GOAL.md` first, then `docs/migration/README.md`. This file is the
running state; update it at the end of every phase and every session.

## Session setup (cloud container)

1. `git submodule update --init` (upstream sources are submodules; a fresh
   clone has empty `upstream/` dirs).
2. Oracle Node: download Node 24.15.0 (`https://nodejs.org/dist/v24.15.0/`)
   and put its `bin/` first on `PATH` for every oracle/golden/listing command.
   The container's default Node is 22.
3. `tools/oracle.sh setup` and `(cd upstream/oar && pnpm install --frozen-lockfile --ignore-scripts)`.
4. Upstream tests need `TZ=Asia/Shanghai` and a non-root user (D23). The cloud
   container runs as root; creating another user there was refused, so
   root-only failures (two chmod cases) stay in the working-copy Linux list
   until CI regenerates it.
5. Branch: work is pushed to `claude/awesome-cannon-1b7ru0` (the session's
   designated branch). GOAL.md says `master`; merging to `master` needs the
   owner's go-ahead.

## Progress

Phase 1 (commander, raft-shared, raft-trace-client): in progress.

| Unit | State |
|---|---|
| js core (D3) | done, reviewed (js-core-reviewer-a/b) |
| js regex (D3) | implemented; review loop in progress |
| commander (D1) | implementing against `tests/golden/commander/scenarios.json` |
| schema runtime (D2) | implementing against `tests/golden/raft-shared/zod-combinators.json` |
| json_schema (D15) | implementing against `tests/golden/raft-shared/json-schema.json` |
| dates / time zones, collation, stream decoders (D3 deferred) | not started |
| http (D5), process/env/node_compat | not started |
| shared contracts and formatters (43 in-scope files) | not started |
| raft-trace-client (4 files) | not started |

## Gates

| Gate | State |
|---|---|
| 1 types | `cargo check --workspace --all-targets` clean (skeleton crates) |
| 2 link | not attempted |
| 3 start | goldens exist (`tests/golden/raft-cli`, `raft-computer`); binaries not ported |
| 5 suite | 0 upstream tests ported |
| 6 CI | no workflow yet |
| 7 e2e | not started |

## test-parity (macOS list)

| package | in-scope | ported | waived | missing |
|---|---|---|---|---|
| cli | 918 | 0 | 17 | 901 |
| shared | 300 | 0 | 1 | 299 |
| trace-client | 29 | 0 | 0 | 29 |
| computer | 948 | 0 | 429 | 519 |
| daemon | 1663 | 0 | 194 | 1469 |
| oar | 61 | 0 | 8 | 53 |

## Optional review items (registered, not blocking)

None yet for this session.
