# oxide-raft migration contract

Port the Raft `raft` CLI, the Raft Computer (`raft-computer`), and the
DaemonCore it hosts from TypeScript to Rust, as one pinned version.

## Pinned sources

| Source | Pin | Package versions |
|---|---|---|
| `upstream/raft-source` | `05f7d8fd77d2535f993d5d90b85118438bc18216` (Release v1.13.0-source.1) | cli `0.0.24`, daemon `1.0.25`, computer `1.0.28` |
| `upstream/oar` | `v0.0.7` (`a1a9857d`) | oar `0.0.7` |

The Rust binaries report these same versions. There is no auto-update; a new
upstream version is a new migration. The oracle runs on Node 24.15.0 (upstream's
`.nvmrc`, also the Node the SEA build embeds); dependency pins are listed in
`mapping-guide.md`.

## Finish line

The Rust `raft` and `raft-computer` binaries replace the TypeScript ones on
macOS, Linux, and Windows:

- `raft` produces the same stdout, stderr, exit codes, HTTP requests, and local
  files as upstream `raft` 0.0.24 for every command in scope.
- `raft-computer` logs in, attaches, runs as a detached background service
  (`decisions.md` D11), and hosts agents
  that the real server (raft.build) can start, message, and stop, using the
  same on-disk state layout as upstream 1.0.28.

## Scope

**In scope**

- `raft` CLI: every command registered in `packages/cli/src/main.ts`, including
  `raft migrate` (`decisions.md` D16).
- DaemonCore (`packages/daemon/src/core.ts` and its transitive closure),
  embedded as a library. Runtimes: claude, codex, gemini, grok, cursor,
  copilot, opencode, kimi (CLI). Runtime account usage via oar (claude, codex,
  kimi, grok). Trace bundle upload stays (`decisions.md` D18).
- `raft-computer`: login, logout, attach, setup, start, stop, restart,
  status, doctor, logs, runners, and the hidden `__service`, `__run`, `__cli`,
  `__print-env`, and `__build-versions`.
- The used closure of `packages/shared`, `packages/trace-client`, and oar.

**Dropped** (delete the code; the observable effect is listed in `decisions.md`)

- Auto-update and release machinery: `upgrade`, `versions`, `operation`,
  `channel` (release channel), `__k-upgrade`, `__installer-converge`,
  `@botiverse/k-carrier`, the hands-node updater, and the installers. The
  resident service keeps upstream's detached-process model (`decisions.md` D11).
- Legacy migration and adoption: `adoptLegacy`, `legacySupervisorTakeover`,
  `legacyKOriginAdoption`, `legacyOsSupervisorMigration`,
  `__legacy-supervisor-takeover`, `__supervisor retire-legacy`, the daemon's
  legacy supervisor and migration guard, legacy-path quarantines. The refusal of
  retired OS-supervisor entries stays (`decisions.md` D11).
- The `slock` CLI alias and the standalone `raft-daemon` binary.
- The `builtin` (pi), `kimi-sdk`, and deprecated `antigravity` runtimes
  (`decisions.md` D17). The SEA host's `runtime-pkg` file
  and `PI_PACKAGE_DIR` stay, because every agent sees them (`decisions.md` D8).
- Computer legacy read fallbacks: `attachment.json` dual-read, `server-runner.*`
  pid/log fallbacks, and the `api.slock.ai` / staging-Fly URL rewrites
  (`decisions.md` D19). The markerless Desktop guard stays (`decisions.md` D20).
- The `lib/` package export surface that only the Electron `raft-computer-app`
  consumes. `lib/` modules that the CLI or the service use (`lib/api.ts`,
  `lib/ipc-client.ts`, and the rest of their closure) are ported.

Scope choices that were open are now D16–D21 in `decisions.md`.

## Gates

Pass them in order. Passing one proves nothing about the next.

1. **Types**: `cargo check --workspace --all-targets` is clean on all three OSes.
2. **Link**: `cargo build --release` produces `raft` and `raft-computer`.
3. **Start**: `raft --help`, `raft-computer --help`, and `raft-computer status`
   run and match the oracle's output, except that `raft-computer --help` lacks
   exactly the `channel`, `operation`, and `upgrade` lines (`decisions.md` D13).
   `raft --help` matches the oracle, including `migrate` (D16). The expected
   goldens and their diffs against the oracle goldens are reviewed once and
   stored in `tests/golden/`.
4. **One test**: one translated test per crate runs and passes.
5. **Suite**: `cargo test --workspace` passes, and `tools/test-parity` shows
   every upstream test in scope as ported or waived with a reason.
6. **CI matrix**: GitHub Actions runs the full suite on macOS, Linux, and
   Windows, and each OS's test counts match that OS's parity report (no silent
   skips; tests upstream itself skips on an OS count as `upstream-skipped`,
   `decisions.md` D9).
7. **End to end**: against a dedicated test server on raft.build:
   `raft-computer setup/attach/start`, an agent run on each locally installed
   runtime (codex, cursor-agent, grok) sends and reads messages through `raft`,
   and `stop` cleans up. Each run's commands and output are saved under
   `docs/migration/evidence/`.

## Work units and roles

Every work unit goes through the same loop, with a separate context for each role:

1. **Implementer**: translates the assigned files against the source and both
   guides. Does not review.
2. **Two adversarial reviewers**: each gets the diff, the source files, and the
   guides, but not the implementer's reasoning. Brief: "Assume this is wrong
   and enumerate how it breaks." Findings name the violated behavior or rule.
3. **Fixer**: applies the findings. Does not decide findings away without
   evidence.

Only the orchestrator runs builds and tests at phase boundaries, and only the
orchestrator commits. The orchestrator also owns golden capture: it runs the
oracle scripts (`tools/golden/`, `tools/zod-golden/`, and the parity listing of
D9) before the work units that consume their output, and commits the results. Workers edit only their assigned files and never stash,
reset, or run `git`.

The work unit changes with the phase:

| Phase | Unit |
|---|---|
| Translation | one source file (plus its test file), or a small group of tightly coupled files |
| Compiler repair | diagnostics for one crate, grouped by file |
| Start-up repair | one failing command with its saved output |
| Test repair | one failing test file |
| CI repair | one platform's failures |

## Order of work

1. `commander`, `raft-shared` (JS helpers and value model, schema runtime,
   JSON Schema validator, `http`/`process`/`env`/`node_compat` helpers, used
   contracts and formatters), `raft-trace-client`.
2. `raft-cli`.
3. `oar`, then `raft-daemon-core`.
4. `raft-computer`.

The pilot runs the full loop on the smallest end-to-end slice:
`raft task claim` with its framework dependencies.

## Stop and report

Stop and ask instead of widening scope when:

- a behavior cannot be reproduced without changing the server or the agent
  runtimes;
- a dropped feature turns out to be required for the finish line;
- an end-to-end step would touch anything outside the dedicated test server.
