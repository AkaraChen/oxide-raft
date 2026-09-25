# raft-computer core — port research (TS → Rust binary)

Source: `upstream/raft-source/packages/computer/src` @ 05f7d8f (v1.0.28). All
paths below are relative to that `src/` unless prefixed. Scope: the commands
`login logout attach setup start stop restart status doctor logs runners
{list,stop} channel {show,set}` plus hidden `__service`, `__run <serverId>`,
`__cli`, `__print-env`. Auto-update, k-carrier and legacy migration are
dropped. OS supervisor install (`osSupervisor*`, `macosLoginCarrier`,
`systemdDiscoveryPath`, `serviceControl`), status/doctor output and tests are
covered elsewhere and only referenced here.

---

## 1. Process architecture

```
user shell ── raft-computer <cmd>            (short-lived CLI; commander)
                 │ withMutationLock(computer/.lock) for mutating verbs
                 │ spawnDetachedService()  ── detached, stdio→run/service.log
                 ▼
            raft-computer __service           (resident supervisor, 1 per RAFT_HOME)
                 │ IPC listener run/service.sock | \\.\pipe\raft-computer-<hash>
                 │ reconcile every 5 s + ad-hoc timers
                 │ spawn (NOT detached) per managed server, stdio→servers/<id>/runner.log
                 ▼
            raft-computer __run <serverId>    (resident runner; hosts DaemonCore)
                 │ DaemonCore websocket to server; spawns agent runtimes
                 │ agent runtimes call `raft-computer __cli …` (bundled raft CLI)
```

One executable, argv-dispatched (`service.ts:1-19` header diagram).

### Entry seam — `index.ts` (418 lines)
- `__print-env` is served first, before anything else loads (`index.ts:404-408`,
  `295-303`): it connects to `--sock`, writes the env frame (§5), and exits 0.
  Any error exits **8**.
- `bootstrapThenRun` (`index.ts:361-393`) runs these steps in order:
  1. `parseLegacyOsSupervisorInvocation`. If it matches a retired entry, it
     writes `retired_os_supervisor_entry_ignored` and returns (other agent,
     legacy).
  2. `dispatchToKResident` (`153-242`): **DROP (k-carrier)**.
  3. `stripForwardedCarrierName` (`260-282`): **DROP (legacy carrier)**.
  4. `bootstrapSupervisedServiceEnv` (`318-349`). This runs only when argv is
     `__service` and the supervisor kind (from `--os-supervised <k>` or
     `RAFT_COMPUTER_OS_SUPERVISOR_KIND`) is `launchd-user` or `systemd-user`.
     - It first exports `SLOCK_HOME` from `--slock-home`/`--raft-home`, then
       captures the login-shell env (§5).
     - On success it applies the env and sets
       `RAFT_COMPUTER_SHELL_ENV_STATE=inherited`.
     - On failure it sets `unavailable:<CODE>` and writes a stderr line
       (`343-346`).
     - `windows-task` never captures.
  5. `await import("./cli.js")` then `runCliAsMain()`. The dynamic import is
     what lets the env be applied before any module-level env reads.
- `buildSelfExecArgv` (`309-316`): in SEA builds this is `[execPath]`;
  otherwise `[execPath, ...execArgv, script]`.

### CLI — `cli.ts` (930 lines)
- Commander program `raft-computer`, `.version(COMPUTER_VERSION)`. The
  description constants are at `cli.ts:142-152` and the help text must stay
  byte-identical.
- `withCliExit` (`94-105`) maps `CliExit` to `process.exitCode`.
- `runCliAsMain` (`917-926`): an uncaught error prints
  `raft-computer: <message>` (plus the stack if `RAFT_COMPUTER_DEBUG_STACK` is
  set) and exits 1.
- `runCli` (`876-899`):
  - `__build-versions` prints JSON.
  - `argv[2] === "__cli"` goes to `runBundledRaftCli(argv.slice(3))` **before**
    commander parses anything. That is the whole raft CLI (see
    `research/cli.md`), which must be linked into the same binary.
- Mutating commands run inside `withComputerMutationLock`: attach, setup,
  start, stop, restart, runners stop, channel set.
- Read-only commands take no lock: status, doctor (without `--fix`), logs,
  runners list, channel show.
- `restart` (`runRestartCommand`, `282-301`), in order:
  1. Best-effort `prepareLocalLifecycleOperations("restart")`.
  2. `findLiveServicePidReadOnly`.
     - No live service: `runStart({recordLifecycleIntent:false})`.
     - Live service: `setServerManaged`, reset degraded runners, then
       `requestServiceRestartViaIpc`.
  3. IPC failure gives `RESTART_SERVICE_UNREACHABLE`. Success prints
     `Service restart requested (pid N); replacement service will take over
     without relying on this shell.`
- `__service --slock-home --raft-home --os-supervised <kind>`: the kind is
  validated against `launchd-user|systemd-user|windows-task`, exported, and
  then `runService()` is called.
- `__run <serverId>` calls `runResident`.
- DROP: `upgrade` (`582-780`), `channel versions`, `operation acknowledge`,
  `__k-upgrade`, `__installer-converge`, `__legacy-supervisor-takeover`,
  `__supervisor retire-legacy`.

### Output and exit codes — `output.ts` (151 lines)
- `info()` writes to stdout.
- `fail(code,msg,exit=1)` writes to stderr and throws `CliExit`. The
  `formatHumanError` block is:
  - `Using state at <~home>`: only for `SETUP_*`, `MIGRATE_*`, `MIGRATION_*`,
    `LEGACY_*` and `NON_INTERACTIVE_SETUP_REQUIRES_FLAGS`.
  - `What happened (CODE): <msg with whitespace collapsed>`
  - `Next: …`: backticked `raft-computer …` commands extracted from the message
    and joined with ` && `, or per-code fallbacks (`87-109`).
  - `State: …`: the state guarantee (`111-126`).
  - `Help: https://app.raft.build/s/community/`
  - An optional post-help note.
- `present()` maps `ComputerError` to `fail`.
- Exit codes to preserve:
  - 0 = ok
  - 1 = any `fail`
  - 8 = `__print-env` failure
  - 77 = runner unlinked (`__run`)
  - 78 = EX_CONFIG (`__run` missing module or config)

### Service — `service.ts` (1213 lines)

**Spawn helpers**
- `buildResidentSpawn` (`161-180`): SEA uses `[execPath, ...execArgv, mode,
  serverId]`; otherwise the node script.
- `buildDetachedServiceEnv` (`207-226`):
  - Sets `RAFT_COMPUTER_PARENT_MUTATION_LOCK_HELD=1` unless the caller says
    false.
  - Sets or deletes `RAFT_COMPUTER_SOURCE_SERVICE_PID`.
  - Deletes `RAFT_COMPUTER_OS_SUPERVISOR_KIND`.
- `spawnDetachedService` (`265-299`):
  1. mkdir the run dir.
  2. `rotateLogIfNeeded(service.log)`, then open it with `"a"`.
  3. Resolve the K resident binary (**DROP**: always use `current_exe`).
  4. `spawn(detached, stdio [ignore, log, log], windowsHide)`, then `unref`.
  5. The **parent** writes `run/service.pid` with the child's pid.

**`runResident`** (`717-751`)
1. Assert a valid id and read the attachment (fail `NO_ATTACHMENT`).
2. Build the core via `defaultCoreFactory` (`395-517`).
3. SIGTERM/SIGINT call `core.stop()`, then exit 0.
4. `await core.start()`, then `writeRunnerVersionEvidence`.

**`defaultCoreFactory`** (`395-517`)
- A missing daemon module prints a message and exits 78.
- `DaemonCore` options:
  - `residentCoreIdentity` (serverUrl, apiKey,
    `machineOwnerProvenance{kind:"managed_computer_runner", serverId,
    serverMachineId}`, daemonVersion = `BUNDLED_DAEMON_VERSION`,
    computerVersion)
  - `localTrace:true`
  - `slockCliPath` (`"__cli"` in SEA, else `RAFT_COMPUTER_CLI_PATH`)
  - lifecycle ack getters and receipt from `residentLifecycleBridge`
  - `computerControlViaSupervisor:true`
- Hooks:
  - `onConnect` writes `runner.connected`; `onDisconnect` clears it.
  - `onHandshakeRejected` handles a 401 with reason
    `computer_machine_unlinked|computer_revoked`: it calls
    `markTerminalUnlinked`, then `process.exit(77)`.
  - `onComputerControl` enqueues a lifecycle op. `restart` calls
    `requestServiceRestartViaIpc({requestId, originServerId})`; `upgrade` is
    DROP.
  - `onComputerRestartReconcile` reads `restart-pending.json`. If the origin
    matches, it calls `emitDone` and clears the marker.
  - DROP: `reconcileComputerLifecycleOrigin` (legacy K) and
    `onComputerUpgradeReconcile`.
- `ensureSeaRuntimePackageDir` (`361-374`) writes `<home>/runtime-pkg/package.json`
  and sets `PI_PACKAGE_DIR`. Keep it only if the Rust DaemonCore still hosts
  the pi runtime.

**Runner exit handling**
- `classifyRunnerExit` (`557-567`), checked in this order:
  1. The runner log contains "Another Slock daemon is already running":
     `already-running` (adopt the external pid via `runnerLockConflict.ts:29`).
  2. Exit 77: `unlinked`.
  3. Exit 78: `config-error`.
  4. SIGTERM/SIGINT or exit 0: `graceful`.
  5. Anything else: `crash`.
- `handleRunnerExitForSupervisor` (`590-710`) holds the exact stderr messages
  per class. Crash budget: 3 crashes in 60 s leads to `degraded`
  (`health.ts:34-35`). Backoff is `CHILD_RESTART_BACKOFF_MS=2000`.

**`runService`** (`840-1213`), in order:
1. mkdir, then `runServiceStartupRecovery` (`776-796`, calls
   `cleanup.runFullCleanup`), then `resolveSourceServicePid`
   (`machineServiceAttestation.ts:184`).
2. Build `runners: Map<serverId, RunnerRecord>`. The child env is
   `buildRunnerChildEnv`, which strips PARENT_LOCK_HELD, SOURCE_SERVICE_PID
   and OS_SUPERVISOR_KIND (`runnerChildEnv.ts`).
3. `spawnChild` (`916-1004`):
   - Marks the record `starting` **synchronously**.
   - Rotates `runner.log`, records the byte offset, and spawns `__run`
     (attached, not detached).
   - On exit it clears `runner.pid`, reads the log tail from that offset
     (`internal/runner-log-diagnostics.ts`), and runs the handler above.
4. `killChild` sends SIGTERM.
5. `reconcile` (`1022-1067`):
   - wanted = `listManagedServerIds`.
   - Clear the external pid if it is dead.
   - Mark the runner ready when `runner.connected.pid == child.pid`; this is
     also when `runner.pid` is written.
   - Spawn if `canSpawn`; kill unwanted children.
   - DROP: the K hold read (`readKRunnerHold`) and K recovery.
6. Mutations exposed to IPC:
   - `restartService`: write `restart-pending.json`, then
     `performServiceSelfRestart` (`serviceControl.ts:93`, other agent) with
     `lib/replacementHandoff.ts`.
   - `resetService`, and `resetRunner` via `applyRunnerReset`.
   - DROP: `upgradeStart`.
7. IPC bind, then `publishServiceIdentityAfterIpcBind`, which writes
   `service.pid` and `service-version.json`.
8. SIGTERM/SIGINT call `shutdownService` (`lib/serviceShutdown.ts:29`):
   - SIGTERM the children and close IPC.
   - Wait up to 10 s, polling every 100 ms, then SIGKILL and poll every 1 s.
   - Clear `service.pid` unless a restart handoff is in progress, then exit 0.
9. Rehydrate each record from `health.json`: degraded or terminalUnlinked
   means parked (`lib/runnerStateMachine.ts:201`).
10. `startServiceReconcileLoop` (5 s `setInterval`, unref'd), then block
    forever.

### Electron co-tenant (compat constraint)
`apps/raft-desktop-electron/src/app/index.ts:19,67,427-445` imports
`runService`/`runResident` from `@botiverse/raft-computer/lib` and re-execs
**its own** binary with `__service`/`__run`. `computerHost.ts:180` talks to the
service over `connectService`. So the Rust CLI may meet a TS service owned by
the desktop app on the same `RAFT_HOME`, and vice versa.

The IPC wire protocol, every file format, and the version-evidence semantics
(§3) must therefore interoperate **across implementations**. The only
cross-implementation guard is `SERVICE_VERSION_SKEW`, which compares version
strings (`services/start.ts:191`).

---

## 2. IPC

Transport (`internal/ipc-server.ts:129`, `lib/ipc-client.ts:74`):
- **POSIX**: Unix socket `<home>/computer/run/service.sock`.
  - The run dir is mkdir'd 0700, owner-uid checked, then chmod'd 0700.
  - Stale probe (`175`): a 500 ms connect where only `ECONNREFUSED` unlinks the
    socket.
  - Then listen and chmod the socket 0600.
- **Windows**: named pipe `\\.\pipe\raft-computer-<first 16 hex of
  sha256(computerDir)>` (`paths.ts:285`). No ACL is applied (Node default
  descriptor). Rust `tokio::net::windows::named_pipe` should keep the default
  and use `first_pipe_instance(true)` to emulate EADDRINUSE.

Framing (`internal/ipc-codec.ts`, 77 lines):
- A u32 **big-endian** length, then UTF-8 JSON. The maximum is 1 MiB.
- Errors are `IPC_FRAME_TOO_LARGE` and `IPC_MALFORMED_FRAME`.

Handshake and messages (`internal/ipc-server.ts:78-79`, `lib/ipc-client.ts:50-56`):

| Direction | Message |
|---|---|
| client → service | `{type:"hello", protocolVersion:1, clientKind:"lib", clientVersion:"0.0.0"}` |
| service → client (accept) | `{type:"hello-ack", protocolVersion:1, serviceVersion:"0.0.0"}` |
| service → client (reject) | `{type:"hello-reject", reason, supported?/message?}` |
| client → service | `{type:"request", id:<uuid>, method, params}` |
| service → client | `{type:"response", id, result}` or `{type:"response", id, error:{code,message}}` |
| either | `ping`/`pong` |
| service → client | `{type:"event", kind, payload}` |
| client → service | `{type:"cancel", id}` on client timeout; the server ignores it |

- Unknown frame types are ignored.
- Handler errors that are not `ServiceClientError` map to `IPC_MALFORMED_FRAME`
  (`ipc-server.ts:478`).
- Client errors are `IPC_REQUEST_TIMEOUT` and `IPC_CLIENT_CLOSED`.
- `IPC_ERROR_CODES`: `lib/types.ts:31-47`.

Methods (`lib/types.ts:89-98`; handler table in `serviceIpcSeam.ts`, which
logs `Service: IPC seam listening at <path>` to stderr on bind):

| method | params | result |
|---|---|---|
| service-status | – | ServiceStatus |
| machine-attestation | – | `MachineServiceAttestation` (`types.ts:100-111`): generation = random uuid per process, `managedSetRevision = "${startedAt}:${ids.join(",")}"`, servicePid, sourceServicePid, managedServerIds, managedMachineIdentities, computerVersion, serviceExecutablePath |
| runner-status | `{serverId}` | RunnerStatus |
| list-runners | – | RunnerStatus[] |
| restart-service | `{requestId, originServerId}` or void | `{status:"accepted"}` |
| reset-service | – | `{status:"ok", previousState, clearedCrashCount}` (`reset.ts:87`) |
| reset-runner | `{serverId}` | `{status:"ok",…}` or `{status:"not-found", serverId}` (`reset.ts:108`) |
| upgrade-start | – | **DROP**. Keep the method name registered and reject with the existing error code, so old desktop clients get a structured error instead of MALFORMED. |

Client timeouts:
- `readMachineServiceAttestation`: 1 s (`machineServiceAttestation.ts:17,47`).
- `waitForRestartConvergence`: 240 attempts × 250 ms (`:125`).
- Self-restart takeover (`serviceControl.ts:113-115`): 10 s total, candidate
  shutdown 1 s, ownership restore 2 s.
- Route-or-disk rule (`reset.ts:48`): only a *connect* failure falls back to
  the disk handler. A handler error from a live service must propagate.

---

## 3. On-disk state

Home is `RAFT_HOME`, then `SLOCK_HOME`, then `~/.slock`, with `~` expansion
(`paths.ts:46-53`). Everything below is under `<home>/computer/` unless it is
marked `<home>/`. `CURRENT_SCHEMA_VERSION = 1` (`paths.ts:44`). Server ids must
match the UUID regex (`paths.ts:92`).

| path | writer | format |
|---|---|---|
| `user-session.json` | `lib/userSession.ts`, `services/login.ts` | pretty JSON, 0600. `{kind:"user-session", schemaVersion, userId, accessToken, refreshToken, serverUrl, email, name, displayName, createdAt, refreshedAt}`. Refresh writes tmp then renames. |
| `channel` | `lib/channelState.ts` | `"<channel>\n"`, 0600. `latest\|alpha\|pinned:<semver>`, default `latest`. |
| `service-version.json`, `servers/<id>/runner-version.json` | `runningVersionEvidence.ts:34`, `versionEvidence.ts` | JSON + `"\n"`, tmp then rename. `{version, installRoot, pid, writtenAt, parentPid?, shellEnvironment?}`. `installRoot = dirname(dirname(entry module))`; for the Rust port use `current_exe` parent-of-parent. It must be non-empty. |
| `.lock` (directory) | `concurrency.ts` | proper-lockfile mkdir lock (§7) |
| `.quarantine/<iso-stamp>-<id>/` | `cleanup.ts:220` | a renamed server dir (only when the attachment is unreadable) |
| `traces/*.jsonl` | `lib/computerTracer.ts` → `trace-client/localTraceSink.ts:69` | rotating spans, disabled by `RAFT_COMPUTER_LOCAL_TRACE=0` |
| `run/service.sock` | ipc-server | socket 0600, dir 0700 |
| `run/service.pid` | parent CLI at spawn **and** service after bind | decimal pid, no newline, 0600 (`internal/process-primitives.ts`) |
| `run/service.state.json` | `serviceState.ts:77` | compact JSON, 0600. `{schemaVersion, state, crashHistory}`. Transitions append a JSON line to service.log (`:103`). |
| `run/service.log` | the service's own stdio + transition lines | rotated (below) |
| `servers/<id>/runner.state.json` | `serverState.ts:213` | pretty JSON (`null,2`), 0600 + chmod. `{kind:"computer-attachment", schemaVersion, serverId, serverSlug, serverMachineId, machineId, apiKey, serverUrl, attachedAt, …}`. **`services/attach.ts:276-300` writes its own object without `schemaVersion`**, and the reader tolerates that; reproduce both exactly. |
| `servers/<id>/attachment.json` | legacy (read + migrate-on-read, `serverState.ts:141`) | see decision below |
| `servers/<id>/runner.pid` / `server-runner.pid` | service `reconcile`; legacy name is a read fallback (`paths.ts:160`) | pid, no newline |
| `servers/<id>/runner.log` / `server-runner.log` | the runner's stdio (fallback `paths.ts:167`) | rotated |
| `servers/<id>/managed.flag` | `serverState.ts:309` | empty file, 0600. Presence means "wanted". |
| `servers/<id>/health.json` | `health.ts:123` | compact JSON, 0600. `{schemaVersion, crashes:[{at,exitCode,signal}], fatalConfig?, terminalUnlinked?{at,reason,serverMachineId?,statusCode}}`. `emitRunnerStateTransition` (`:387`) appends `{at,kind:"runner-state-changed",serverId,fromState,toState,trigger}` to service.log. |
| `servers/<id>/runner.connected` | `residentConnectionMarker.ts:24` | `{pid,connectedAt}`, 0600, written **synchronously**. Cleared only if the pid matches (`:33`). |
| `servers/<id>/lifecycle-operations.json` (+ `.mutation.lock`) | `lifecycleOperations.ts:79` | `{schemaVersion, operations:[{operationId, parentOperationId?, action, targetVersion?, trigger?, pendingPhases, createdAt}]}`, tmp then rename, 0600 |
| `servers/<id>/runner.state.json.unlinked-<ts>-<8hex>.bak` | `setup.ts` (unlinked recovery) | archived attachment |
| `<home>/restart-pending.json` | `restartMarker.ts:17` | pretty JSON, `.tmp` then rename |
| `<home>/runtime-pkg/package.json` | `service.ts:361` | only if the pi runtime is kept |
| DROP | | `upgrade.log`*, `migration-dismissals.json`, `adoption.log`, `<home>/machine-operations/`, `<home>/machines/`, `upgrade-staging/`, `upgrade-snapshot.json` |

\* `lifecycleOperations.ts:273` (`retireCompletedUpgradeShutdownsFromLog`)
reads `upgrade.log` to retire upgrade ops. With updates gone, that becomes a
no-op. Stale `action:"upgrade"` ops left by an older TS install still need
some retirement rule, or they will be acked forever.

Log rotation (`logRotation.ts:31-32,80,105`):
- Rotation happens only at spawn time, when the file's UTC day differs from
  today or its size exceeds 64 MiB.
- The archive is `<stem>.<YYYY-MM-DD>[.n].log`.
- Archives older than 14 days are pruned.

Byte-exactness: formats differ per file (pretty vs compact vs a trailing `\n`).
Rust should use a per-file serializer: `serde_json::to_string_pretty` (2-space
indent, matching `JSON.stringify(v,null,2)`) or `to_string`. Key order must
follow the TS object literal order, so use struct field order rather than a
map.

**Legacy decision:** drop-in compatibility means a home created by older
versions may still hold only `attachment.json` or `server-runner.pid`/`.log`.
- Recommend keeping the *read fallbacks*: `paths.ts:129,152-167` and the
  `serverState.ts:141` merge. That is about 60 lines.
- Also keep the server URL rewrite (`serverState.ts:51-66`: staging fly →
  aws, `api.slock.ai` → `api.raft.build`).
- Drop everything under machine-operations and adoption.

---

## 4. Server API and command flows

HTTP goes through `proxy.ts:80` `computerFetch`, which uses undici and honours
`HTTPS_PROXY`/`HTTP_PROXY` (either case) and `NO_PROXY` (`:24-64`). In Rust,
use reqwest with a *custom* `Proxy::custom` closure that reproduces those
NO_PROXY rules; do not rely on reqwest's built-in rules.

The base URL is `https://api.raft.build`, overridden by `SLOCK_SERVER_URL`,
then `RAFT_SERVER_URL` (`serverUrl.ts`).

| call | site | auth / body | notable responses |
|---|---|---|---|
| POST `/api/auth/device/authorize` | `apiClient.ts:33` | `{clientName:"raft-computer"}` | 201. A 404 gives the "device login is not enabled… SLOCK_DEVICE_LOGIN_ENABLED" message (`:40`). |
| POST `/api/auth/device/token` | `:54` | `{deviceCode}` | `authorization_pending`, `access_denied`, `expired_token` |
| POST `/api/auth/refresh` | `lib/userSession.ts:159` | `{refreshToken}` | 200 `{accessToken, refreshToken}` |
| GET `/api/auth/me` | `apiClient.ts:525` | Bearer user | |
| GET `/api/servers/` | `:451` | Bearer user | setup server list |
| POST `/api/computer/attach` | `:137` | Bearer user, `{serverSlug, name}` | 201. 401 `session_invalid`; 403 `requires_admin`/`not_authorized`; 404 `server_not_found`/disabled (`services/attach.ts:214`). |
| POST `/internal/computer/preflight` | `:347` | Bearer `sk_…`, body `"{}"` | slug refresh (`targetServer.ts:28`) and unlinked detection |
| GET `/api/servers/:id/machines` | `:692` | Bearer user | |
| GET `/internal/computer/runners[?scope=server]` | `:765` | Bearer sk | `runners list [--all]` |
| POST `/internal/computer/runners/:agentId/stop` | `:784` | Bearer sk | |
| POST `/api/servers/:sid/machines/:mid/computer-lifecycle-operations` | `localLifecycleIntents.ts:54` | Bearer user + `X-Server-Id`; `{operationId, parentOperationId, action, targetVersion?, completionMode?}` | 201 `{operationId}` |
| DROP | | | `/api/computer/adopt-legacy` (`:195,246,300`), `/api/computer/legacy-machines` (`:573,618`), diagnosticsPush `/internal/machine/scope-attestation` and worker `/api/trace-bundles` (`services/diagnosticsPush.ts:549,571`; only setup's migration path uses them) |

`lib/userSession.ts`:
- JWT `exp` is checked with a 30 s leeway (`:8,199`).
- An in-flight refresh map dedupes concurrent refreshes (`:135-156`).

Command flows. Exact strings live in the presenter files; port them verbatim.
- **login**: `services/login.ts:82`, presenter `login.ts`, `browserHandoff.ts`.
  - Poll every `max(1, interval)` s until `expiresIn`.
  - Codes: `DEVICE_AUTHORIZE_FAILED`, `LOGIN_DENIED`, `LOGIN_EXPIRED`,
    `LOGIN_FAILED`.
  - The browser is opened with `open` / `xdg-open` / `cmd /c start`, via
    Enter-to-open on a TTY.
- **logout**: `lib/api.ts:383-418`:
  1. Clear all managed flags.
  2. Stop the service.
  3. Unlink the session.
  - Messages: `login.ts:82-95`.
- **attach**: `services/attach.ts:103`, presenter `attach.ts`.
  - It **never starts** the service. `--no-start`/`--foreground` only change
    the `Next:` hint.
- **setup**: `setup.ts` (1735 lines). Keep:
  - Non-TTY without `-y` gives `NON_INTERACTIVE_SETUP_REQUIRES_FLAGS`.
  - Refresh or login, then the servers list (`SETUP_SERVER_LIST_FAILED`,
    `SETUP_SERVER_UNAVAILABLE_TO_ACCOUNT`), then the already-attached / role
    check (`SETUP_REQUIRES_ADMIN`).
  - `attachFromSetup`, which prints `Connecting this computer to X… done.`
  - The `--no-start` message.
  - `startFromSetup`. On unlinked, it archives the attachment as a `.bak` and
    loops.
  - The running summary (`1186-1190`).
  - Raw-mode `readLine` (`770-818`); in Rust use crossterm raw mode.
  - Drop: `--machine`, migration detection, and the picker (about half the
    file).
- **start**: `services/start.ts:307` (dedupes in-flight starts per home), then
  `startInner:329`:
  1. Resolve the target (`targetServer.ts:48`: `NO_ATTACHMENT`,
     `NOT_ATTACHED`, `AMBIGUOUS_SERVER`).
  2. Check terminal-unlinked (`:258`, `COMPUTER_MACHINE_UNLINKED`).
  3. `setServerManaged`.
  4. Host lifecycle (other agent).
  5. `findLiveServicePid`; spawn if absent.
  6. Clear degraded (via IPC `reset-runner` when a service is live, `:213-232`).
  7. Version skew check (`:191`, `SERVICE_VERSION_SKEW(_SUSPECT)`).
  8. Wait up to 15 s, polling every 100 ms (`:60-61,162,551`), for all of:
     - `runner.pid` alive,
     - `runner-version.json.pid == runner.pid` and
       `version == COMPUTER_VERSION`,
     - `runner.connected.pid == runner.pid`.
  9. `START_DAEMON_TIMEOUT` (`:274`).
  - `--foreground` sets PARENT_LOCK_HELD and runs `runService` inline.
  - Presenter: `startStop.ts:104-150`.
- **stop**: `services/stop.ts:106`.
  - SIGTERM `service.pid`, then poll every 200 ms for up to 5 s (`:43-44`).
  - Codes: `STOP_SIGNAL_FAILED`, `STOP_TIMEOUT`.
  - Presenter: `startStop.ts:201-207`.
- **restart**: see §1.
- **runners**: `runners.ts`, a table with `padEnd` 38/11/10/15.
- **logs**: `logs.ts`, default 200 lines, `--service`, secrets redacted via
  `doctor.ts:40`, `NO_DAEMON_LOG`.
- **channel show/set**: `channel.ts`, `lib/channelState.ts`.
  - It prints `Channel set to X.` and "The next `raft-computer upgrade` uses
    X." Keep this text even though `upgrade` is gone.
  - `CHANNEL_INVALID`.
- **accountUnavailable.ts**: en / zh-cn chosen from `LC_ALL`, `LC_MESSAGES`,
  `LANG`.

Env vars read by core:
- `RAFT_HOME`, `SLOCK_HOME`
- `SLOCK_SERVER_URL`, `RAFT_SERVER_URL`
- `RAFT_COMPUTER_CLI_PATH`
- `RAFT_COMPUTER_PARENT_MUTATION_LOCK_HELD`, `RAFT_COMPUTER_SOURCE_SERVICE_PID`
- `RAFT_COMPUTER_OS_SUPERVISOR_KIND`, `RAFT_COMPUTER_SUPERVISOR_OWNER`
- `RAFT_COMPUTER_SHELL_ENV_STATE`
- `RAFT_COMPUTER_LOCAL_TRACE`, `RAFT_COMPUTER_DEBUG_STACK`
- `PI_PACKAGE_DIR`
- proxy vars, and the locale vars above

Drop `RAFT_COMPUTER_UPGRADE_BASE_URL`, `RAFT_COMPUTER_RELEASE_BACKEND`,
`SLOCK_UPGRADE_TRIGGER` and `SLOCK_DAEMON_TRACE_UPLOAD_URL`.

---

## 5. Shell env capture (`shellEnvCapture.ts`, 406 lines)

- Runs only for `__service` under launchd-user or systemd-user (§1).
- Returns `SHELL_ENV_UNSUPPORTED_PLATFORM` on anything except darwin/linux
  (`:165`).
- The shell comes from the **passwd entry** (`os.userInfo().shell`, which must
  be absolute), not `$SHELL` (`:79`). Its basename must be one of
  zsh/bash/sh/dash/ksh (`:77`). In Rust use `getpwuid_r`.
- `mkdtemp(tmpdir/"raft-se-")` with socket `s`; the short path keeps it under
  the `sun_path` limit.
- Command: `shell -i -l -c "exec '<self>'… __print-env --nonce <uuid> --sock '<p>'"`.
  Every argv element is single-quoted with `'\''` escaping (`:204-207`).
- The shell is spawned detached with stdio ignored.
- Frame (`:98-135`):
  - Header `RAFT-ENV1 <nonce>\n`.
  - Then `key=value\0` entries. Keys that are empty, contain `=`, or contain
    NUL are skipped, as are values containing NUL.
  - Trailer `RAFT-ENV1-END <nonce>\n`.
  - Parsing is strict: bytes outside the frame or a duplicate key reject the
    whole frame.
- Limits: 10 s timeout and 1 MiB maximum.
- Success requires both a valid frame and shell exit 0.
- On timeout, the process group gets TERM, then KILL 1 s later. It does not
  signal a group with no target.
- Failure codes (`:51-59`): `UNSUPPORTED_PLATFORM`, `UNSUPPORTED_SHELL`,
  `SPAWN_FAILED`, `TIMEOUT`, `OUTPUT_TOO_LARGE`, `BAD_FRAME`,
  `SHELL_EXITED_NONZERO`, each prefixed `SHELL_ENV_`.
- `applyCapturedEnv` (`:390-406`) replaces rather than merges:
  1. Delete parent keys that are absent from the snapshot.
  2. Apply the snapshot.
  3. Re-apply the protected keys (`:41-49`: SLOCK_HOME, RAFT_HOME, CLI_PATH,
     SUPERVISOR_OWNER, OS_SUPERVISOR_KIND, PARENT_MUTATION_LOCK_HELD,
     SOURCE_SERVICE_PID) from their pre-capture values, deleting them if they
     were unset.
- The outcome is persisted in `service-version.json.shellEnvironment`.
- Rust notes:
  - Capture and apply **in `main()` before the tokio runtime or any thread
    starts**, because `set_var` is unsound once threads exist. Alternatively,
    keep a captured env map and pass it through `Command::env_clear().envs()`.
    Either way, the reqwest proxy config must read from the same map.
  - Handle values as `OsString`/bytes: Node decodes lossily as UTF-8; Rust can
    stay exact. Serialize with `\0` separators.
  - `__print-env` must be dispatched before any other init, including logging
    setup, because its stdout/stderr go to /dev/null.
- `windowsPowerShellEnv.ts` is only used by `osSupervisorRuntime` (other
  agent).

---

## 6. Module table

Totals over non-test `src/**/*.ts`: **27,626 lines**.
- KEEP: 5,799
- PARTIAL: 9,654
- DROP(legacy): 2,689
- DROP(update): 324
- DROP(k-carrier) direct: 71, plus the `k*.ts` family (6,284) = 6,355
- DROP(lib-only/diagnostics): 1,739
- DROP(test harness): 624
- Other agent's scope: 4,929 (osSupervisor*, macosLoginCarrier,
  systemdDiscoveryPath, serviceControl, windowsPowerShellEnv, status, doctor,
  doctorCli)

Realistic port surface ≈ KEEP + about 55% of PARTIAL ≈ **11k TS lines**.

| module | lines | class | note |
|---|---:|---|---|
| setup.ts | 1735 | PARTIAL | drop --machine, migration detect, picker, diagnosticsPush |
| service.ts | 1213 | PARTIAL | drop K resident, K hold, upgradeStart, upgrade reconcile, legacy origin |
| cli.ts | 930 | PARTIAL | drop upgrade, versions, operation, __k-*, __installer-*, legacy |
| apiClient.ts | 796 | PARTIAL | drop adopt-legacy, LegacyMachinesClient |
| lib/api.ts | 674 | PARTIAL | ComputerApi facade; keep logout/reset routing, drop upgrade/legacy |
| services/start.ts | 558 | PARTIAL | drop K host-lifecycle hooks |
| lib/types.ts | 509 | PARTIAL | IPC types; drop upgrade events (keep method name) |
| internal/ipc-server.ts | 496 | KEEP | |
| cleanup.ts | 426 | PARTIAL | drop upgrade-staging/snapshot tmp cleanup |
| index.ts | 418 | PARTIAL | keep __print-env + env bootstrap |
| health.ts | 409 | KEEP | |
| shellEnvCapture.ts | 406 | KEEP | |
| paths.ts | 397 | PARTIAL | drop migration/adoption/upgrade paths; keep legacy read fallbacks |
| lib/ipc-client.ts | 393 | KEEP | |
| localLifecycleIntents.ts | 367 | PARTIAL | drop `prepare*Upgrade*` (`:159-307`) |
| serverState.ts | 346 | PARTIAL | decide on legacy attachment.json merge |
| services/attach.ts | 323 | KEEP | |
| channel.ts | 292 | PARTIAL | drop `versions` (Hands release discovery) |
| lifecycleOperations.ts | 291 | PARTIAL | upgrade-log retire becomes a no-op |
| services/stop.ts | 231 | PARTIAL | drop K host hook |
| lib/runnerStateMachine.ts | 228 | KEEP | |
| startStop.ts | 227 | KEEP | presenter |
| lib/userSession.ts | 225 | KEEP | |
| machineServiceAttestation.ts | 192 | KEEP | |
| services/login.ts | 184 | KEEP | |
| accountUnavailable.ts | 171 | KEEP | |
| concurrency.ts | 170 | KEEP | |
| residentLifecycleBridge.ts | 164 | PARTIAL | strip `acknowledgeKReadyReceipt`/`bindKUpgradeReadyAcknowledgement` (`:30-99`) |
| serviceState.ts | 163 | KEEP | |
| output.ts | 151 | KEEP | |
| reset.ts | 126 | KEEP | |
| logRotation.ts | 120 | KEEP | |
| lib/readers.ts | 116 | KEEP | |
| internal/service-pid-fallback.ts | 115 | KEEP | |
| serviceIpcSeam.ts | 103 | PARTIAL | upgrade-start handler |
| machineReadiness.ts, machineFacts.ts | 98, 88 | KEEP | |
| login.ts, targetServer.ts, runners.ts, proxy.ts | 95, 93, 84, 84 | KEEP | |
| lib/events.ts, lib/traceTypes.ts, lib/computerTracer.ts | 84, 34, 24 | PARTIAL | trace sink shared with daemon port |
| lib/serviceShutdown.ts, internal/ipc-codec.ts, runnerLockConflict.ts | 83, 77, 73 | KEEP | |
| lib/state.ts, lib/channelState.ts, runningVersionEvidence.ts, browserHandoff.ts | 71, 71, 70, 69 | KEEP | |
| restartMarker.ts, versionEvidence.ts, version.ts | 65, 63, 62 | KEEP/KEEP/PARTIAL | versions baked via `env!()` in build.rs |
| internal/runner-log-diagnostics.ts, internal/process-primitives.ts, attach.ts | 54, 51, 50 | KEEP | |
| residentConnectionMarker.ts, logs.ts, lib/errors.ts, residentCoreIdentity.ts, lib/replacementHandoff.ts, lib/serviceIdentity.ts, serverUrl.ts, runnerChildEnv.ts, services/errors.ts, serviceReconcileLoop.ts | 40…9 | KEEP | |
| legacySupervisorTakeover, legacyOsSupervisorMigration, legacyKOriginAdoption, machineOperation{Store,Runtime}, machineConvergenceReducer, services/adoptLegacy, lib/{migration,migrationDismissals,adoptLegacyResponse} | 2689 | DROP(legacy) | machineOperation* only reachable from legacySupervisorTakeover |
| serviceUpgradeStart, computerRelease, releaseAuthority | 324 | DROP(update) | |
| durableFile, realFileIdentity | 71 | DROP(k-carrier) | only imported by macosLoginCarrier/kHostAdapter/kResidentBinary/index K path |
| k*.ts (18 files) | 6284 | DROP(k-carrier/update) | replacement handled by the other agent |
| services/diagnosticsPush, lib/index, lib/actions, lib/affordances | 1739 | DROP(lib) | |
| internal/h-family-chaos, fixtures/*, test-fixtures/*, test/runNamedCase | 624 | DROP(test) | |

**Does the Rust port need `lib/`?** Not as a public library.
- `lib/index.ts` is the Electron desktop app's import surface
  (`apps/raft-desktop-electron/src/app/{index,computerHost}.ts`). The desktop
  app keeps using the TS package.
- The Rust binary needs only the *internal* pieces of `lib/` as private
  modules: ipc-client, types, state, runnerStateMachine, serviceShutdown,
  userSession, channelState, readers, replacementHandoff, serviceIdentity,
  errors, and the api.ts routing helpers.
- What must be preserved is **wire and disk compatibility** with that TS lib:
  IPC (§2), files (§3), and `clientKind:"lib"`.

---

## 7. Async and concurrency hazards

1. **Single-writer supervisor with no mutex.**
   - `reconcile` runs from the 5 s interval, from exit handlers, from backoff
     timers and from IPC mutations. That is safe in Node only because
     `spawnChild` marks the record `starting` synchronously before its first
     `await` (`service.ts:916`).
   - In tokio, run the supervisor as **one actor task** that owns
     `HashMap<ServerId, RunnerRecord>`, with IPC handlers and child-exit
     watchers sending messages to it. Do not share it behind `Arc<Mutex>`
     across `.await`.
   - Reconciles must coalesce: Node overlaps async `reconcile` bodies, and
     only the synchronous `starting` guard prevents double-spawn.
2. **Mutation lock = proper-lockfile.** There is no Rust crate for it; it must
   be reimplemented bit-compatibly (`concurrency.ts:30-76`), because a TS
   desktop app may contend for the same lock. The protocol:
   - Acquire by `mkdir computer/.lock` (atomic); EEXIST triggers retry.
   - Retries: 10, minTimeout 200, maxTimeout 800, factor 1.5.
   - Refresh the directory mtime every `stale/2` = 30 s.
   - It is stale if mtime is more than 60 s old; then rmdir and retry.
   - It is compromised if an mtime update fails or the mtime changed under us
     (`onCompromised` gives `MUTATION_LOCK_COMPROMISED` and aborts via a
     signal).
   - `realpath:false`.
   - `ELOCKED` gives `CONCURRENT_OPERATION` "Another Computer command is
     currently mutating state. Wait a moment and retry."
   - The lifecycle ops lock uses the same protocol on
     `lifecycle-operations.json.mutation.lock`, with stale 60 s and retries
     {10, 20, 200, 1.5}.
   - `cleanupStaleLock` (`cleanup.ts:353-386`) re-enters via `lock(retries:0)`
     and never rm's directly.
3. **Lock inheritance by env flag.**
   - A CLI holding `.lock` spawns `__service` with
     `PARENT_MUTATION_LOCK_HELD=1`, which makes startup recovery skip stale
     lock cleanup (`cleanup.ts:399-409`).
   - A self-restart replacement is also spawned with
     `parentMutationLockHeld:true` (`serviceControl.ts:150`).
   - The runner child env strips the flag.
   - This must be replicated exactly, or a service could reclaim a live CLI's
     lock.
4. **Startup recovery quarantines unreadable server dirs**
   (`cleanup.ts:251-280`). The attachment write in `services/attach.ts` is a
   plain write, not tmp+rename. A service booting concurrently with an attach
   that is outside the lock (the foreground or Electron path) could read a
   truncated file and quarantine it.
   - Rust: write attachments with tmp+rename (still byte-compatible).
   - Keep the lock-held skip.
   - Orphan scan (`cleanup.ts:158-203`): this uses `ps` and a `comm` starting
     with `slock-`, and is a no-op on Windows. The Rust binary's comm is
     `raft-computer`, so the scan is effectively dead code; keep the no-op.
5. **Restart handoff overlap** (`serviceControl.ts:93-241`):
   1. The incumbent `releaseListener()` closes IPC but keeps its runners.
   2. It spawns a replacement with `SOURCE_SERVICE_PID`.
   3. It polls attestation, requiring `servicePid==replacement`,
      `sourceServicePid==me`, and identical managed set + machine ids.
   4. Then it SIGTERMs itself via `setImmediate`.
   - In the meantime the replacement's `__run` children hit the daemon machine
     lock ("Another Slock daemon is already running"), classify as
     `already-running`, and adopt the external pid. They respawn only after
     the old runner dies (`clearExternalRunnerPidIfDead`).
   - On failure the incumbent must restore its listener within 2 s.
   - An empty managed set gives `SELF_RELAUNCH_UNAVAILABLE`.
   - `shutdownService` must not delete `service.pid` during the handoff.
   - The Rust IPC listener therefore needs an explicit release/rebind
     capability; on POSIX that means unlinking and re-binding the socket path.
6. **Two writers of `service.pid`.** The spawning CLI writes the child pid
   (`service.ts:265-299`), and the service rewrites it after IPC bind. Readers
   treat "pid alive" as "service up". Pid reuse and EPERM-as-alive
   (`internal/process-primitives.ts`) must match.
7. **Readiness depends on DaemonCore semantics.**
   - `runner-version.json` is written only after `core.start()` resolves
     (`service.ts:717-751`).
   - `runner.connected` is written by `onConnect` synchronously.
   - `start` requires all three pid matches within 15 s. The Rust DaemonCore's
     `start()` must resolve at the same point (after the first handshake or
     after listener setup) and must invoke hooks from a context that can do
     sync file I/O.
   - `process.exit(77)` from inside a hook means the Rust hook needs a way to
     terminate the process (or signal main) without skipping `runner.log`
     flush.
8. **Stop vs shutdown timeouts race.** `stop` gives up after 5 s
   (`STOP_TIMEOUT`), but the service waits up to 10 s for children before
   SIGKILL (`lib/serviceShutdown.ts:4`). Keep both numbers; the race is
   existing behaviour.
9. **Windows signals.** Node `process.kill(pid,"SIGTERM")` on Windows is
   `TerminateProcess`, so no handler runs.
   - `stop`, the child kills and the self-restart exit are all hard kills.
   - Non-detached `__run` children are **not** killed with the service and are
     then adopted via the lock-conflict path.
   - `service.pid` is left behind and cleaned as stale.
   - Rust should reproduce the observable outcome; a Job Object would change
     behaviour.
10. **Sync I/O in callbacks.** DaemonCore callbacks call
    `readOperationsSync` (`lifecycleOperations.ts:71`) and the sync marker
    writes. In Rust keep these as `std::fs` calls from a non-async fn, or use
    `spawn_blocking`. Never hold the lifecycle lock across them.
11. **Session refresh dedupe.** The in-flight map in `lib/userSession.ts` is
    per process. Concurrent CLI processes can both refresh, and last
    tmp+rename wins; refresh-token rotation may then invalidate the loser.
    That is existing behaviour, so do not "fix" it silently.
12. **Log offset diagnostics.** The exit classifier reads `runner.log` from
    the byte offset recorded at spawn. Rotation happens before recording the
    offset, and the child appends via an inherited fd. In Rust, open the log
    with `O_APPEND`, take the offset from `metadata().len()` after rotating,
    and give the fd to `Stdio::from(file)` for both stdout and stderr.
13. **Shell env apply before threads.** See §5.
