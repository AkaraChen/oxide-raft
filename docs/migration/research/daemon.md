# Research: porting `packages/daemon` (DaemonCore) to `crates/raft-daemon-core`

Source: `upstream/raft-source/packages/daemon` @ 05f7d8f (daemon v1.0.25). All paths below are relative to
`upstream/raft-source/packages/` unless noted. Line numbers are 1-based and point into the pinned tree.

Target: library crate `crates/raft-daemon-core` (currently a 14-line stub, `crates/raft-daemon-core/src/lib.rs`),
embedded into `raft-computer`, whose `__run <serverId>` today does `new DaemonCore(opts)`.

Scope rules applied (from the task): port only what `src/core.ts` transitively needs; no standalone daemon
binary; DROP `builtin`/`pi` and `kimi-sdk` runtimes; KEEP claude, codex, gemini, grok, cursor, copilot,
opencode, kimi (CLI); KEEP OAR account usage; replace `@modelcontextprotocol/sdk` with `rmcp`; no
auto-update; drop legacy migration paths; stay byte/wire compatible with server + spawned agents.

---

## 1. DaemonCore public surface used by `computer`

The host is `computer/src/service.ts`.

- `ResidentCore` interface: just `{ start(): Promise<void>; stop(): Promise<void> }` (`computer/src/service.ts:319-328`).
- `defaultCoreFactory` (`computer/src/service.ts:395-517`):
  - Calls `ensureSeaRuntimePackageDir` → sets `PI_PACKAGE_DIR`. This is a pi-only hack, so DROP it.
  - Dynamically imports `@botiverse/raft-daemon/core`; if the module is missing, it exits with EX_CONFIG `78`.
  - Constructs `new DaemonCore({...})` with:
    - `...residentCoreIdentity(creds)` (`computer/src/residentCoreIdentity.ts`), which provides:
      - `serverUrl`, `apiKey`;
      - `machineOwnerProvenance: { kind: "managed_computer_runner", serverId, serverMachineId }`;
      - `daemonVersion = BUNDLED_DAEMON_VERSION`, `computerVersion = COMPUTER_VERSION`.
    - `localTrace: true`.
    - `lifecycleHooks`:
      - `onConnect` writes the "connected" marker; `onDisconnect` clears it.
      - `onHandshakeRejected` → `classifyTerminalHandshakeRejection` → `markTerminalUnlinked` → `process.exit(77)`.
    - `slockCliPath: resolveResidentSlockCliPath(isSea)`: `"__cli"` when SEA, else `$RAFT_COMPUTER_CLI_PATH`, else `undefined`.
    - Lifecycle-ack hooks:
      - `getComputerLifecycleAcks()` and `getComputerLifecycleReadyAcks()` are **synchronous** (`computer/src/lifecycleOperations.ts:186-191`), because core embeds them in sync-built `machine:ready` / `machine:shutdown` frames.
      - `onComputerLifecycleReceipt(operationId, phase)` goes to the lifecycle bridge (`acknowledgeLifecycleReceipt` removes a pending phase).
    - `reconcileComputerLifecycleOrigin: adoptLegacyKUpgradeOrigin`.
    - `computerControlViaSupervisor`, which adds supervisor capabilities to ready.
    - `onComputerControl(action, ctx)`: `enqueueLifecycleOperation`, then restart → `requestServiceRestartViaIpc` and upgrade → `requestServiceUpgradeViaIpc`.
    - Reconcile hooks:
      - `onComputerUpgradeReconcile` → `reconcileKUpgradeOnConnect`.
      - `onComputerRestartReconcile`: if a pending restart marker exists, it calls `emitDone ok`.
- `runResident` (`computer/src/service.ts:717-751`):
  - SIGTERM/SIGINT → `await core.stop()` then `exit(0)`.
  - Main path: `await core.start()`, then `writeRunnerVersionEvidence`.
- `classifyRunnerExit` treats these as special:
  - the message regex `Another Slock daemon is already running` + `pid=N` means a lock conflict (see §7);
  - exit code 77 means unlinked;
  - exit code 78 means a config error.
  - **The error message text is part of the host contract.**
- `__cli` busybox: `computer/src/cli.ts:887-897` → `runBundledRaftCli(argv.slice(3))`.
  - `runBundledRaftCli` itself lives in `daemon/src/core.ts:1255`. It sets `process.argv=[execPath,"slock",...argv]` and imports `@botiverse/raft/dist/index.js`.
  - In Rust this becomes `raft-computer __cli ...` → `raft-cli` crate.

`DaemonCoreOptions` (`daemon/src/core.ts:978-1070`) groups into:

- Identity:
  - `serverUrl`, `apiKey`, `daemonVersion`, `computerVersion`, `hostname`, `osDescription`, `machineOwnerProvenance`.
- Paths:
  - `slockCliPath`, `dataDir`, `slockHome`, `machineStateDir`.
- Test seams:
  - `runtimeDetector`, `connectionOptions`, `connectionFactory`, `agentManagerFactory`;
  - `defaultAgentEnvVarsProvider`, `reminderClock`, `tracer`;
  - `migrationTransport`, `runtimeAccountUsageCollector`.
- Tracing:
  - `localTrace*`.
- Computer lifecycle:
  - `lifecycleHooks{onConnect,onDisconnect,onHandshakeRejected}`;
  - `getComputerLifecycleAcks`, `getComputerLifecycleReadyAcks`, `onComputerLifecycleReceipt`;
  - `reconcileComputerLifecycleOrigin`;
  - `onComputerControl`, `computerControlViaSupervisor`;
  - `onComputerUpgradeReconcile`, `onComputerRestartReconcile`.
- `ComputerControlContext` (`core.ts:1090`) is `{operationId, requestId, emitUpgradeProgress, emitUpgradeDone}`.

Other exports the host relies on: `readDaemonVersion` (`core.ts:1186`; baked `__RAFT_DAEMON_VERSION__`, else package.json),
`resolveRaftCliPath` (`core.ts:1212`, `dist/cli/index.js`), and the re-export of `legacySupervisor` (DROP).

**Rust shape suggestion (fact-derived):**
- Use a `DaemonCore::new(opts) -> Self` plus `async start(&self)` / `async stop(&self)`.
- Model the lifecycle hooks as a trait object. The ack getters must be sync (`fn`, not `async fn`).
- `onComputerControl` returns a future and must not block the message loop (`core.ts:4144-4168`).

---

## 2. Module graph (transitive from `src/core.ts`, relative imports only)

The graph was computed by walking `import`/`export … from` edges starting at `src/core.ts`, excluding `*.test.ts`.

- It covers **117 modules / 49,210 lines** (`wc -l`), out of 50,815 non-test src lines overall.
- Modules not reached from core:
  - `index.ts`, `historyFormatting.ts`, `agentO11yClient.ts`;
  - `testing/*`, `*.typeproof.ts`.

Classification legend:
- **KEEP**: port it.
- **DROP(pi)**: builtin/pi runtime.
- **DROP(kimi-sdk)**.
- **DROP(legacy)**: legacy migration / legacy daemon.
- **UNSURE**: live feature that is not clearly in scope.

| lines | module | class | note |
|---:|---|---|---|
| 135 | agentActivityProducer.ts | KEEP | |
| 828 | agentAppInbox.ts | KEEP | persisted v3 JSON (§7) |
| 1727 | agentCredentialProxy.ts | KEEP | §6 |
| 350 | agentInboxDeliveryDebt.ts | KEEP | |
| 180 | agentInboxProjection.ts | KEEP | |
| 611 | agentInboxStateMachine.ts | KEEP | |
| 437 | agentLifecycleRecord.ts | KEEP | |
| 685 | agentMigrationExport.ts | UNSURE | server-driven agent migration (`machine:migration_transport:lease`) |
| 701 | agentMigrationHttpTransport.ts | UNSURE | |
| 395 | agentMigrationImport.ts | UNSURE | |
| 689 | agentMigrationObjectStoreBundle.ts | UNSURE | tar-stream + gzip |
| 1001 | agentMigrationResumableBundle.ts | UNSURE | |
| 140 | agentMigrationWorkspaceArchive.ts | UNSURE | |
| 36 | agentMigrationWorkspacePath.ts | UNSURE | |
| 296 | agentNoProcessResidency.ts | KEEP | |
| 8128 | agentProcessManager.ts | KEEP | god object (§4); contains pi/kimi-sdk branches to delete |
| 83 | agentProxyInboxCoordinator.ts | KEEP | |
| 474 | agentRuntimeInput.ts | KEEP | wake/turn prompt text (wire-visible to agents) |
| 257 | agentStartCoordinator.ts | KEEP | start queue |
| 69 | agentStartDispatchProjection.ts | KEEP | |
| 149 | agentStartPendingDeliveryBuffer.ts | KEEP | |
| 55 | agentStatusTransitionTrace.ts | KEEP | |
| 286 | agentVisibleDeliveryLedger.ts | KEEP | in-memory only |
| 972 | apmStateMachine.ts | KEEP | pure reducers (good first port) |
| 221 | apps/cleaner/configReceiver.ts | KEEP | |
| 160 | apps/cleaner/definition.ts | KEEP | |
| 584 | apps/cleaner/runtime.ts | KEEP | |
| 76 | apps/reminder/inboxDefinition.ts | KEEP | |
| 1283 | apps/reminder/reminderCache.ts | KEEP | |
| 735 | apps/reminder/runtime.ts | KEEP | |
| 26 | attachmentFormatting.ts | KEEP | |
| 59 | axExampleFixtures.ts | KEEP (trivial) | doc/example fixtures for `axSurface`; likely unnecessary in Rust |
| 127 | chatBridgeRequest.ts | KEEP | |
| 543 | cindy.ts | KEEP | onboarding seed workspace content |
| 66 | claudeStartupCrashDiagnostic.ts | KEEP | |
| 140 | computerMigrationGuard.ts | DROP(legacy) | `assertLegacyDaemonKeyNotAdoptedByComputer`, called from `start()` |
| 606 | connection.ts | KEEP | §3 |
| 4479 | core.ts | KEEP | |
| 218 | daemonFetch.ts | KEEP | `createProviderHttpClient` is pi-only |
| 89 | daemonOrphanReaper.ts | KEEP | |
| 191 | directUploadCapability.ts | KEEP | used by feedbackTranscriptCollector + traceBundleUpload |
| 208 | drivers/antigravity.deprecated.ts | UNSURE | not in keep list; still registered in `drivers/index.ts` |
| 189 | drivers/claude.ts | KEEP | |
| 395 | drivers/claudeEventNormalizer.ts | KEEP | |
| 59 | drivers/claudeInputBudget.ts | KEEP | |
| 135 | drivers/claudeLaunch.ts | KEEP | |
| 105 | drivers/claudeProviderIsolation.ts | KEEP | |
| 865 | drivers/cliTransport.ts | KEEP | §5 |
| 1655 | drivers/codex.ts | KEEP | JSON-RPC client |
| 795 | drivers/codexEventNormalizer.ts | KEEP | |
| 56 | drivers/codexHome.ts | KEEP | |
| 143 | drivers/codexInstructionShape.ts | KEEP | |
| 83 | drivers/codexTelemetrySidecar.ts | KEEP | |
| 227 | drivers/copilot.ts | KEEP | |
| 265 | drivers/cursor.ts | KEEP | |
| 292 | drivers/gemini.ts | KEEP | |
| 64 | drivers/geminiEventNormalizer.ts | KEEP | |
| 808 | drivers/grok.ts | KEEP | ACP JSON-RPC client |
| 400 | drivers/grokEventNormalizer.ts | KEEP | |
| 17 | drivers/grokHome.ts | KEEP | |
| 73 | drivers/index.ts | KEEP | remove builtin/pi/kimi-sdk factories |
| 1120 | drivers/kimi-sdk.ts | DROP(kimi-sdk) | |
| 367 | drivers/kimi.ts | KEEP | kimi CLI `--wire` |
| 130 | drivers/managedMcpTools.ts | DROP(pi) | `createManagedMcpPiTools`, pi-only |
| 174 | drivers/nodeHostLaunch.ts | KEEP | |
| 622 | drivers/opencode.ts | KEEP | |
| 1981 | drivers/pi.ts | DROP(pi) | |
| 380 | drivers/piCommandTool.ts | DROP(pi) | |
| 91 | drivers/piEventNormalizer.ts | DROP(pi) | |
| 510 | drivers/piToolExecutionObservability.ts | DROP(pi) | |
| 315 | drivers/probe.ts | KEEP | |
| 595 | drivers/raftCliGuide.ts | KEEP | system-prompt text source |
| 408 | drivers/runtimeArtifacts.ts | KEEP | has pi/kimi-sdk string refs |
| 216 | drivers/runtimeSession.ts | KEEP | |
| 230 | drivers/systemPrompt.ts | KEEP | |
| 567 | drivers/types.ts | KEEP | |
| 13 | drivers/windowsPowerShellEnv.ts | KEEP | |
| 143 | feedbackTranscriptCollector.ts | KEEP | |
| 124 | feedbackTranscriptWindow.ts | KEEP | |
| 273 | launchPhaseTransition.ts | KEEP | |
| 7 | launchProxyCleanup.ts | KEEP | |
| 225 | legacySupervisor.ts | DROP(legacy) | re-exported by core; used by computerMigrationGuard |
| 56 | logger.ts | KEEP | |
| 23 | loopbackNoProxy.ts | KEEP | |
| 186 | machineLock.ts | KEEP | |
| 587 | managedMcpRuntimeProxy.ts | KEEP | → rmcp |
| 28 | onboardingSeedContent.ts | KEEP | |
| 111 | providerConnectionLaunch.ts | KEEP | has builtin/pi refs |
| 320 | proxy.ts | KEEP | |
| 88 | proxyFailureTrace.ts | KEEP | |
| 63 | raftHome.ts | KEEP | |
| 130 | registry.manifest.ts | KEEP | |
| 106 | runtimeAccountUsage/collector.ts | KEEP | OAR |
| 149 | runtimeAccountUsage/oarAdapter.ts | KEEP | OAR |
| 240 | runtimeBusyDeliveryCoordinator.ts | KEEP | |
| 106 | runtimeCommunicationTrace.ts | KEEP | |
| 61 | runtimeCompactionProjection.ts | KEEP | |
| 84 | runtimeErrorDeliveryPolicy.ts | KEEP | |
| 403 | runtimeErrorDiagnostics.ts | KEEP | has pi/kimi-sdk refs |
| 69 | runtimeEventTrace.ts | KEEP | |
| 93 | runtimeInputByteMetrics.ts | KEEP | |
| 135 | runtimeLaunchVersion.ts | KEEP | |
| 54 | runtimeModelSourceProjection.ts | KEEP | |
| 237 | runtimeNotificationState.ts | KEEP | |
| 68 | runtimeOutputWindow.ts | KEEP | |
| 370 | runtimeProcessBindingFence.ts | KEEP | |
| 54 | runtimeProgressState.ts | KEEP | |
| 30 | runtimeTelemetrySanitization.ts | KEEP | |
| 107 | runtimeTurnState.ts | KEEP | |
| 287 | scopedAppStorage.ts | KEEP | |
| 234 | scopedAppStorageObservability.ts | KEEP | |
| 26 | secretFile.ts | KEEP | |
| 214 | sessionTranscriptReader.ts | KEEP | |
| 73 | spawnFailureClassification.ts | KEEP | |
| 267 | traceBundleUpload.ts | UNSURE | uploads local traces to worker; fail-open, env-disableable |
| 371 | wikiAgentWorkspace.ts | KEEP | |
| 168 | workspaces.ts | KEEP | |

**Totals (wc lines):**

| class | modules | lines |
|---|---:|---:|
| DROP(pi) | 5 | 3,092 |
| DROP(kimi-sdk) | 1 | 1,120 |
| DROP(legacy) | 2 | 365 |
| UNSURE: agent migration (7 modules) | 7 | 3,647 |
| UNSURE: antigravity.deprecated | 1 | 208 |
| UNSURE: traceBundleUpload | 1 | 267 |
| **KEEP** | **100** | **40,511** |
| total | 117 | 49,210 |

Caveats:
- KEEP overstates the Rust port size. It includes pi/kimi-sdk/builtin branches inside `agentProcessManager.ts`, `runtimeArtifacts.ts`, `runtimeErrorDiagnostics.ts`, `providerConnectionLaunch.ts`, and `drivers/index.ts`, plus legacy branches in `core.ts`:
  - legacy-path warnings;
  - the `agent-inbox/<id>.json` quarantine;
  - the `reminders/mirror.json` quarantine;
  - the legacy `agent-token` file path in cliTransport;
  - `LEGACY_CLAUDE_PROVIDER_CONFIG_DIR` warnings.
- If the migration modules are kept, KEEP is 44,158. If migration and trace upload are kept, it is 44,425.

External dependencies reached (non-`node:`):

| package | used by | Rust plan |
|---|---|---|
| `@botiverse/raft-shared` | ~65 modules, plus subpaths `appConfigTransport`, `appRuntimeTrace`, `apps/cleaner/configProtocol`, `apps/reminder/protocol` | `crates/raft-shared` |
| `@botiverse/raft-trace-client` | core, traceBundleUpload | `crates/raft-trace-client` |
| `@botiverse/raft/dist/index.js` | core `runBundledRaftCli` | `crates/raft-cli` |
| `@botiverse/oar` | runtimeAccountUsage/* | `crates/oar` |
| `@modelcontextprotocol/sdk` | managedMcpRuntimeProxy | rmcp |
| `ws`, `https-proxy-agent`, `undici` | connection, proxy, daemonFetch | tokio-tungstenite + reqwest (proxy-aware) |
| `tar-stream` | migration bundles only | (UNSURE) |
| `@botiverse/kimi-code-sdk`, `@earendil-works/pi-ai`, `@earendil-works/pi-coding-agent`, `typebox` | dropped modules only | DROP |

`package.json` also lists `@jackwener/opencli` (runtime resolution only; see the opencli wrapper in §5), `ajv`, `commander`, `safe-regex2`, and `zod`. None of these is imported by the graph modules.

---

## 3. Server connection (`src/connection.ts`, 606 lines)

- URL: `serverUrl.replace(/^http/, "ws") + "/daemon/connect"`. Header `Authorization: Bearer <apiKey>`.
- Proxy: via `buildWebSocketOptions` (`proxy.ts`). Env precedence is `WSS_PROXY`/`WS_PROXY`/`HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` (either case), with `NO_PROXY` matching.
- Frames: JSON text, one message per frame. Types are `ServerToMachineMessage` (`shared/src/index.ts:545-699`) and `MachineToServerMessage` (`shared/src/index.ts:775-903`).
- Timers:
  - Connect timeout 30s.
  - Inbound watchdog 70s: when no inbound traffic arrives, it sends `{type:"ping"}` once, then terminates if still idle.
  - The server also sends `ping`, and core replies `{type:"pong"}` (`core.ts:4098-4100`).
- Reconnect backoff: starts at 1000ms, doubles each time, max 30000ms, and resets on `open`.
- Handshake rejection (`unexpected-response`):
  - Reads the `slock-reason` response header and normalizes it to `/^[a-z0-9_:-]{1,80}$/i`, else `"invalid_header"`.
  - Calls `onHandshakeRejected`.
  - HTTP 401 with `legacy_machine_key_migrated` stops reconnecting.
  - The computer host exits 77 on terminal rejections.
- `send()` while not open drops the message, **except** it keeps the latest `agent:activity` and the latest `agent:session:invalidate` per agent (filtered to the latest launchId).
  - On open these are flushed **before** `onConnect`: invalidations first, then activity.
- Stale-socket guard: every ws callback checks `this.ws !== ws`.
- Clock seam: `Clock { now, setTimeout, clearTimeout }` (`connection.ts:29-33`). `systemClock` is backed by raft-shared `setClockTimeout`/`clearClockTimeout`.

Core connection handling:

- **`handleMessage`** (`core.ts:3630`):
  - Logs a summary, then offers the message to `localScheduleRuntime.handleServerMessage` first. That consumes `reminder.*`, `app_config.upsert`, and `app_config.snapshot` (`registry.manifest.ts:98-115`).
  - It then switches on:
    - `machine:context`;
    - agent messages: `agent:start`, `agent:start:wiki`, `agent:stop`, `agent:reset-workspace`, `agent:inbox:purge`, `agent:deliver`, `agent:runtime_profile:migration`, `agent:runtime_profile:daemon_release_notice`, `agent:workspace:list`, `agent:workspace:read`, `agent:skills:list`, `agent:diagnostic:session_transcript`, `agent:diagnostic:feedback_transcript`, `agent:activity_probe`;
    - machine messages: `machine:workspace:scan`, `machine:workspace:delete`, `machine:runtimes:rescan`, `machine:migration:source_workspace_archive`, `machine:runtime_models:detect`, `machine:runtime_account_usage:refresh`, `machine:migration_transport:lease`, `machine:migration:cancel`;
    - `ping`;
    - `computer:restart`, `computer:upgrade`, `computer:lifecycle:receipt` (`core.ts:3636-4185`).
  - `summarizeIncomingMessage` (`core.ts:1375`) lists them all.
- **Handshake-then-ready (`handleConnect`, `core.ts:4316`)**:
  1. Host `onConnect` hook.
  2. Regenerate opencli wrappers once.
  3. `emitReady()` (`core.ts:4193`) sends `machine:ready` with:
     - `capabilities`: `agent:start`, `agent:stop`, `agent:deliver`, `workspace:files`, `WIKI_WORKSPACE_PACK_CAPABILITY`, and `BUILT_IN_READY_CAPABILITIES` (`registry.manifest.ts:25-27`, i.e. `REMINDER_FIRE_REQUEST_CAPABILITY`), plus supervisor capabilities when `computerControlViaSupervisor` is set;
     - `runtimes`, `runtimeVersions`, `runningAgents`;
     - `hostname`, `os: "${platform} ${arch}"`;
     - `daemonVersion`, `computerVersion`, `migrationTransport`, `lifecycleAcks`.
  4. `reconcileComputerLifecycleOrigin` (`core.ts:4261`): up to 3 attempts at 50ms, gated by connection generation.
  5. Upgrade and restart reconcile hooks.
  6. Per running or idle agent: `agent:session` and `agent:runtime_profile` (source `"connect"`).
  7. `localScheduleRuntime.onConnect()`, then `requestSnapshot(agentId)` per agent. That sends `reminder.snapshot.request` and `app_config.snapshot.request`.
- **`handleDisconnect`**: `core.ts:4466`.
- **`machine:context` → `bindAuthenticatedMachineContext`** (`core.ts:3231`):
  - Creates the `ScopedAppStorageFactory` for `(machineId, serverId)` (§7).
  - On a conflicting rebind it revokes storage and fails closed.
- **`stop()`** (`core.ts:1747`), in order:
  1. Schedule runtime stop.
  2. Invalidate reconcile.
  3. Observer and uploader stop.
  4. `agentManager.stopAll()`.
  5. If connected, `machine:shutdown {reason: computerVersion ? "computer_stop" : "daemon_stop", lifecycleAcks: <shutdown-phase acks>}`.
  6. Migration transport stop.
  7. Disconnect.
  8. Lock release.
- **HTTP calls from core, runner credential mint** (`core.ts:3293` `requestRunnerCredentialOnce`):
  - Request: `POST {serverUrl}/internal/computer/runners/:agentId/credentials` with headers `Authorization: Bearer <machine apiKey>` and `X-Raft-Client: daemon-server-session-worker`.
  - Body: `{scopes: RUNNER_CREDENTIAL_SCOPES, name: "runner:<runtime>:<agentId first 8>"}`, where scopes are `["send","read","mentions","tasks","reactions","server","channels","knowledge","mcp"]` (`core.ts:146-151`).
  - The response `apiKey` must start with `sk_agent_`.
  - Retries: 3 attempts, 250ms apart.
  - Disabled by `SLOCK_AGENT_RUNNER_CREDENTIALS_DISABLED`.
  - Revocation: `DELETE /internal/computer/runners/:id/credentials/:credId` (agentProcessManager `revokeManagedRunnerCredential`).
- **Outbound helpers**: `agent:deliver:ack`, mention-delivery transitions and terminal errors, `agent:status`, and `agent:activity` (with `clientSeq` deduped server-side by `(daemonInstanceId, launchId, clientSeq)`).
- **Account usage**: `machine:runtime_account_usage:refresh` → collector → `machine:runtime_account_usage:snapshot` (`core.ts:4058-4088`). This path is fire-and-forget.
- **computer control**:
  - Dedupe by `operationId ?? requestId` against both the durable acks and the in-memory `handledComputerControlOperationIds` (`core.ts:4102-4123`).
  - Progress and done frames: `computer:upgrade:progress`, `computer:upgrade:done`, `computer:restart:done`.
  - Failure error strings: `control_busy`, `self_relaunch_unavailable`, `computer_control_failed` (`core.ts:4144-4168`).

---

## 4. Agent process management (`src/agentProcessManager.ts`, 8128 lines)

**Constants** (`agentProcessManager.ts:211-446`):

| constant | value | env override |
|---|---|---|
| max concurrent starts | 5 | `SLOCK_DAEMON_MAX_CONCURRENT_AGENT_STARTS` |
| min start interval | 500ms | `SLOCK_DAEMON_AGENT_START_INTERVAL_MS` |
| activity heartbeat | 60s | |
| stdin notification | 3s initial, 15s retry | |
| session-ready retry | 15s | |
| runtime-error backoff | 10s → 5min; fence threshold 3 | |
| spawn-fail backoff | 1s → 30s | |
| credential-mint backoff | 60s → 10min | |
| compaction stale | 5min | |
| review stale | 10min | |
| progress stale | 15min | |
| start timeout | 2min | |
| stalled SIGTERM wait | 10s | |
| trajectory coalesce | 350ms; max trajectory text 2000 | |

**Structure**

- `AgentProcess` (`agentProcessManager.ts:530-581`) keeps timers inside tagged state unions. A typeproof file enforces that no timer is detached: `agentProcessManager.timerStateTypes.typeproof.ts`.
- The start queue is `AgentStartCoordinator` (`agentStartCoordinator.ts:45-257`):
  - It is a synchronous queue plus one `setTimeout` pump (`:131-138`).
  - It exposes `claimStartSlot`, `releaseStartSlot`, `cancelQueued`, `cancelAllQueued`, and `assertInvariants`.
  - `startAgent` (`agentProcessManager.ts:2635`) either rebinds a running, starting, or queued agent, or enqueues a promise whose resolve/reject is held in the queue item. `pumpAgentStartQueue` is at `:2714`.
- Core dedupes `agent:start` by `startDispatchId` using accepted and accepting maps, plus a 1024-entry receipt cache (`core.ts:3507`).
  - `startAgentFromMessage` (`core.ts:3550`): wiki workspace, credential mint, buffers `agent:deliver` messages that arrive while starting (`agentStartPendingDeliveryBuffer.ts`), wake promotion (`selectWakeDeliveryIndex`, `core.ts:1458`), `agentManager.startAgent`, then replays the buffer.
  - Start failure → `reportAgentStartFailure` sends `agent:status inactive` and `agent:activity runtime_unavailable`.

**`startAgentNow`** (`agentProcessManager.ts:2847-3702`), in order:

1. `initializeAgentWorkspace` (`workspaces.ts:12-37`):
   - `mkdir -p`, then write `MEMORY.md` if it is absent, and create `notes/`.
   - Seed files are write-if-absent (cindy onboarding: `cindy.ts`, `onboardingSeedContent.ts`).
   - Then `ensureWikiWorkspaceIfConfigured`.
2. `enforceRuntimeLaunchVersion` (`runtimeLaunchVersion.ts`).
3. Standing prompt `driver.buildSystemPrompt(runtimeConfig, agentId)` (`:2966`). The turn prompt is composed from about 10 sources (`agentRuntimeInput.ts`).
4. Deferred spawn if the driver's lifecycle says `defer-until-message` (opencode).
5. `buildSpawnConfig`:
   - Mints a credential again. **This duplicates the mint logic in core** (`:3876-3972`).
   - `materializeProviderConnectionForSpawn` (`providerConnectionLaunch.ts`).
   - Merges `defaultAgentEnvVarsProvider` env.
6. Runtime context includes `agentProxyInboxCoordinator`.
7. Session creation: `driver.createSession ?? createChildProcessRuntimeSession` (§5). The spawn uses `detached: false` (`:3659`).
8. Binding fence (`runtimeProcessBindingFence.ts`).
9. Event handlers: `stdout` → `parseLine`, `runtime_event`, `stderr` (Codex reconnect noise filtered), `error`, `exit`, `close`.
10. After spawn: `agent:status active` + activity "Starting…", then arm the startup timeout.

**Close handling** (inside `startAgentNow`):

- Resume recovery: send `agent:session:invalidate`, then cold start.
- Clean exit: restart if a message is queued, else cache an idle restart snapshot.
- Recoverable error: backoff restart.
- Startup timeout.
- Nonrecoverable error: inactive.
- The decision logic is split into pure reducers in `apmStateMachine.ts` (972 lines; `reduceApm*` exports at `:345-640`, no IO).

**Stop**

- `stopAgent` (`:4067`):
  1. `cleanupLaunchProxies` (`launchProxyCleanup.ts` unregisters the credential proxy and the MCP proxy).
  2. Revoke the credential.
  3. `runtime.stop({signal:"SIGTERM", forceAfterMs: wait ? 5000 : …})`. With wait, there is a 5s timeout and then SIGKILL.
- `stopAll` (`:4849`): silent stops, then `reapOrphanProcesses(pids)` (`daemonOrphanReaper.ts:33-89`). That probes with `kill(pid,0)`, SIGKILLs survivors, and polls every 50ms for up to 2s.
  - **Only direct child pids are killed. There is no process-group kill and no `detached`**, so grandchildren (e.g. an MCP server a CLI spawned) are not reaped by the daemon.
- `sendStdinNotification` (`:7604`) is behind many suppression gates (busy, compaction, review, turn state, notification state). `sendAgentStatus` is at `:7566`.

---

## 5. Drivers, system prompt, CLI transport

### 5.1 Driver contract (`drivers/types.ts`, 567 lines)

- `RuntimeDriver` fields:
  - `id`;
  - `lifecycle` (`persistent` | `per_turn`, stdin mode, `inFlightWake: steer|spawn_new`, `start`, `exit`);
  - `communication {chat:"slock_cli", runtimeControl}`;
  - `session.recovery`;
  - `model {detectedModelsVerifiedAs, toLaunchSpec}`;
  - `supportsStdinNotification`, `busyDeliveryMode`.
- `RuntimeDriver` methods:
  - `probe()`, `spawn(ctx) → {process}`, `parseLine(line) → ParsedEvent[]`;
  - `encodeStdinMessage`, `buildSystemPrompt(config, agentId)`;
  - optional `createSession`.
- `ParsedEvent` is the union all normalizers target. `SpawnContext` carries agentId, launchId, config, prompt, standingPrompt, workingDirectory, slockHome, slockCliPath, daemonApiKey, cliTransportTraceDir, and runtime context.
- `drivers/runtimeSession.ts` implements `ChildProcessRuntimeSession`:
  - `start` → `driver.spawn`.
  - stdout: StringDecoder utf8 → split on `\n` → `parseLine`. stderr lines are trimmed.
  - `send` writes `encodeStdinMessage(...) + "\n"`.
  - `stop(signal, forceAfterMs)` sends the signal, then an unref'd SIGKILL timer.
  - `isAlive` uses `kill(pid,0)`, with EPERM treated as alive.
- Driver registry: `drivers/index.ts`. Factories are builtin, claude, codex, grok, antigravity, copilot, cursor, gemini, kimi, kimi-sdk, opencode, pi. Only this file imports pi/kimi-sdk.
- **Drivers are stateful and do IO inside `parseLine`.** Codex, grok, and kimi write JSON-RPC requests/responses to child stdin from within the line parser. For Rust, give the driver a handle to the child's stdin writer (e.g. an mpsc sender), not a pure `fn parse`.

### 5.2 Per-driver launch facts

All drivers call `prepareCliTransport(ctx, extraEnv)` (§5.3) and spawn with `cwd = workingDirectory` and `stdio: pipe×3`.

**Claude** (`claude.ts`, `claudeLaunch.ts`):

- Setup:
  - `extraEnv = buildClaudeProviderIsolationEnv`. For a custom provider (`ANTHROPIC_BASE_URL` + `ANTHROPIC_API_KEY` in config env), this unsets the 34 host keys in `CLAUDE_CUSTOM_PROVIDER_HOST_ENV_KEYS` (`claudeProviderIsolation.ts:12-46`) unless they are set explicitly.
  - Writes the system prompt to `<slockDir>/claude-system-prompt.md` (0600).
  - Managed MCP config file `mcp.json`.
  - `delete env.CLAUDECODE`.
- Args: `--allow-dangerously-skip-permissions --dangerously-skip-permissions --verbose --permission-mode bypassPermissions --output-format stream-json --input-format stream-json --include-partial-messages --model <m|sonnet> --disallowed-tools EnterPlanMode,ExitPlanMode,ScheduleWakeup,CronCreate,CronList,CronDelete --append-system-prompt-file <f>`.
  - Optional: `--effort`, `--setting-sources project,local` (custom provider), `--settings {"fastMode":true}`, `--resume <sid>`, `--mcp-config <f>`.
- First stdin line: `{"type":"user","message":{"role":"user","content":[{"type":"text","text":…}]},"session_id"?}`.
- Windows: shell mode when the resolved path is `.cmd`/`.bat` or unresolved. macOS app-bundle fallback. Known-bad version 2.1.59.

**Codex** (`codex.ts`):

- Binary resolution (`codex.ts:68-255`):
  - Explicit `CODEX_BIN` is authoritative.
  - Win32: npm-global `@openai/codex/bin/codex.js` run via `resolveNodeHostLaunch`. This is **rejected on SEA/unknown hosts** (`NodeHostUnavailableError`), then PATH (skipping `codex-command-runner*`), then `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe`.
  - POSIX: PATH, then darwin desktop bundle paths.
  - Candidates then go through version arbitration.
  - **In the Rust host (SEA-equivalent), the node-JS-entry candidate is unavailable unless a system `node` is found.**
- Args: `app-server [-c mcp_servers.<name>.url="<url>"] --listen stdio://`.
- JSON-RPC:
  1. `initialize {clientInfo:{name:"slock-daemon",version:"1.0.0"}, capabilities:{experimentalApi:true}}`.
  2. `thread/start` or `thread/resume` with `{cwd, approvalPolicy:"never", sandbox:"danger-full-access", sandbox_mode, developerInstructions: <standing prompt>, experimentalRawEvents:true, model, config.model_reasoning_effort, serviceTier:"fast"}`. Resume adds `threadId` and `excludeTurns:true`.
  - All server→client requests are rejected.
  - Resume errors classified as "missing rollout" or "thread writer busy" → fresh thread plus a notice.
  - `CODEX_HOME` defaults to `~/.codex` (`codexHome.ts`).

**Grok** (`grok.ts:399-460`):

- `extraEnv {NO_COLOR:"1"}`. Args `agent --no-leader --always-approve stdio` (ACP JSON-RPC). Lifecycle: persistent.
- The `initialize` request is sent on `queueMicrotask` after spawn, followed by `session/new` or `session/load` with `{cwd, mcpServers:[], _meta:{systemPromptOverride: standingPrompt, yoloMode:true, modelId?}}`.
- Handles `session/request_permission` (auto-approve with fingerprint dedupe) and `_x.ai/interject`.
- `GROK_HOME` falls back to `$HOME/.grok` (`grokHome.ts`).

**Kimi CLI** (`kimi.ts:74-190`):

- Files in the **workspace cwd**:
  - `.slock-kimi-system.md` (the prompt; written unless this is a resume and the file exists);
  - `.slock-kimi-agent.yaml` (`version: 1 / agent: / extend: default / system_prompt_path: ./.slock-kimi-system.md`).
- Args: `--wire --yolo --agent-file <f> --session <id|uuid> [--mcp-config-file <f>] [--model m]`.
- The managed MCP config is `{mcpServers:{<name>:{url, transport:"http"}}}`.
- Wire `initialize {protocol_version:"1.3", client:{name:"slock-daemon",version:"1.0.0"}, capabilities:{supports_question:false, supports_plan_mode:false}}`. Lifecycle: persistent / steer.

**Cursor** (`cursor.ts:40-117`):

- `cursor-agent --print --output-format stream-json --force [--model] [--resume sid] <prompt>`. Lifecycle: per_turn. `shell` on win32.
- Env goes through `withWindowsUserEnvironment` (a PowerShell registry read, `probe.ts:194`).
- Managed MCP is a JSON **overlay** into `<cwd>/.cursor/mcp.json`, with install/restore.

**Gemini** (`gemini.ts:17-77`):

- `--output-format stream-json --yolo -p "" [--model] [--resume sid]`. The prompt goes via **stdin**, not argv. Lifecycle: per_turn.
- Env (each is set only when not explicitly configured):
  - `GEMINI_CLI_TRUST_WORKSPACE=true`;
  - `GEMINI_PTY_INFO=child_process` on win32;
  - `GEMINI_CLI_SYSTEM_DEFAULTS_PATH=<managed mcp settings>`, whose settings are `{mcpServers:{<name>:{httpUrl}}}`.

**Copilot** (`copilot.ts`): `copilot --output-format json --allow-all-tools --allow-all-paths -p <prompt> [--model] [--effort]`. Lifecycle: per_turn.

**OpenCode** (`opencode.ts:197-238`):

- Env `OPENCODE_CONFIG_CONTENT=<json config incl. agent "slock" + managed MCP {url, enabled:true}>`.
- Args: `run --format json --dangerously-skip-permissions --pure --dir <cwd> [--model] [--agent slock (version < 1.15.0)] [--session sid] -- <prompt>`.
  - The prompt is `"No new messages are pending. Stop now."` when there is no turn input.
- Minimum supported version 1.14.30. Lifecycle: per_turn, defer-until-message, terminate-on-turn-end.

**Antigravity** (UNSURE): `--print-timeout … --dangerously-skip-permissions [--continue]`, plus a JSON overlay for MCP.

Probing (`probe.ts`): `which` (POSIX) or PowerShell `Get-Command` (win32, 5s timeout, env via `createWindowsPowerShellChildEnv`, which strips `PSModulePath`). Versions come from `readCommandVersion` (5s timeout). Core's `detectRuntimes` is at `core.ts:1264`.

### 5.3 CLI transport (`drivers/cliTransport.ts`, 865 lines)

This is the most wire-sensitive surface: agents run `raft …` / `slock …` through these wrappers.

- Launch dir:
  - `slockDir = <SLOCK_HOME>/cli-transport/<safe agentId>/<launchId | pid-N>` (`:350`).
  - The legacy `<cwd>/.slock/*` wrapper files are removed.
- Credential paths:
  - Proxy path, when the config has an `sk_agent_` key:
    - Removes the stale `agent-token`.
    - `registerAgentCredentialProxy({activeCapabilities:"send,read,mentions,tasks,reactions,server,channels,knowledge"})`.
    - Writes the proxy token to `<home>/agent-proxy-tokens/<agentId>/<launchPart>.token` (dir 0700, file 0600; `:664`).
  - Legacy path (DROP candidate): writes `authToken || daemonApiKey` to `<slockDir>/agent-token` (0600).
- Wrappers written into `slockDir`: POSIX `slock` and `raft` with identical bodies, mode 0755 (`:707-710`). The body, in order:
  1. `#!/usr/bin/env bash`.
  2. Launch-forwarding guard (`:59-90`). If `SLOCK_AGENT_LAUNCH_DIR` names a different sibling launch dir (no `/`, `\`, `.`, `..`, or symlink), it `exec`s that dir's same-named wrapper.
  3. `unset RAFT_PROFILE SLOCK_PROFILE RAFT_PROFILE_DIR SLOCK_PROFILE_DIR`.
  4. NO_PROXY prelude (`:503-506`) prepending `127.0.0.1,localhost` (`loopbackNoProxy.ts`).
  5. `SLOCK_CLI=<cliPath>`, plus `-e` fallback candidates (skipped for `__cli`).
  6. Optional `export ELECTRON_RUN_AS_NODE=1`.
  7. The env prefix `SLOCK_AGENT_ID=… SLOCK_SERVER_URL=…`, plus either `SLOCK_AGENT_PROXY_URL=… SLOCK_AGENT_PROXY_TOKEN_FILE=… SLOCK_AGENT_ACTIVE_CAPABILITIES=…` or `SLOCK_AGENT_TOKEN_FILE=…`.
  8. `exec <nodeHost.command> "$SLOCK_CLI" "$@"`.
  - Under SEA (i.e. the Rust `raft-computer`), `nodeHost.command = execPath` and `cliPath="__cli"`, so the wrapper runs `exec <raft-computer> __cli "$@"`.
- Windows wrappers: `slock.cmd`/`raft.cmd` and `slock.ps1`/`raft.ps1` (`:712-798`):
  - Both set UTF-8 (`chcp 65001`, `PYTHONUTF8`, `LANG`/`LC_ALL=C.UTF-8`) and include their own forwarding guards.
  - The `.cmd` guard uses `fsutil reparsepoint`.
  - The `.ps1` passes `$input` through.
- The marker `slock-daemon-generated` identifies daemon-owned wrappers. `upgradeExistingAgentWrappers` (`:214`) rewrites prior launches' wrappers to forward to the current launch.
- The `opencli` wrapper (`:373-458`) is written if `@jackwener/opencli` resolves. That resolution is a Node module lookup; in Rust you have to decide how to find it.
- **Spawn env** (`:820-855`):
  1. Base: `process.env`.
  2. `FORCE_COLOR=0`.
  3. Config `envVars`, then driver `extraEnv`.
  4. win32 utf8 env.
  5. Runtime context env (`:536-543`):
     - `SLOCK_CURRENT_AGENT_ID`, `SLOCK_CURRENT_SERVER_ID`;
     - `RAFT_CURRENT_COMPUTER_ID`, `RAFT_CURRENT_COMPUTER_NAME`, `RAFT_CURRENT_COMPUTER_HOSTNAME`, `RAFT_CURRENT_COMPUTER_OS`;
     - `SLOCK_CURRENT_DAEMON_VERSION`, `SLOCK_CURRENT_WORKSPACE_PATH`.
  6. Identity and transport:
     - `SLOCK_HOME`, `SLOCK_AGENT_ID`, `SLOCK_AGENT_LAUNCH_ID`, `SLOCK_CLI_TRANSPORT_TRACE_DIR`, `SLOCK_SERVER_URL`;
     - `SLOCK_AGENT_LAUNCH_DIR=<basename(slockDir)>`, `SLOCK_CLI_TRANSPORT_DIR=<slockDir>`;
     - `PATH=<slockDir>:<PATH>`.
  7. Deleted from the env:
     - `SLOCK_AGENT_TOKEN` and the raw-credential denylist (incl. `SLOCK_AGENT_CREDENTIAL_KEY`);
     - the profile denylist;
     - `SLOCK_AGENT_PROXY_URL`, `SLOCK_AGENT_PROXY_TOKEN`, `SLOCK_AGENT_PROXY_TOKEN_FILE`, `SLOCK_AGENT_ACTIVE_CAPABILITIES`, `SLOCK_AGENT_TOKEN_FILE`.
  8. `applyLoopbackNoProxyEnv`.
  - **Credentials are never in the runtime env, only in the wrapper's exec prefix.**
- Node host (`nodeHostLaunch.ts`): `detectNodeHostKind` returns electron, sea, node, or unknown. `resolveNodeHostLaunch` throws for sea/unknown unless a system node is present.
- `raftHome.ts`: `RAFT_HOME` > `SLOCK_HOME` > `~/.slock`. Core sets `process.env.SLOCK_HOME` in its constructor when `slockHome` is not given (`core.ts:1511-1588`).

### 5.4 System prompt (`drivers/systemPrompt.ts`, 230 lines, plus `raftCliGuide.ts`, 595 lines)

- `buildCliSystemPrompt(config, {extraCriticalRules, commandShell})` (`systemPrompt.ts:65-217`). The prompt contains, in order:
  1. Identity header.
  2. "Current Runtime Context" (`:37-63`; only emitted when there are more than 4 lines).
  3. Instruction precedence.
  4. The CLI-guide sections (communication, credentialHygiene).
  5. CRITICAL RULES.
  6. Startup sequence (with an optional step 0 for `runtimeProfileControl.kind === "daemon_release_notice"`).
  7. Messaging header format.
  8. More guide sections: sendingMessages, reminders, threads, discovery, channelAwareness, readingHistory, historicalReferences, tasks, splittingTasks, mentions, communicationStyle, conversationEtiquette, liveConstraints, formattingMentionsChannels, workspaceAndMemory, compactionSafety.
  9. Optional "Initial role".
- Each driver's `buildSystemPrompt` supplies per-driver `extraCriticalRules` and shell (claude.ts:183, codex.ts:1219, gemini.ts:286, grok.ts:672, cursor.ts:189, copilot.ts:221, kimi.ts:303, opencode.ts:619).
- **Golden fixtures exist**: `src/drivers/__snapshots__/systemPrompt/common.md` plus a per-runtime patch (`claude.patch`, `codex.patch`, `copilot.patch`, `cursor.patch`, `gemini.patch`, `grok.patch`, `kimi.patch`, `opencode.patch`, `configured.patch`; the pi/builtin/kimi-sdk patches are dropped). Use them as byte-exact parity tests.
- Codex puts the prompt in `developerInstructions`, Claude in an `--append-system-prompt-file`, Grok in `_meta.systemPromptOverride`, and Kimi in an agent-file. The per-turn drivers put it in the prompt text.

---

## 6. Credential proxy and managed MCP

### 6.1 `agentCredentialProxy.ts` (1727 lines)

- Registrations live in a **module-global** `Map`:
  - The key is a token `sap_` + 32 random bytes, base64url.
  - Registration and unregistration functions are at `:1672-1727`.
- One shared `http` server on `127.0.0.1:0` (3 bind attempts, `unref`), lazily started.
- Per request:
  1. Validate `Authorization: Bearer sap_…`, then an origin match (403 `agent_proxy_origin_mismatch`).
  2. Strip hop-by-hop, `host`, and auth headers.
  3. Inject `Authorization: Bearer <sk_agent_…>`, `X-Agent-Id`, `X-Raft-Client: cli`, `X-Slock-Agent-Active-Capabilities`, and `traceparent`.
- Locally served routes:
  - `GET` runtime-version;
  - `GET` inbox (local app inbox);
  - `POST inbox/ack` → server `/internal/agent-api/app-sources/ack`;
  - `GET` events (local pending deliveries merged in).
- `POST` send, `tasks/claim`, and `tasks/update-status` go through freshness side-effect preparation, which can **hold** the request until visible deliveries are consumed.
- Buffering and responses:
  - JSON bodies are buffered for send/events/history, and a 5xx becomes a synthesized failure response.
  - Streaming responses pass through with `x-raft-*` headers.
  - The upload headers timeout is 5min (`SLOCK_DAEMON_ATTACHMENT_UPLOAD_HEADERS_TIMEOUT_MS`).
- `routeFamilyForPath`: `:1020`.
- Fetch pre-response timeout: `SLOCK_DAEMON_FETCH_PRE_RESPONSE_TIMEOUT_MS`, default 30s (`proxy.ts`).
- The proxy calls back into APM state (visible-delivery ledger, inbox), so it is **not** a dumb reverse proxy.

### 6.2 `managedMcpRuntimeProxy.ts` (587 lines) → rmcp

- Loopback HTTP server; route `/mcp/<token>`; 1 MiB request cap.
- Per request it builds an MCP `Server{name:"raft-managed-mcp-runtime", version:"1.0.0"}` with `StreamableHTTPServerTransport`, **stateless, JSON responses** (no SSE sessions). In rmcp, use a stateless streamable-HTTP server with `json_response`.
- Tool names: `r<sha256(runtimeName)[0:8]>_<sanitized>`, capped at 48 chars. The MCP server entry name is `rm<6 chars>`.
- Upstream calls, all with `Bearer sk_agent`, `X-Slock-Agent-Active-Capabilities: mcp`, and a 25s timeout:
  - `GET /internal/agent-api/mcp/tools` (catalogVersion 1);
  - `POST /internal/agent-api/mcp/call`.
- Config files: `<home>/managed-mcp-runtime/<agent>/<launch>/<runtime>-<file>` (0600). Cursor and antigravity use JSON overlay install/restore into the workspace instead.

---

## 7. Persistent state (all under RAFT_HOME / SLOCK_HOME unless noted)

| path | writer | format |
|---|---|---|
| `machines/machine-<sha256(apiKey)[0:16]>/daemon.lock/owner.json` | `machineLock.ts:99-186` | JSON `{pid, token, hostname, startedAt, serverUrl, apiKeyFingerprint(16 hex), schemaVersion:2, kind, serverId?, serverMachineId?}` 0600 |
| `machines/<lockId>/traces/daemon-trace-<iso>-<pid>-<seq4>.jsonl` | trace-client `localTraceSink.ts:57-120` | span JSONL, `schema_version:1` |
| `machines/<lockId>/trace-uploads/<file>.uploaded.json` | `traceBundleUpload.ts:238-259` | JSON |
| `app-storage/v1/<machineId>/<serverId>/<appId>/agents/<agentId>/state.json` | `scopedAppStorage.ts:154-181` | app JSON (atomic temp+rename, 0600, dirs 0700) |
| `app-storage/v1/<machineId>/<serverId>/<appId>/computer/state.json` | same | |
| `app-storage-quarantine/v1/unscoped/<uuid>-<leaf>` | `scopedAppStorage.ts:241-281` | legacy files moved here (DROP candidate) |
| `agents/<agentId>/…` (workspace; `MEMORY.md`, `notes/`, seed files) | `workspaces.ts`, core `agentsDataDir` | |
| `cli-transport/<agentId>/<launch>/{slock,raft,*.cmd,*.ps1,opencli,claude-system-prompt.md,agent-token?}` | cliTransport | §5.3 |
| `agent-proxy-tokens/<agentId>/<launch>.token` | cliTransport | token, 0600 |
| `managed-mcp-runtime/<agent>/<launch>/<runtime>-<file>` | managedMcpRuntimeProxy | 0600 |
| workspace `.slock-kimi-system.md`, `.slock-kimi-agent.yaml`, `.cursor/mcp.json` overlay | kimi/cursor drivers | |

Machine lock (`machineLock.ts`):

- The root is `machineStateDir ?? resolveRaftHomePath("machines")`.
- Acquisition:
  - `mkdir daemon.lock` is the atomic primitive.
  - On EEXIST: if the owner pid is alive, raise a conflict. If there is no `owner.json` and the lock is younger than 30s, also raise a conflict. Otherwise remove the lock and retry once.
- Release does **not** delete `owner.json`. It rewrites it with `pid:0`, so the machine identity stays discoverable (`:137-164`).
- The conflict error message (`:48-51`) is matched by the computer host regex. Keep it verbatim.
- The owner kind for the computer is `managed_computer_runner`. `legacy_raw_daemon` is the default for a raw daemon (DROP).

App-state formats:

- **Agent inbox** (`agentAppInbox.ts:235-246`): `{version:3, items:[durable-retention items], acknowledgedSources:[…], ackIntents:[…]}\n`. Restore accepts versions 1–3.
  - appId is `system.agent-inbox`. The legacy `agent-inbox/<id>.json` is quarantined (`core.ts:1591`).
- **Reminder**: appId `system.reminder`, per agent (`registry.manifest.ts:82-92`). Format in `apps/reminder/reminderCache.ts`. The legacy `reminders/mirror.json` is quarantined.
- `agentVisibleDeliveryLedger.ts` is in-memory only.
- The idle-restart snapshot and the start-dispatch receipt cache are in memory.

---

## 8. Apps (reminder, cleaner) and `registry.manifest.ts`

`createBuiltInLocalScheduleRuntime` (`registry.manifest.ts:48-130`) is the only place that names apps.

- It combines the inbox registries (reminder + cleaner) into one.
- `bindScopedStorage(factory)` runs after `machine:context` binds storage.
- `start` and `stop`.
- `handleServerMessage`:
  - `app_config.upsert` and `app_config.snapshot` go to the cleaner via `receiveCleanerConfigMessage`, which traces `daemon.app_config.receive`.
  - Everything else goes to `reminder.handleServerMessage`.
- `beforeAck` and `beforeServerAuthorizedAck` (reminder), `replayPendingReceipts`, `onConnect`.
- `requestSnapshot(agentId)` sends `reminder.snapshot.request` + `app_config.snapshot.request`.
- `requestReminderSnapshotIfUnsynchronized`.

**Reminder** (`apps/reminder/runtime.ts` 735, `reminderCache.ts` 1283):

- Inbound messages: `reminder.upsert {agentId, reminder: ReminderJob}`, `reminder.cancel {agentId, reminderId, version}`, `reminder.snapshot {agentId, reminders}` (`shared/src/index.ts:681-683`), plus `reminder.fire_receipt.ack` and `reminder.fire_request.result` (`shared/src/apps/reminder/protocol.ts:15-50`).
- Outbound messages: `reminder.armed`, `reminder.arm_rejected`, `reminder.fire_request`, `reminder.fire_receipt`, `reminder.fire_attempt` (`protocol.ts:54-95`), and `reminder.snapshot.request` (`index.ts:879`).
- Owner-fence kinds per message: `runtime.ts:88-94`.
- Timers go through the injected `Clock`.
- Scheduling limits: max schedule-ahead delay 24h (it re-arms beyond that), and fire retry 1s → 60s (`reminderCache.ts:271-273`).
- Bounded alert phases and retry deadlines: `runtime.ts:200-300`.
- Ready capability: `REMINDER_FIRE_REQUEST_CAPABILITY`.

**Cleaner** (`apps/cleaner/*`):

- Per-agent config `{appId, ownerAgentId, enabled, thresholdBytes, intervalMs, revision}` (`runtime.ts:25-41`), bounded by `CLEANER_CONFIG_BOUNDS` (shared).
- On each interval it `lstat`s `<agentsDataDir>/<agent>/MEMORY.md` (`runtime.ts:553`), with a measurement timeout. Above the threshold it mints an inbox item and wakes the agent via `notifyInbox`.
- Uses its own `CleanerClock {now, schedule, cancel}` (`runtime.ts:56-66`), which is different from the connection `Clock`.

---

## 9. Tracing and logger

- `logger.ts` (56 lines):
  - Lines look like `"<formatUtcTimestamp> [INFO|WARN|ERROR] <msg>"` and go to console.log/warn/error.
  - `subscribeDaemonLogs(listener)` is a global listener set.
  - Rust equivalent: `tracing` with a custom formatter plus a broadcast layer. The host may parse stdout lines (e.g. the lock-conflict regex).
- Tracer: `@botiverse/raft-trace-client`, `createTraceClient({source:"daemon"})` with a `MultiSink`.
- Local sink `LocalRotatingTraceSink` (`trace-client/src/localTraceSink.ts`):
  - Rotates at 5 MiB or 5 min (plus per-machine jitter from `traceJitter.ts`, keyed by lockId). Keeps 8 files. Files are 0600, the dir 0700.
  - Only allowlisted ID/error attributes are kept raw (`:9-39`).
  - It never throws.
  - Env: `SLOCK_DAEMON_LOCAL_TRACE`, `…TRACE_MAX_FILE_BYTES`, `…MAX_FILE_AGE_MS`, `…MAX_FILES`, `…JITTER_DISABLED`.
- Bundle upload (`traceBundleUpload.ts`, UNSURE):
  - Initial delay, then every 5 min (`SLOCK_DAEMON_TRACE_UPLOAD_INTERVAL_MS`). It only uploads closed, non-current files older than a minimum age.
  - Each file is gzipped; the SHA-256 of the gzip is computed.
  - The attestation comes from `POST /internal/machine/scope-attestation` (`directUploadCapability.ts`). Then `/api/trace-bundles` creates a signed upload that is PUT to the worker (default `https://slock-trace-upload.botiverse.dev`, `core.ts:146`).
  - Uploaded files are marked in `trace-uploads/`.
  - Disable with `SLOCK_DAEMON_TRACE_UPLOAD_DISABLED`; override the URL with `…_URL`.
- The trace `traceparent` is propagated from `agent:deliver` into the proxy headers.

---

## 10. Concurrency and async hazards (what makes the port hard)

1. **Single-threaded implicit atomicity.** All state in core/APM/proxy is mutated from one JS event loop. Many invariants rely on "no await between check and set":
   - start-dispatch dedupe maps (`core.ts:3507`);
   - `handledComputerControlOperationIds`;
   - `AgentStartCoordinator.assertInvariants`.
   - In Rust, use one actor task owning `DaemonState` with an mpsc command channel, rather than `Arc<Mutex>` sprinkled everywhere.
2. **Fire-and-forget promises everywhere in `handleMessage`**:
   - `void …then()` for account usage, computer control, lifecycle receipts, `emitReadyIfConnected`, reminder sends.
   - Their completions mutate shared state later, so each one needs an explicit "send the result back to the actor" path.
3. **Connection-generation fencing.**
   - The ready/reconcile retries (`core.ts:4261`) check that the generation is unchanged.
   - `connection.ts` ignores events from stale sockets (`this.ws !== ws`).
   - Offline-queue semantics: keep only the latest activity/invalidate per agent, flush them before `onConnect`.
4. **Synchronous host callbacks.** `getComputerLifecycleAcks()` is called inside sync frame builders (`computer/src/lifecycleOperations.ts:186-191`). The Rust trait must expose a sync fn, e.g. reading an in-memory mirror.
5. **Timer soup in AgentProcessManager.** There are about 15 timer kinds per agent (heartbeat, stdin-notify retry, session-ready retry, startup timeout, stalled SIGTERM, backoffs, trajectory coalesce, compaction/review/progress stale). They are stored in tagged state unions so a stale timer cannot fire after a state change. Port each timer as a cancellable task keyed by `(agentId, launchId, generation)`.
6. **Closure-captured launch identity.** Child event handlers capture `launchId` and the process, then compare against the current `AgentProcess`. Late events from a previous launch must be dropped (binding fence, `runtimeProcessBindingFence.ts`).
7. **Start-queue promises.** `startAgent` returns a promise resolved or rejected by a later pump. Cancellation (`cancelQueued`, `cancelAllQueued` on stop) must reject waiters. The rebind path mutates the queued item in place.
8. **Pending-delivery buffer during start.** `agent:deliver` messages that arrive while mint/spawn is awaiting are buffered and replayed after start (`core.ts:3550`). The ordering must be preserved.
9. **Module-global registries.** The credential-proxy registrations and the MCP-proxy registrations are process globals with lazily started servers. In Rust, make them owned services inside `DaemonCore`; this also makes tests hermetic.
10. **Driver IO inside the parser.** Codex, grok, and kimi JSON-RPC state machines write to the child's stdin from `parseLine`, and grok defers `initialize` via `queueMicrotask`. The Rust driver needs a stdin sink plus a pending-request map.
11. **The proxy holds requests.** `send` / `tasks/*` can wait on the freshness side-effect gate, which depends on APM state. That is a cross-component async dependency: the HTTP handler awaits the agent state actor.
12. **Duplicate credential-mint logic.** It exists in both core and APM (`agentProcessManager.ts:3876-3972`), with backoff 60s → 10min. Unify it in the port.
13. **Process teardown.** SIGTERM, then SIGKILL after N ms, then the orphan reaper, which only polls direct pids. On Windows there is no signal semantics (`kill()` terminates). Rust must mirror this; process-group kill would be a behavior change.
14. **Wire strings that must stay byte-exact:**
    - wrapper scripts;
    - system prompt;
    - turn inputs (`agentRuntimeInput.ts`);
    - the `slock-daemon-generated` marker;
    - tool-name hashing for MCP;
    - the `slock-reason` normalization;
    - the lock-conflict error text;
    - clientInfo `slock-daemon`/`1.0.0`.

---

## 11. Tests (108 `*.test.ts`, 65,897 lines)

Setup facts:
- `vitest.config.ts`: `include: src/**/*.test.ts`, `pool: forks`, 30s timeouts. Snapshots update locally and are only checked in CI.
- `vitest.sea-host.config.ts` runs `src/testing/seaHostHarness.sea-suite.ts` serially.
- Helpers: `src/testing/drydock.ts` (event probes, `FakeClock`), `fakeClock.ts`, `promptFixture.ts`.
- Process tests use **fake drivers or scripts** (a `#!/usr/bin/env node` script written to tmp in `grok.test.ts:55`, a `#!/bin/sh` stub in `opencode.test.ts:255`, and overridden `spawn` in `agentProcessManager.*.test.ts`).
- Real CLIs are only used in opt-in integration tests.

Classification:

- **Dropped-feature (14 files)**:
  - pi: `drivers/pi.test.ts`, `pi.integration.test.ts`, `piCommandTool.test.ts`, `piEventNormalizer.test.ts`, `piToolExecutionObservability.test.ts`, `piToolExecutionRuntimeSession.test.ts`, `managedMcpTools.test.ts`, `agentProcessManager.builtin.e2e.test.ts`.
  - kimi-sdk: `drivers/kimi-sdk.test.ts`, `releaseRuntimeSdkPreflight.test.ts`.
  - legacy: `legacySupervisor.test.ts`, `computerMigrationGuard.test.ts`.
  - out of graph: `historyFormatting.test.ts`, `agentO11yClient.test.ts`.
  - Mixed files that reference dropped runtimes but are mostly KEEP: `core.test.ts`, `agentProcessManager.codex.test.ts`, `agentProcessManager.sessionTranscript.test.ts`, `drivers/cliTransport.test.ts`, `drivers/runtimeContract.test.ts`, `drivers/systemPrompt.snapshot.test.ts`, `daemonFetch.test.ts`, `runtimeErrorDiagnostics.test.ts`.
- **UNSURE (migration, 6 files)**: `agentMigrationExport`, `…HttpTransport` (net), `…Import`, `…ObjectStoreBundle`, `…ResumableBundle`, `…WorkspaceArchive`. `traceBundleUpload.test.ts` goes with the trace-upload decision.
- **Real-CLI opt-in integration (4, skipped by default)**:
  - `drivers/claude.integration.test.ts` (`RUN_CLAUDE_INTEGRATION_TESTS=1`);
  - `codex.integration.test.ts` (`RUN_CODEX_…`);
  - `grok.integration.test.ts` (`RUN_GROK_…`);
  - `pi.integration.test.ts` (drop).
- **Integration / e2e with fake children, sockets, or tmp fs**:
  - core and managers: `core.test.ts` (5584; proc+net+fs), `agentProcessManager.claude.test.ts` (4113), `agentProcessManager.codex.test.ts` (11234, fake timers), `agentProcessManager.grok|cindy|systemPrompt|sessionTranscript|appInboxTrace.test.ts`;
  - `runnerApp.e2e.test.ts` (net);
  - proxies and networking: `agentCredentialProxy.test.ts` (4320, net), `proxy.test.ts` (net), `managedMcpRuntimeProxy.test.ts`;
  - drivers: `drivers/codex.test.ts` (3550, snapshot), `claude.test.ts`, `claudeLaunch.test.ts`, `gemini.test.ts`, `grok.test.ts`, `kimi.test.ts`, `opencode.test.ts`, `probe.test.ts`, `runtimeSession.test.ts`, `cliTransport.test.ts` (1744, platform-conditional);
  - persistence and apps: `scopedAppStorageObservability.test.ts`, `machineLock.test.ts`, `raftHome.test.ts`, `wikiAgentWorkspace.test.ts`, `workspaces.test.ts`, `agentAppInbox.test.ts`, `scopedAppStorage.test.ts`, `apps/reminder/reminderCache.test.ts` (2012), `apps/reminder/runtime.test.ts`, `registry.manifest.appTrace.test.ts`;
  - `testing/drydock.test.ts`.
- **Pure unit / contract**:
  - core modules: `connection.test.ts` (1390, fake clock), `apmStateMachine.contract.test.ts`, `agentStartCoordinator`, `agentStartPendingDeliveryBuffer`, `agentVisibleDeliveryLedger`, `agentInboxProjection`, `agentInboxStateMachine`, `agentLifecycleRecord`, `agentNoProcessResidency`, `agentActivityProducer`, `agentRuntimeInput` (+ `.snapshot`);
  - runtime state: `launchPhaseTransition`, `runtimeNotificationState`, `runtimeProgressState`, `runtimeTurnState`, `runtimeLaunchVersion`, `runtimeModelSourceProjection`, `spawnFailureClassification`;
  - proxy and transport helpers: `proxyFailureTrace`, `chatBridgeRequest`, `directUploadCapability`, `providerConnectionLaunch`, `agentCredentialProxy.routeFamily`;
  - misc: `feedbackTranscriptWindow`, `logger`, `printSeam`, `task695ContinueOnErrorProbe`;
  - scoped storage and reminder source contracts: `scopedAppStorage.sourceContract`, `reminderRetryRoutes.sourceContract` (these two grep TS source text and do not port), `apps/cleaner/configReceiver`, `apps/cleaner/runtime`, `apps/reminder/inboxDefinition`;
  - drivers: `drivers/claudeEventNormalizer`, `codexBlankFinalAnswer.contract`, `codexInstructionShape`, `copilot`, `cursor`, `geminiEventNormalizer`, `grokEventNormalizer`, `nodeHostLaunch`, `raftCliGuideFreshness`, `systemPrompt`, `systemPrompt.snapshot`, `windowsPowerShellEnv`;
  - account usage: `runtimeAccountUsage/collector`, `oarAdapter`, `oarKimi`.

Porting leverage:
- The normalizer tests (claude, codex, grok, gemini) and the `systemPrompt` snapshots are table-driven, so their fixtures can be reused verbatim as Rust golden tests.
- `connection.test.ts` and `apmStateMachine.contract.test.ts` define the fencing and state semantics most precisely.
