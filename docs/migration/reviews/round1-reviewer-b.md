# Round 1 review: reviewer B (architecture, ownership, concurrency)

Scope: `decisions.md` D6, D7, D8, D11, D12, D13, D14; `mapping-guide.md` §7, §8
(and §9 where it conflicts); `README.md`; scope-drop dependencies; cross-artifact
conflicts. Source paths are relative to `upstream/raft-source/packages/`
(commit 05f7d8f) unless noted.

Severity counts: blocker 5, major 21, minor 13.

---

## Blockers

### 1. Child events tagged by `(agent_id, launch_id)` misattribute a dead process's events to its successor
- **Severity:** blocker
- **Artifact:** decisions.md D6 (lines 180–181, 187–189)
- **Failing behavior / source fact:** TS never fences process events by
  launch id. It fences by object identity: listeners are closed over the
  specific `runtime`/`runtimeProcessBindingFence` instance and check
  `current.runtime === runtime` (daemon/src/agentProcessManager.ts 3262–3320,
  3612). The stop path documents that launch ids are not unique:
  "launchId can be absent/reused" and "silent respawn under the SAME launchId"
  (agentProcessManager.ts ~4088–4170, comment near 4126), and that
  "stop() can resolve after a later start has run" (same block). A late
  `exit`, stdout line or stderr chunk from process N arrives after process N+1
  was bound under the same `(agent_id, launch_id)`; D6's tag accepts it and
  drives N+1's state machine (for example an exit marks the fresh process dead,
  or a stale `thread/started` sets `currentSessionId`).
- **Correction:** Tag every child-originated `CoreMsg` with a per-spawn
  identity minted when the session object is created (a monotonic
  `process_instance_id`, which TS already has as `processInstanceId`), and make
  the actor compare it with the currently bound session exactly where TS
  compares object identity. Timers that TS binds to a runtime object (see
  finding 8) use the same key.

### 2. Driver/session ownership in D7 cannot satisfy the synchronous reads and sends the APM performs
- **Severity:** blocker
- **Artifact:** decisions.md D7 (lines 224–230) vs D6 (lines 176–193)
- **Failing behavior / source fact:** D7 puts the driver inside a session task
  that is "never shared" and flushes stdin after each `parse_line`. TS reads
  and writes driver/session state synchronously from APM handlers:
  - `runtimeProcessBindingFence.send(...)` is synchronous, calls
    `driver.encodeStdinMessage` (which mutates driver state, e.g. grok
    `pendingDeliveryRequests`, daemon/src/drivers/grok.ts 634–670), writes to
    stdin, and returns `{ok, acceptedAs}`/`unsupported`; the APM branches on
    that result in the same tick (agentProcessManager.ts 2597, 7754, 7880,
    8046; daemon/src/drivers/runtimeSession.ts `send`).
  - `ap.driver.currentSessionId` is read synchronously (agentProcessManager.ts
    7301, 7487); `runtime.currentRuntimeHomeDir` likewise (4993, 5444, 5575).
  If the driver lives in a separate task, each of these becomes a round-trip
  await, which splits handlers that TS runs without interleaving (D6's own
  invariant) and lets a stdout-driven `parse_line` mutate driver state between
  the APM's check and its use.
- **Correction:** State in D6/D7 that the driver and the session's mutable
  state are owned by `CoreActor` (inside the agent record). Only the raw pipe
  I/O lives outside: a per-process reader task posting lines, and a per-process
  ordered writer task fed by an unbounded channel. `send()` and `parse_line()`
  run inside the actor, push bytes to the writer channel in call order, and
  return synchronously.

### 3. Synchronous `get_computer_lifecycle_ready_acks()` versus a source hook that waits up to 60 s
- **Severity:** blocker
- **Artifact:** decisions.md D6 (lines 204–206)
- **Failing behavior / source fact:** The host option type returns a Promise
  (daemon/src/core.ts 1013–1063), `emitReady` awaits it (core.ts 4193–4250),
  and the computer implementation `getReadyAcknowledgements` is async and may
  call `waitForRestartConvergence` (240 polls × 250 ms, up to 60 s,
  computer/src/machineServiceAttestation.ts 125–160) and k-carrier
  `loadOperation` (computer/src/residentLifecycleBridge.ts 100–137). A
  synchronous trait method either blocks the actor (inbound frames, child exits,
  timers, proxy requests all stall up to 60 s) or cannot express the wait at
  all, changing the `ready` frame's `lifecycleAcks`.
- **Correction:** Make `get_computer_lifecycle_ready_acks` async, invoked from
  a spawned future whose completion posts a continuation (`ReadyAcksResolved
  { generation, acks }`) that sends `ready` only if the connection generation
  still matches — the same fencing `emitReady` does after its await.

### 4. D11 says legacy `--os-supervised` invocations boot a service; TS refuses them
- **Severity:** blocker
- **Artifact:** decisions.md D11 (lines 320–321)
- **Failing behavior / source fact:** `bootstrapThenRun` first calls
  `parseLegacyOsSupervisorInvocation`; for the exact legacy grammar
  (`__service` at argv index 1 or 2, then `--slock-home X` and
  `--os-supervised <kind>`, exact length) it writes
  `raft-computer: retired_os_supervisor_entry_ignored kind=<kind>\n` and
  returns without starting a service (computer/src/index.ts 361–393,
  computer/src/osSupervisorLifecycle.ts 16–50). Only non-legacy shapes reach
  `cli.ts`, where `--os-supervised` is validated against three kinds, written
  to `RAFT_COMPUTER_OS_SUPERVISOR_KIND`, and gates shell-env capture
  (computer/src/cli.ts 784–799; index.ts 318–349). "Accept and ignore" changes
  the stderr bytes and exit behavior for stale systemd/launchd definitions and
  makes a stale unit fight the detached service D11 itself warns about.
- **Correction:** Replace the bullet with: translate
  `parseLegacyOsSupervisorInvocation` and its refusal line exactly; in the
  non-legacy path, translate the `--os-supervised` validation, the env write,
  and the `--slock-home`/`--raft-home` handling (cli.ts 784–790) exactly. List
  `osSupervisorLifecycle.ts`'s parser as kept despite the README's
  legacy-migration drop.

### 5. D13 invents `upgrade_unsupported` and puts the reply in the wrong layer
- **Severity:** blocker
- **Artifact:** decisions.md D13 (lines 381–384)
- **Failing behavior / source fact:** In TS, DaemonCore does not reply to
  `computer:upgrade` itself. It dedupes by durable lifecycle acks plus
  `handledComputerControlOperationIds`, invokes `onComputerControl` in
  `Promise.resolve().then`, maps only `control_busy`,
  `self_relaunch_unavailable`, `computer_control_failed`, and replies only when
  a `requestId` exists; with no host hook the message is ignored
  (daemon/src/core.ts 4102–4173). The computer hook enqueues a lifecycle
  operation (shutdown + ready phases) for upgrade as well as restart, then
  relays over IPC (computer/src/service.ts 426–516). The TS path for a host
  that cannot upgrade already exists: `serviceUpgradeStart` rejects with
  `UPGRADE_START_REJECTED` / `describeUpgradeStartRejection("K_COORDINATOR_SEA_ONLY")`
  (computer/src/serviceUpgradeStart.ts 67–72), and `requestServiceUpgradeViaIpc`
  turns that into `emitUpgradeDone({ok:false, error: err.message})`
  (computer/src/serviceControl.ts). The server therefore sees that message
  text, not `"upgrade_unsupported"`, and replays are deduped rather than
  answered twice.
- **Correction:** Keep `core.ts`'s `computer:upgrade` handling as a straight
  translation. Put the drop in the computer: the Rust `upgrade-start` handler
  takes the existing non-SEA rejection branch, so the wire `error` is the
  `describeUpgradeStartRejection("K_COORDINATOR_SEA_ONLY")` text. Then decide
  explicitly whether the hook still enqueues the upgrade lifecycle op (TS
  does) and how that op is retired (finding 20).

---

## Majors

### 6. "Handles each message to completion" hides the interleaving TS relies on at real I/O awaits
- **Severity:** major
- **Artifact:** decisions.md D6 (lines 190–193)
- **Failing behavior / source fact:** TS handlers interleave at every real
  I/O await and re-check state afterward: `startAgent` awaits
  `buildSpawnConfig` (mint, provider materialization, env-vars provider,
  agentProcessManager.ts 3124, 3847–3874), `driver.spawn` awaits
  `prepareCliTransport` (which awaits `registerAgentCredentialProxy`,
  daemon/src/drivers/cliTransport.ts 651) and
  `prepareManagedMcpRuntimeProxy` (drivers/codex.ts 888–989), then checks
  `stopEpochChanged` (agentProcessManager.ts 3220–3226). Stop awaits
  `runtime.stop({forceAfterMs})` for up to 5 s and checks `startEpochChanged`
  (4088–4170). Awaits on already-resolved values yield only to microtasks and
  do not let queued I/O events in. "Continuation message" does not say which
  awaits become continuations or where the continuation is enqueued relative to
  already-queued frames.
- **Correction:** Add a rule: an await that performs real I/O or a timer
  becomes a continuation posted to the back of the actor queue (other messages
  may run first, as in Node, so every post-await re-check is translated). An
  await that only chains in-process promises (no I/O) must not be split; the
  following code runs in the same handler. Require each split point to be
  listed in the translation notes with the re-check that follows it in TS.

### 7. Ordering of post-start continuation versus first stdout line is unspecified
- **Severity:** major
- **Artifact:** decisions.md D6, D7
- **Failing behavior / source fact:** Listeners are attached before
  `await runtimeProcessBindingFence.start` (agentProcessManager.ts 3262–3320,
  3612), and after `start` resolves the APM sends `agent:status active`,
  broadcasts "Starting…", and arms the startup timeout (3676–3681). In Node,
  `RuntimeSession.start` attaches its stdout listener after `await
  driver.spawn`, and the continuation after `start` runs in a microtask before
  any stdout `data` event can be dispatched. If the Rust reader task can post a
  line before the start continuation, the server sees activity/trajectory
  frames before `active`, and the startup timeout is armed after the first
  event.
- **Correction:** Specify that the reader task for a new process is not started
  (or its lines are buffered by the actor) until the post-start continuation
  has run.

### 8. Generation-dropping timers breaks timers TS binds to a specific process
- **Severity:** major
- **Artifact:** decisions.md D6 (lines 187–190)
- **Failing behavior / source fact:** Not all TS timers live in the state
  union. `RuntimeSession.stop` arms an unref'd SIGKILL timer that checks
  `this.closed` of that session at fire time (daemon/src/drivers/runtimeSession.ts
  161–169); the stop-wait promise (5 s) holds the old `ap`
  (agentProcessManager.ts 4088–4170). If a restart bumps the agent's
  generation, D6 drops the old SIGKILL timer, and the old process that ignored
  SIGTERM is never force-killed.
- **Correction:** Distinguish state-union timers (fenced by generation) from
  object-bound timers (fenced by the per-spawn identity from finding 1, firing
  against that process even after the agent has moved on). Enumerate the ~15
  timer kinds with their fencing key.

### 9. Proxy handler requests must preserve TS's synchronous segments, and the proxy cannot be a serial actor
- **Severity:** major
- **Artifact:** decisions.md D6 (lines 183–185, 198–202)
- **Failing behavior / source fact:** The credential proxy's side-effect step
  reads `getPendingMessages` before `await loadRecentTargetMessages`, reads
  `getBoundary` after it, calls `planAgentInboxSideEffect` with a synchronous
  `isMessageModelSeen` callback into APM state, then awaits
  `applyAgentInboxStateMachineEffects` (daemon/src/agentCredentialProxy.ts
  1500–1544; coordinator closures at agentProcessManager.ts 2112–2127). A
  one-request-per-call oneshot design either turns the synchronous callback
  into an await inside a pure planner (impossible without restructuring) or
  splits one TS synchronous segment across several actor turns. Separately,
  `registerAgentCredentialProxy` is awaited from inside the actor's start
  path; if the proxy service processes requests serially while one handler
  awaits the actor, the actor's register call deadlocks.
- **Correction:** Specify that each TS synchronous segment between awaits in a
  proxy handler is one actor request carrying everything it needs (e.g., a
  `PlanSideEffect` request that runs the planner, with `isMessageModelSeen`,
  inside the actor). HTTP handlers run as independent tasks; registration is a
  direct registry update (or a non-blocking message), never a request queued
  behind handlers.

### 10. Proxy lifetime and port identity change when module globals become DaemonCore-owned
- **Severity:** major
- **Artifact:** decisions.md D6 (lines 198–202)
- **Failing behavior / source fact:** Both proxies are process-global,
  lazily bound once, `unref()`'d and never closed (agentCredentialProxy.ts
  231–233, 327–365; daemon/src/managedMcpRuntimeProxy.ts 54–57, 448). The port
  written into agent wrappers/config is stable for the life of the `__run`
  process, across DaemonCore reconnects and across agents. If the service is
  owned by a DaemonCore instance and dropped or re-created, live agents hold a
  dead URL.
- **Correction:** State that one proxy server per process is created lazily on
  first registration, survives for the process lifetime, and never keeps the
  runtime alive on its own. Ownership by DaemonCore is fine only if DaemonCore
  is a process singleton.

### 11. D7 omits codex's microtask and kimi's in-`spawn` writes, and does not order flush versus event dispatch
- **Severity:** major
- **Artifact:** decisions.md D7 (lines 218–229)
- **Failing behavior / source fact:**
  - codex also defers `initialize` through `queueMicrotask`, which sets
    `pendingThreadRequest` (drivers/codex.ts 979–985); `startInitialTurn`
    depends on `managedMcpReady` (1145–1167).
  - kimi writes to stdin twice synchronously inside `spawn()` before it
    returns (drivers/kimi.ts 178–202), outside `parse_line`, so `spawn` also
    needs `DriverIo`.
  - grok writes from `encodeStdinMessage` and from paths reached by
    `parseLine` (grok.ts 689, 694, 698, 738).
  - The TS writes happen during `parseLine`, i.e., before the APM handles the
    returned events; if the APM's event handling calls `send()` (finding 2),
    D7's "flush after each call" must still place the parser's writes first.
- **Correction:** Give `spawn` and `encode_stdin_message` the same `DriverIo`,
  list codex's and grok's microtask writes (with the state they set) as
  "run right after spawn returns, before the start continuation's first read",
  and require that queued parser writes are pushed to the writer channel before
  the parsed events are dispatched.

### 12. D7's I/O model has no place for the drivers' model-detection JSON-RPC clients
- **Severity:** major
- **Artifact:** decisions.md D7
- **Failing behavior / source fact:** codex `detectModelsFromAppServer` spawns
  its own app-server, writes `initialize`, reacts to responses by writing
  `initialized` and paginated `model/list` requests from the `data` handler,
  and has a 5 s timeout that kills the child (drivers/codex.ts 1505–1600).
  Grok has a similar ACP detector. These are not `RuntimeDriver` trait calls
  and do not go through `RuntimeSession`, so D7's trait/`DriverIo` design does
  not cover them.
- **Correction:** Add a sub-decision: detection clients are standalone async
  functions owning their child process, line splitter (they `.trim()` lines,
  unlike `runtimeSession`), writes and timeout; state that they never touch
  actor state and return a value.

### 13. D8's "system node on PATH" claim is false
- **Severity:** major
- **Artifact:** decisions.md D8 (lines 253–257)
- **Failing behavior / source fact:** `detectNodeHostKind` returns electron →
  sea → unknown → node (daemon/src/drivers/nodeHostLaunch.ts 98–115);
  `resolveNodeHostLaunch` passes through for node, adds
  `ELECTRON_RUN_AS_NODE=1` for electron, and throws `NodeHostUnavailableError`
  for sea and unknown (156–174). There is no PATH lookup. Consequences D8 does
  not state: gemini (drivers/gemini.ts 132) and opencode (drivers/opencode.ts
  321) call it unconditionally, so on a SEA host they fail to launch where they
  need a node host; codex catches the throw and rejects only the npm_global
  candidate (drivers/codex.ts 150–177). The opencli wrapper uses
  `process.execPath` for non-node hosts (cliTransport.ts 373–441), and whether
  it exists depends on `createRequire(import.meta.url).resolve("@jackwener/opencli")`
  succeeding (307–347), which in a SEA resolves from the executable's location
  plus NODE_PATH/global folders.
- **Correction:** Rewrite: on the Sea host, `resolve_node_host_launch` always
  raises `NodeHostUnavailableError` with the TS message; list the gemini,
  opencode and codex outcomes; specify `resolve_opencli_bin_path` as a
  translation of Node's module-resolution algorithm from `current_exe()`'s
  directory (including NODE_PATH and global folders) or state that it always
  returns null, with the observable effect (no opencli wrapper) recorded.

### 14. Host identity is Sea in D8 and non-Sea in D13; every `isSeaBinary` branch needs a disposition
- **Severity:** major
- **Artifact:** decisions.md D8 vs D13
- **Failing behavior / source fact:** D8 makes the Rust binary take SEA
  branches; D13's rejection relies on the non-SEA branch of
  `serviceUpgradeStart` (serviceUpgradeStart.ts 67–72). Other SEA branches:
  `buildResidentSpawn` (computer/src/service.ts 161–180),
  `ensureSeaRuntimePackageDir` (361–374), `resolveResidentSlockCliPath`
  (386–393), `spawnDetachedService`'s resident binary via
  `resolveKResidentBinary` (265–299), the CLI `upgrade` check
  (computer/src/cli.ts 609–612), and K recovery in `reconcile`.
- **Correction:** Add a table to D8: each `isSeaBinary`/`detectNodeHostKind`
  call site, the branch the Rust binary takes, and the observable result.

### 15. `PI_PACKAGE_DIR` and `runtime-pkg/package.json` leak into agent env from dropped pi code
- **Severity:** major
- **Artifact:** README.md "Dropped" (line 51); decisions.md D8
- **Failing behavior / source fact:** On a SEA host, `ensureSeaRuntimePackageDir`
  writes `<home>/runtime-pkg/package.json` and sets `process.env.PI_PACKAGE_DIR`
  (computer/src/service.ts 361–374). `prepareCliTransport` builds `spawnEnv`
  from `{...process.env}` (cliTransport.ts 820–834), so every agent (not just
  pi) sees `PI_PACKAGE_DIR`. Dropping pi deletes this and changes agent env and
  on-disk files.
- **Correction:** Decide explicitly: keep `ensureSeaRuntimePackageDir` (file and
  env var) because it is observable for kept runtimes, or record the removal as
  an intentional divergence.

### 16. mapping-guide §9's readline rule contradicts `runtimeSession`'s manual splitter
- **Severity:** major
- **Artifact:** mapping-guide.md §9 (line 235)
- **Failing behavior / source fact:** Agent stdout is not split by readline.
  `runtimeSession` feeds chunks through `StringDecoder`, splits on `"\n"` only
  (a `"\r"` stays in the line), skips blank lines, never flushes a trailing
  partial line on exit, and emits the raw `stdout` chunk event before splitting;
  stderr is per-chunk `toString().trim()` (daemon/src/drivers/runtimeSession.ts).
  `BufReader::lines()` delivers the final unterminated line, strips `"\r\n"`,
  and fails with `InvalidData` on invalid UTF-8 instead of substituting U+FFFD.
  Where Node readline is used, it also splits on lone `"\r"`, which `lines()`
  does not.
- **Correction:** Replace the row: agent stdout uses a translated splitter
  (`Utf8StreamDecoder` + split on `\n`, skip empty, drop trailing partial);
  stderr is chunk-trimmed; real readline uses a helper that implements Node's
  `\r`/`\n`/`\r\n` rules with lossy decoding.

### 17. mapping §7's "env read only at entry points" misses the TS env mutations spawned children depend on
- **Severity:** major
- **Artifact:** mapping-guide.md §7 (lines 178–186); decisions.md D14
- **Failing behavior / source fact:** TS mutates `process.env` after entry:
  `__service` action sets `SLOCK_HOME` and `RAFT_COMPUTER_OS_SUPERVISOR_KIND`
  (computer/src/cli.ts 790, 796); `ensureSeaRuntimePackageDir` sets
  `PI_PACKAGE_DIR` (service.ts 370); DaemonCore sets `SLOCK_HOME` when no
  `slockHome` option is given (daemon/src/core.ts 1523); `start` sets
  parent-lock env (computer/src/services/start.ts 471–476); CLI `preAction`
  sets `RAFT_PROFILE` (cli/src/main.ts 240). Module-level code (proxies,
  `resolveRaftHome()` fallbacks, `spawnEnv = {...process.env}`) reads the
  mutated value later. An `Env` snapshot taken at entry and handed to
  long-lived services before a mutation leaves them stale.
- **Correction:** List every TS `process.env` mutation with the point it
  happens and which consumers must see it; require consumers constructed before
  a mutation to receive the post-mutation `Env` (or define the Env as owned by
  the process-level owner and passed after all mutations complete).

### 18. §7's `Env` type ignores Windows case-insensitivity and Node's spawn dedupe
- **Severity:** major
- **Artifact:** mapping-guide.md §7 (line 180)
- **Failing behavior / source fact:** On Windows, `process.env` lookups are
  case-insensitive (`process.env.PATH` returns `Path`'s value), and
  `{...process.env, PATH: x}` produces both `Path` and `PATH` keys
  (cliTransport.ts 820–834 does exactly this). Node's `child_process` on
  Windows sorts env keys and keeps the first of each case-insensitive group
  (so `PATH` beats `Path`). Rust's `Command::env` on Windows dedupes
  case-insensitively with last-write-wins value and first-seen key spelling.
  With conflicting values, the child sees a different PATH. On Unix, Node
  decodes env keys and values lossily as UTF-8; `OsString` keys preserve bytes
  that Node would have replaced.
- **Correction:** Specify `Env` semantics per platform: case-insensitive
  lookup on Windows, and a spawn serializer that reproduces Node's
  sort-then-keep-first rule; on Unix, lossy UTF-8 for keys and values to match
  Node's round-trip.

### 19. `Promise.race`→`select!` and `Promise.all`→`try_join_all` cancel work that TS lets finish
- **Severity:** major
- **Artifact:** mapping-guide.md §8 (lines 200–201)
- **Failing behavior / source fact:** `withComputerMutationLock` races
  `fn(signal)` against `compromised` (computer/src/concurrency.ts 147–157). On
  compromise TS throws but `fn` keeps running until it observes the abort
  signal; only then do its later writes stop. `select!` drops `fn`'s future at
  its current await, abandoning partially written files and skipping its own
  cleanup. `stopAll` uses `Promise.all(stopAgent...)`
  (agentProcessManager.ts 4869): on one rejection, the other stops continue in
  TS; `try_join_all` cancels them.
- **Correction:** Map `Promise.race` to "spawn/keep the losers running unless
  the source aborts them" (e.g., `select!` over `&mut` futures that are then
  awaited or spawned to completion), and `Promise.all` to `join_all` followed by
  first-error selection when the source relies on side effects of the
  remaining promises.

### 20. D13's retirement rule removes ops that TS keeps, changing `lifecycleAcks` on the wire
- **Severity:** major
- **Artifact:** decisions.md D13 (lines 390–393)
- **Failing behavior / source fact:** `retireCompletedUpgradeShutdownsFromLog`
  removes only the shutdown phase of upgrade operations whose `requestId`
  appears in `upgrade.log` with outcome ok; ready phases stay pending
  (computer/src/lifecycleOperations.ts 273–291). Pending phases are
  what `readPendingLifecycleAcknowledgements` puts into `ready` and
  `machine:shutdown` frames (core.ts 1765–1790, 4193–4250), and what the
  `computer:*` dedupe consults (core.ts 4102–4173). "Retire by action" removes
  whole ops, including ready phases the server may still be waiting to see.
- **Correction:** Either keep TS behavior (no action-based retirement) or
  define the new rule precisely (which phases, which file write, under the
  mutation lock) and record it as an intentional wire divergence with the
  frames it changes.

### 21. `supervisor-mutations-v1` must stay conditional
- **Severity:** major
- **Artifact:** decisions.md D13 (lines 383–384)
- **Failing behavior / source fact:** The capability is added only when
  `computerControlViaSupervisor` holds (core.ts 4193–4250), which the computer
  derives from `supervisorMutationsAttested` (residentLifecycleBridge.ts).
  "Still advertised" reads as unconditional. A runner not under an attested
  service (or under a version-skewed TS service, D12) would advertise a route
  that cannot work.
- **Correction:** Say "advertised under the same condition as TS".

### 22. K-wired hooks in kept code have no disposition
- **Severity:** major
- **Artifact:** README.md "Dropped" (lines 40–52); decisions.md D6, D11, D13
- **Failing behavior / source fact:** Kept paths call dropped K/legacy code:
  `reconcileComputerLifecycleOrigin` → `adoptLegacyKUpgradeOrigin` and
  `onComputerUpgradeReconcile` → `reconcileKUpgradeOnConnect`
  (service.ts 426–516); ready acks via k-carrier `loadOperation` and
  `acknowledgeKReadyReceipt` → `acknowledgeOperation`
  (residentLifecycleBridge.ts); `readKRunnerHold` gates spawns fail-closed
  (computer/src/kRunnerHold.ts; `reconcile` in service.ts 840–1213);
  `resolveKResidentBinary` in spawn (service.ts 265–299);
  `dispatchToKResident` and `stripForwardedCarrierName` in `bootstrapThenRun`
  (index.ts 361–393); `RAFT_COMPUTER_DISPATCHER_PATH` in
  `resolveStableDispatcherPath` (computer/src/macosLoginCarrier.ts 136–160).
  Each changes observable behavior when deleted (e.g., whether `ready` is
  re-emitted after origin adoption; whether runners spawn while a TS-written
  hold file exists on a shared home).
- **Correction:** Add a table to D13: each hook, "unset/no-op/kept", and the
  observable effect, including the co-tenant case D12 promises.

### 23. D14 captures unconditionally; TS gates capture on a POSIX supervised `__service`
- **Severity:** major
- **Artifact:** decisions.md D14 (lines 401–412)
- **Failing behavior / source fact:** `bootstrapSupervisedServiceEnv` runs only
  for `__service` with kind `launchd-user` or `systemd-user` from
  `--os-supervised` or `RAFT_COMPUTER_OS_SUPERVISOR_KIND` (index.ts 318–349);
  all other invocations (the detached `__service` that `start` spawns, the
  macOS login carrier whose plist has no `--os-supervised`,
  macosLoginCarrier.ts, `__run`, `__cli`, every user command) keep the
  inherited env. When it runs, it also sets `SLOCK_HOME` from
  `--slock-home`/`--raft-home` and `RAFT_COMPUTER_SHELL_ENV_STATE` to
  `inherited` or `unavailable:<code>` (inherited by agents and read by status,
  computer/src/status.ts ~360), and prints a stderr message on failure.
  Capture is POSIX-only with `SUPPORTED_SHELLS` = zsh, bash, sh, dash, ksh
  (computer/src/shellEnvCapture.ts). D14 also conflicts with D11, which says
  `--os-supervised` is ignored (finding 4).
- **Correction:** Restate the facts with the gate, the side effects, and the
  shell allowlist; make D11 and D14 agree on how the kind is parsed.

### 24. A synchronous capture needs its own timeout and group-kill without threads
- **Severity:** major
- **Artifact:** decisions.md D14 (lines 408–410)
- **Failing behavior / source fact:** The TS capture is async: a unix-socket
  server, a detached `-i -l -c` shell process group, a 10 s timeout, a 1 MiB
  cap, and `killGroup` with a delayed SIGKILL (shellEnvCapture.ts). Doing this
  "before any thread" means a single-threaded poll loop; the delayed SIGKILL
  either blocks startup or needs a timer thread, which makes the later
  `set_var` unsound by D14's own argument. Capture is also replace-not-merge
  (`applyCapturedEnv`), so keys absent from the capture must be removed.
- **Correction:** Specify: poll(2)-based single-threaded capture, the exact
  timeout/cap/kill sequence (including whether startup waits for the SIGKILL
  grace), `remove_var` for keys not in the capture, then protected-key restore.

### 25. D11's "reconciles are coalesced" changes TS scheduling
- **Severity:** major
- **Artifact:** decisions.md D11 (lines 328–330)
- **Failing behavior / source fact:** TS runs `await reconcile()` once, then a
  5 s unref'd interval (computer/src/serviceReconcileLoop.ts), plus
  uncoalesced `scheduleReconcile` timeouts (service.ts 840–1213), guarded by
  `starting` and `canSpawn` (computer/src/lib/runnerStateMachine.ts 154–165).
  Overlapping reconciles are expected. Coalescing can swallow a reset-triggered
  `scheduleReconcile(0)` that arrives while a reconcile is in flight. A single
  supervisor task that awaits inside reconcile or the async exit handler also
  blocks IPC, exit handling and readiness during the 10 s + 2 s + 2 s restart
  handoff (computer/src/serviceControl.ts `performServiceSelfRestart`).
- **Correction:** Translate the TS schedule (interval plus independent
  timeouts, each posting a `Reconcile` message) and make awaits in reconcile
  and exit handling continuations so the supervisor keeps serving IPC.

---

## Minors

### 26. D12 lock location and omitted proper-lockfile behaviors
- **Severity:** major (listed here to keep the D12 findings together)
- **Artifact:** decisions.md D12 (lines 354–359)
- **Failing behavior / source fact:** The computer lock is not `<file>.lock`:
  `concurrency.ts` locks `computerDir` with an explicit
  `lockfilePath = join(computerDir, ".lock")` (computer/src/concurrency.ts
  80–116). proper-lockfile 4.1.2 also: registers a `signal-exit` 3.0.7 handler
  that `rmdirSync`s held locks on exit and on signals; clamps `stale` to ≥
  2000 ms and `update` to [1000, stale/2] (default stale/2); probes mtime
  precision (sets `ceil(now/1000)*1000+5`, `s` vs `ms`); treats "mtime not
  ours" or ENOENT on refresh as compromise; retries via `retry` 0.12 on any
  error; does no pid check (the comment in concurrency.ts claiming one is
  wrong). Without the exit/signal cleanup, a Ctrl-C'd Rust command leaves
  `.lock` and blocks the next command for 60 s where TS does not. Unlisted
  users: `machineOperationStore.ts:209` (stale 30 s, update 10 s, 50 retries)
  and the `cleanup.ts:365` reclaim.
- **Correction:** Correct the path, list every lock site with its options, and
  enumerate the behaviors above (exit/signal cleanup first) as part of
  "exactly".

### 27. The daemon machine lock is a separate protocol, missing from D12
- **Severity:** minor
- **Artifact:** decisions.md D12
- **Failing behavior / source fact:** daemon/src/machineLock.ts uses an mkdir
  `daemon.lock` directory with `owner.json` (schemaVersion 2, token, pid;
  release rewrites pid to 0), a 30 s incomplete-lock stale rule, ESRCH-only
  death, and the "Another Slock daemon is already running" text. It is the
  only exclusion between a TS and a Rust runner on one home.
- **Correction:** Add it to D12's interop list with its own golden test.

### 28. `current_exe()` canonicalization is platform-dependent in Node
- **Severity:** minor
- **Artifact:** decisions.md D11 (lines 316–319); mapping-guide §7 (line 193)
- **Failing behavior / source fact:** libuv realpaths `process.execPath` on
  macOS and Linux, not on Windows (`GetModuleFileNameW`). Rust's macOS
  `current_exe` does not canonicalize; `std::fs::canonicalize` on Windows
  resolves junctions and returns `\\?\` paths. The plist uses
  `path.resolve(process.execPath)` or `RAFT_COMPUTER_DISPATCHER_PATH`
  (macosLoginCarrier.ts 136–160). Paths are embedded in wrappers and plists.
- **Correction:** Define `node_exec_path()`: realpath on macOS/Linux, raw
  `GetModuleFileNameW` on Windows; keep the dispatcher override.

### 29. Windows spawn flags in D11 don't match libuv
- **Severity:** minor
- **Artifact:** decisions.md D11 (lines 322–324)
- **Failing behavior / source fact:** libuv sets `STARTF_USESHOWWINDOW` +
  `SW_HIDE` for `windowsHide`, and adds `CREATE_NO_WINDOW` only when no stdio
  slot is an inherited fd; the service's stdio is the log fd
  (service.ts 265–299), so TS does not pass `CREATE_NO_WINDOW`. The `__run`
  child is not detached but is spawned with `windowsHide` (service.ts
  `spawnChild`); without `SW_HIDE`, each runner (and, through its console,
  each agent) can show a console window.
- **Correction:** Specify the flags per spawn site from libuv's
  `process.c`, including `SW_HIDE` for `__run`.

### 30. Windows pipe and argv quoting need golden tests
- **Severity:** minor
- **Artifact:** decisions.md D12 (lines 351–353); mapping-guide §9
- **Failing behavior / source fact:** libuv creates pipe instances with
  `FILE_FLAG_FIRST_PIPE_INSTANCE` only on the first one; tokio's
  `ServerOptions` defaults to `reject_remote_clients(true)`, and the handoff
  (`releaseListener`, computer/src/internal/ipc-server.ts) creates a new first
  instance while the old instance still serves the restart request. Separately,
  `shell:true` spawns (`cmd.exe /d /s /c "…"`) and libuv's `quote_cmd_arg`
  differ from Rust std's argument and batch-file escaping.
- **Correction:** Pin the tokio options explicitly and golden-test the handoff
  on Windows; add a `js::spawn_windows_command_line` that reproduces libuv
  quoting.

### 31. `__print-env` matches anywhere in argv
- **Severity:** minor
- **Artifact:** decisions.md D14 (lines 404, 411–412)
- **Failing behavior / source fact:** index.ts 404 checks
  `process.argv.includes("__print-env")`, so any command with that word as an
  argument (e.g., `raft-computer __cli message send __print-env`) enters print-env
  mode and exits 8 on socket error.
- **Correction:** State that the Rust check is also "any argv element", not a
  subcommand match.

### 32. `__cli` must keep real fds and the TS preconditions
- **Severity:** minor
- **Artifact:** decisions.md D8 (lines 252–253)
- **Failing behavior / source fact:** `runBundledRaftCli` sets
  `process.argv=[execPath,"slock",...]` and imports the CLI dist
  (core.ts 1255–1262), so `enforceSupportedNodeRuntime` runs
  (cli/src/index.ts) and `forwardManagedTransportIfNeeded` may `spawnSync` the
  wrapper with `stdio:"inherit"` (cli/src/auth/managedTransport.ts,
  cli/src/main.ts 400). The `Io` trait (mapping §9) must hand real fds to that
  child, and exit is `status ?? 1`.
- **Correction:** State that `__cli` passes real stdio, runs the translated
  runtime check (or records its removal), and uses the multi-thread runtime
  question resolved (finding 36).

### 33. `attach` temp+rename breaks the drop-in contract
- **Severity:** minor
- **Artifact:** decisions.md D12 (lines 362–364)
- **Failing behavior / source fact:** The brief requires identical on-disk
  behavior; TS writes attach state in place. A "deliberate hardening" is a
  behavior change that also alters inode/mode handling for co-tenant readers.
- **Correction:** Move it to Open items for user sign-off.

### 34. D6's `CoreHost` hook list is incomplete
- **Severity:** minor
- **Artifact:** decisions.md D6 (lines 204–206)
- **Failing behavior / source fact:** The computer passes `lifecycleHooks`
  (onConnect writes the connected marker, the supervisor's readiness evidence;
  onHandshakeRejected → exit 77), `onComputerRestartReconcile`,
  `onComputerLifecycleReceipt`, and `computerControlViaSupervisor`
  (service.ts 426–516; core.ts 4316–4403).
- **Correction:** List every host hook with sync/async shape from core.ts
  1013–1063.

### 35. Connection writer and replay semantics are unstated
- **Severity:** minor
- **Artifact:** decisions.md D6
- **Failing behavior / source fact:** `connection.send()` is synchronous,
  checks `readyState === OPEN`, and otherwise queues replayable messages or
  drops them (daemon/src/connection.ts 196–222). With a separate writer task,
  frames accepted by the channel but not written before close are lost instead
  of replayed.
- **Correction:** Keep the open check and the replay queue in the actor;
  requeue replayable frames the writer did not flush.

### 36. Runtime flavor for `__cli` is undefined
- **Severity:** minor
- **Artifact:** mapping-guide.md §8 (lines 198–199); decisions.md D8
- **Failing behavior / source fact:** §8 says the CLI uses a current-thread
  runtime and the computer a multi-thread one; `__cli` runs `raft_cli::run`
  inside the computer binary.
- **Correction:** State that `__cli` builds its own current-thread runtime
  (as `raft` does) before calling `raft_cli::run`.

### 37. §8's unref rule is one-sided
- **Severity:** minor
- **Artifact:** mapping-guide.md §8 (lines 205–206)
- **Failing behavior / source fact:** Node also keeps the process alive for
  ref'd handles after `main` logic ends (pending sockets, ref'd timers, child
  handles), so fire-and-forget ref'd work completes before exit. Returning from
  Rust `main` kills it.
- **Correction:** Add: ref'd pending work in the source must be awaited before
  `main` returns.

### 38. `home_dir` precedence differs by platform
- **Severity:** minor
- **Artifact:** mapping-guide.md §7 (lines 191–192)
- **Failing behavior / source fact:** libuv `uv_os_homedir` checks `HOME` on
  Unix and `USERPROFILE` on Windows; Windows does not consult `HOME`.
- **Correction:** Spell out the per-platform order.

### 39. Cross-artifact conflicts not covered above
- **Severity:** minor
- **Artifact:** README.md, mapping-guide.md, decisions.md
- **Failing behavior / source fact:**
  - README line 23 says `raft-computer` "runs as an OS service"; D11 says
    upstream runs no OS service manager and forbids adding one.
  - README line 37 lists hidden commands `__service`, `__run`, `__cli` but not
    `__print-env`, which D14 requires.
  - README line 52 drops the `lib/` public API; D12 governs
    `lib/ipc_client.rs`, which the CLI itself uses.
  - mapping §4 (lines 108–110) allows `std::process::exit` only in `main` and
    for `__run` 77/78; the service and resident also exit directly
    (`serviceShutdown` exit 0 after the 10 s barrier,
    computer/src/lib/serviceShutdown.ts; print-env exit 8; onHandshakeRejected
    77 in the service hooks).
  - D6 says the actor never awaits other components but gives `CoreHost` an
    async `on_computer_control`; TS runs it detached
    (`Promise.resolve().then`, core.ts 4102–4173), and its `emitUpgradeProgress`
    / `emitUpgradeDone` callbacks call `connection.send` from outside the actor.
  - Legacy wrapper upgrade (pre-marker wrappers in
    `upgradeExistingAgentWrappers`) and `regenerateExistingOpencliWrappers`
    (cliTransport.ts 460–487) are legacy-shaped code in kept paths; README's
    "legacy migration" drop does not say whether they stay.
- **Correction:** Fix the README lines; extend the §4 exception list; state
  that `on_computer_control` is spawned and its emitters post messages to the
  actor; mark those two wrapper routines as kept.
