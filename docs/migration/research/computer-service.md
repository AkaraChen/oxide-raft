# raft-computer: service, K/update machinery, status/doctor, installers

Source: `upstream/raft-source/packages/computer` (1.0.28, commit 05f7d8f). All paths below are relative to that package unless they start with `packages/` (that means relative to `upstream/raft-source/`).
Scope: the resident service lifecycle, the K / hands update machinery (to drop), status/doctor output, what the server expects for lifecycle actions, the installers, and a keep/drop call for each test file. IPC, state files, the server API, and setup/login/attach are covered elsewhere.

---

## 0. Correction to the task premise (read first)

**Upstream 1.0.28 does not run the resident service under an OS service manager.**

- The service is a **detached, self-spawned process** (`spawn(..., {detached:true})` + `unref()`), with its pid written to `<home>/computer/run/service.pid` (`src/service.ts:265-299`).
- On **macOS only**, a launchd LaunchAgent (the "login carrier") runs `<dispatcher> __service --slock-home <home>` **at login** (`RunAtLoad`). It has **no `KeepAlive`** (`src/macosLoginCarrier.ts:192-253`). Its only job is to bring the service back after a reboot or re-login.
- On **Linux and Windows** nothing brings the service back after reboot. `convergeCliHostLifecycle` returns `not-applicable` there (`src/macosLoginCarrier.ts:517-528, 886-895`).
- `osSupervisor.ts` / `osSupervisorRuntime.ts` / `osSupervisorLifecycle.ts` generate the **retired** per-home manager definitions: launchd `build.raft.computer.<hash>` with KeepAlive, systemd `raft-computer-<hash>.service` with `Restart=always`, and Windows scheduled task `\Raft-Computer-<hash>`. Today they are used only to **detect and retire** those old definitions (`retireLegacyOsSupervisor`, `src/osSupervisorRuntime.ts:422`). The only callers are `legacyOsSupervisorMigration.ts:19-54` and `cli.ts:862-873` (`__supervisor retire-legacy`, which the installers invoke). All of this falls under the legacy-migration DROP.
- Restart is a **self-handoff protocol**, not an OS restart. The live service spawns a replacement, waits until the replacement attests the same managed set over IPC, then the incumbent exits (`src/serviceControl.ts:93-240`). Both `raft-computer restart` and the server's `computer:restart` use it. **This does not fit** launchd `KeepAlive` or systemd `Restart=always`: the manager would respawn the exiting incumbent, or kill the replacement, which is outside the unit's cgroup and pid.

**Recommendation:** port the model that exists, not a new one. Keep the detached process, `service.pid`, SIGTERM stop, and the IPC self-restart. Keep the macOS login carrier with the byte-identical label, path, and plist minus the K-specific parts (§2.3). Optionally add login autostart on Linux and Windows later, as a new feature. Converting to a true `systemd --user` / `KeepAlive` service would change restart semantics, the pidfile ownership model, and the server restart acks.

---

## 1. `@botiverse/k-carrier`, `@botiverse/hands-node`, and the `k*` / machine* modules

### 1.1 Direct package imports (non-test)

| File:line | Symbols | Purpose |
|---|---|---|
| `src/kPaths.ts:1-4` | `slotArtifactPath`, `Slot` | K slot layout. In the Rust ref (`_ref/k-carrier/src/storage.rs:183-186`): `<home>/computer/k/slots/{stable,experiment}/artifact.bin` plus `VERSION`. Other K files under `<home>/computer/k/`: `operation.json`, `journal.jsonl`, `receipts/`, `upgrade.lock`, `slots/*.staging`, `slots/stable.old`. |
| `src/kHostAdapter.ts:29-33` | `HostAdapter`, `ProcessEvidence`, `Slot` (types) | Computer implements K's quiesce/start/healthProbe/resume host boundary. |
| `src/kInstallerConvergence.ts:6-11` | `ProcessEvidence`, `ReleaseSource`, `Upgrader`, `bootstrapStable`, `QuarantineResult` | `__installer-converge`: installer hands verified bytes to K, which promotes them to the stable slot. |
| `src/kOperationAcknowledgement.ts:3-8` | `acknowledgeOperation`, `loadOperation`, `OperationOutcome`, `OperationRecord` | `raft-computer operation acknowledge <id>`. |
| `src/kReleaseSource.ts:23-35` | hands-node `getHandsDeviceId`; hands-node/updater `createHandsUpdater`, `HandsUpdateError`, `UpdateCandidate`, `UpdateChannel`; k-carrier `Release`, `ReleaseSource`, `ReleaseContext` | Resolves the upgrade target from Hands (`https://hands.build`, app `raft-computer-cli`, `src/releaseAuthority.ts:4-5`) plus the CDN manifest. |
| `src/kUpgradeCoordinator.ts:1-7` | `bootstrapStable`, `CreateUpgraderOptions`, `NotificationEvent`, `OperationRecord`, `Upgrader` | The detached `__k-upgrade` driver. |
| `src/kUpgradeProcess.ts:4-8` | `loadOperation`, `OperationRead`, `Upgrader` | Spawns and inspects the coordinator. |
| `src/kUpgrader.ts:1-9` | `createUpgrader`, `fileProvenanceJournal`, `HostAdapter`, `ReleaseSource`, … | Canonical K construction. |
| `src/kUpgradeReconcile.ts:1` | `Upgrader` (type) | Reports K's terminal receipt to the origin server on connect. |
| `src/kLifecycleSurface.ts:1` | `CreateUpgraderOptions` (type) | K's readback that the live service runs the experiment slot. |
| `src/legacyKOriginAdoption.ts:3-8` | `loadOperation`, `persistOperation`, … | Legacy adoption (DROP per scope). |
| `src/residentLifecycleBridge.ts:14-18` | `acknowledgeOperation`, `loadOperation`, `OperationRead` | Binds a server `upgrade` ready-ack to K's promoted receipt. |
| `src/status.ts:30-34` | `loadOperation`, `OperationOutcome`, `OperationPhase` | Status/doctor "Upgrade:" line. |

The tests `doctor.test.ts:7`, `status.test.ts:12`, `installScriptContract.test.ts:14`, `kCarrierDependency.test.ts:13`, and all `k*.test.ts` also import k-carrier.
The TS k-carrier 0.1.8 package is not installed in the snapshot (no `node_modules`). Its API above is inferred from call sites plus the Rust reference.

### 1.2 Module-by-module verdict

**DROP (pure update machinery):**

| Module | What it does | Evidence |
|---|---|---|
| `kHostAdapter.ts` | quiesce/park the managed set, start a slot, healthProbe via `machine-attestation`, resume | `:1-26, :167` |
| `kInstallerConvergence.ts` | `__installer-converge <ver> <sha>`: prints `converged` or `not-initialized` | `:22, :297`; used by `install.ps1:681-697` |
| `kOperationAcknowledgement.ts` | acknowledges K terminal receipts | `:315` |
| `kReleaseSource.ts`, `computerRelease.ts`, `releaseAuthority.ts`, `channel.ts`, `lib/channelState.ts` | Hands/CDN release resolution and the release channel (`<home>/computer/channel`) | `computerRelease.ts:4-9` (`https://cdn.raft.build/computer`, `RAFT_COMPUTER_UPGRADE_BASE_URL`) |
| `kUpgrade{Coordinator,Process,Reconcile}.ts`, `kUpgrader.ts`, `kConsent.ts`, `kLifecycleSurface.ts`, `kHostLifecycleRefresh.ts`, `kAcceptanceApp.ts` | the upgrade transaction, consent prompt (`Install Raft Computer X? [y/N]`, `kConsent.ts:33-35`), and acceptance fixture | |
| `serviceUpgradeStart.ts` | IPC `upgrade-start` handler; spawns `__k-upgrade` | `:53`; wired at `service.ts:1081`, `serviceIpcSeam.ts:70-78` |
| `kRunnerHold.ts` | `<home>/computer/k/host-runner-hold.json`; a held service does not spawn runners during an upgrade | `kRunnerHold.ts:6-20`, `service.ts:1026`. Drop it: a no-update build never holds. |
| `kResidentBinary.ts` | picks the K stable/experiment slot over the running binary | `:41-78`. Replace with `std::env::current_exe()`. |
| `index.ts:153-242` `dispatchToKResident` | the PATH "dispatcher" execs the K stable slot and sets `RAFT_COMPUTER_DISPATCHER_PATH` | Drop. The Rust binary *is* the program. |
| `index.ts:260+` `stripForwardedCarrierName` | workaround for ≤1.0.17 carriers forwarding argv | Drop (legacy). |
| `legacyKOriginAdoption.ts`, `legacySupervisorTakeover.ts`, `legacyOsSupervisorMigration.ts`, `osSupervisor*.ts`, `services/adoptLegacy.ts` | legacy migration | DROP per scope |

**KEEP (not update machinery, despite the names):**

| Module | Keep because | Notes |
|---|---|---|
| `versionEvidence.ts` | writes/reads `{version, installRoot, pid, writtenAt, parentPid?, shellEnvironment?}` JSON (`:6-63`) | Written to `<home>/computer/service-version.json` and `<home>/computer/servers/<id>/runner-version.json` (paths per `paths.ts:148-149, 306-307`). Status and readiness read it. |
| `runningVersionEvidence.ts` | the writer used by service and runner (`:58-64`) | `installRoot` = `dirname(dirname(module))` (`:21-32`). In Rust use `current_exe().parent()`, or keep a string. Nothing checks it except `installRoot.length>0` (`machineReadiness.ts:50`). |
| `machineFacts.ts` + `machineReadiness.ts` | pure "all target runners alive, attested at the expected version, connected" gate used by `start` (`services/start.ts:51-52`) | `expectedVersion` = COMPUTER_VERSION. Keep. |
| `machineServiceAttestation.ts` | IPC `machine-attestation` handler: `{computerVersion, serviceGeneration(uuid per start), servicePid, serviceExecutablePath, sourceServicePid?, managedServerIds, managedMachineIdentities, managedSetRevision}` (`:162-182`). Used by self-restart convergence (`serviceControl.ts:11-14`) and restart ready-acks | Keep. Only `serviceExecutablePath` is K-motivated; keep it anyway, it's cheap. |
| `residentLifecycleBridge.ts` | produces `lifecycleAcks` for the daemon's `ready` frame and consumes server receipts (`:139-164`) | **PARTIAL**: keep the restart path (`:118-128`, via the pending-restart marker plus `waitForRestartConvergence`). Remove `loadOperation(kStateDir)` (`:115`), `bindKUpgradeReadyAcknowledgement` (`:50-98`), and `acknowledgeKReadyReceipt` (`:30-47`). |
| `lifecycleOperations.ts`, `localLifecycleIntents.ts`, `restartMarker.ts` | per-server durable ledger `servers/<id>/lifecycle-operations.json` of `{operationId, action, pendingPhases}` for shutdown/ready acks (`paths.ts:202-203`) | Keep. Upgrade-specific helpers (`retireCompletedUpgradeShutdownsFromLog`, upgrade intents) can shrink. |
| `machineOperationRuntime.ts`, `machineOperationStore.ts`, `machineConvergenceReducer.ts` | `<home>/machine-operations/<opId>.json` CAS store (`machineOperationRuntime.ts:18-29`) | Only caller is `legacySupervisorTakeover.ts` → **DROP**. |
| `residentCoreIdentity.ts` (tested by `serviceProvenance.test.ts`) | builds the DaemonCore identity `{serverUrl, apiKey, machineOwnerProvenance:{kind:"managed_computer_runner", serverId, …}}` | Keep. Not K-related. There is no `serviceProvenance.ts`; the test file is named after the concept. |
| `lib/serviceIdentity.ts` | publishes pid, then version evidence, after the IPC bind | Keep |

**What must be replaced by simple code:**
1. **Binary location:** `current_exe()` everywhere (the service spawn, the runner `__run` spawn, the `__cli` wrapper path, and the launchd carrier `ProgramArguments[0]`). `resolveStableDispatcherPath` (`macosLoginCarrier.ts:136-160`) rejects paths under `<home>/computer/k/`. Keep that check, so an old K slot binary is never persisted into the plist.
2. **Service identity/attestation:** unchanged (the `machine-attestation` IPC).
3. **Readiness:** unchanged (`machineReadiness`).
4. **Lifecycle acks:** restart/start/stop acks unchanged. For `upgrade`, see §4.
5. `status` "Upgrade:" / doctor "K upgrade receipt": see §3.

---

## 2. Service install and control today

### 2.1 Process model (keep)
- `start` → `services/start.ts`. Under the mutation lock (`cli.ts:399-409`) it:
  1. converges the macOS login carrier to `enabled` (`services/start.ts:373-390`, darwin only);
  2. calls `spawnDetachedService` (`services/start.ts:498`).
  `spawnDetachedService` rotates `service.log`, opens it for append, and spawns `<exe> __service` with `detached:true`, stdio → `computer/run/service.log`, and `windowsHide`. It writes `service.pid` with the child pid (`service.ts:265-299`), and the service rewrites it with its own pid.
- `start` then waits for machine readiness (§1.2).
- User-visible start lines are in `startStop.ts`: `Service already running (pid N).` (`:105`), `Running service in the foreground (managing X of Y attached server(s)). Ctrl-C to stop.` (`:108`), `Service started (pid N); keeps running after this terminal closes.` (`:111`), `Managing X of Y attached server(s). Logs: <path>` (`:146`), `Per-server runner logs: …` (`:148`), and ``Check state with `raft-computer status`.`` (`:149`).
- `stop` → `services/stop.ts`: SIGTERM the pid from `service.pid` (the only kill site, `:131-134`), wait `timeoutMs`, then converge the carrier to `disabled` (`:116-120`). Output: `Service not running.`, `Service not running (cleared stale pidfile for pid N).`, `Stopped service (pid N).` (`startStop.ts:202-206`). Errors: `STOP_SIGNAL_FAILED` (`stop.ts:199`) and `STOP_TIMEOUT` ``…Force-kill with: kill -9 N`` (`:229`).
- On Windows, `process.kill(pid,'SIGTERM')` is `TerminateProcess`.
- `restart` → if a service is live, IPC `restart-service` (`serviceControl.ts:243-253`) → `performServiceSelfRestart` (`:93`). Otherwise the cold path is the same as start.
- Status liveness is `service.pid` plus `kill(pid,0)` (EPERM counts as alive) (`internal/process-primitives.ts:19-51`). `findLiveServicePid[ReadOnly]` reads the single candidate `run/service.pid`; the read-write variant clears it when stale (`internal/service-pid-fallback.ts:1-20, 57-83`).
- `cleanup.ts` (doctor `--fix`, `:407-426`) does the following:
  - clears stale pidfiles;
  - kills orphans: runs `ps -o pid,ppid,comm -A`, takes direct children of the service whose `comm` starts with `slock-` and are not in the pidfiles, and SIGTERMs them; skipped on Windows (`:104-205`);
  - quarantines power-loss partial state to `computer/.quarantine/<ts>-<serverId>/`;
  - removes tmp files older than 24h;
  - releases stale locks.
  The `slock-` comm filter may be dead in practice (the processes are `raft-computer`). Worth checking before porting.
- `reset.ts`: IPC-or-disk routing for `reset-service` (clears `service.state.json` crashHistory) and `reset-runner` (per-runner `health.json`) (`:48-126`). Not OS-level.

### 2.2 Retired OS-manager definitions (for reference / DROP)
All three share the hash `sha256("<platform>\0<normalized home>")[:16]` (lowercased on win32) and the owner token `raft-computer-os-supervisor-v1-<hash>` (`osSupervisor.ts:66-74, 311-312`). Argv: `<bin> __service --slock-home <home> --os-supervised <kind>` (`:158-160`).
- **launchd:** label `build.raft.computer.<hash>`, file `~/Library/LaunchAgents/<label>.plist`. The plist has `ProgramArguments`, `EnvironmentVariables{RAFT_COMPUTER_SUPERVISOR_OWNER, PATH}`, `RunAtLoad`, `KeepAlive`, `ProcessType=Background`, and stdout/stderr → `<home>/computer/run/service.log` (`:188-231, 350-376`). Control: `launchctl disable|bootout|print gui/<uid>/<label>` (`:506-525`).
- **systemd:** unit `raft-computer-<hash>.service` in `${XDG_CONFIG_HOME:-~/.config}/systemd/user/`. Contents (`:162-186`):
  ```
  [Unit]
  Description=Raft Computer per-user service (<ownerToken>)
  After=network-online.target

  [Service]
  Type=simple
  Environment="PATH=<buildSystemdDiscoveryPath>"
  ExecStart="<bin>" "__service" "--slock-home" "<home>" "--os-supervised" "systemd-user"
  Restart=always
  RestartSec=2
  KillMode=control-group

  [Install]
  WantedBy=default.target
  ```
  Control: `systemctl --user disable --now` and `systemctl --user show … --property=LoadState,ActiveState,SubState,UnitFileState,MainPID` (`:487-503`). The PATH comes from `systemdDiscoveryPath.ts:50`: allowlisted runtime-manager bins plus standard dirs.
- **Windows:** a scheduled task `\Raft-Computer-<hash>` (not an SCM service). Its XML has a LogonTrigger for the user SID, `InteractiveToken`, `LeastPrivilege`, `IgnoreNew`, `ExecutionTimeLimit PT0S`, and `RestartOnFailure PT2S×999` (`:257-304`). Control: `schtasks /Change /DISABLE`, `/End`, `/Query /XML /HRESULT`, plus a PowerShell `Schedule.Service` state query (`:527-561`).
- `osSupervisorLifecycle.ts:16-50` parses that exact 5-token argv so that old definitions still boot `__service`. The `__service` command still accepts `--os-supervised` (`cli.ts:787`), and `OS_SUPERVISOR_KIND_ENV_VAR` = `RAFT_COMPUTER_OS_SUPERVISOR_KIND` feeds `runnerChildEnv.ts`.
  - **Port suggestion:** accept and ignore `--os-supervised <kind>`, so any stale definition on a user machine still boots a service instead of erroring.

### 2.3 macOS login carrier (KEEP, simplified)
- Label: `build.raft.computer.login.<sha256(path.resolve(home))[:16]>`. Note the hash input has **no platform prefix**, unlike osSupervisor (`macosLoginCarrier.ts:171-176, 204`).
- Plist path: `~/Library/LaunchAgents/<label>.plist`, domain `gui/<uid>`.
- Exact plist (`:216-243`):
  ```xml
  <?xml version="1.0" encoding="UTF-8"?>
  <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
  <plist version="1.0">
    <dict>
      <key>Label</key>
      <string>build.raft.computer.login.<hash></string>
      <key>ProgramArguments</key>
      <array>
        <string><dispatcherPath></string>
        <string>__service</string>
        <string>--slock-home</string>
        <string><home></string>
      </array>
      <key>EnvironmentVariables</key>
      <dict>
        <key>PATH</key>
        <string>~/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin</string>
      </dict>
      <key>RunAtLoad</key>
      <true/>
      <key>ProcessType</key>
      <string>Background</string>
      <key>StandardOutPath</key>
      <string><home>/computer/run/service.log</string>
      <key>StandardErrorPath</key>
      <string><home>/computer/run/service.log</string>
    </dict>
  </plist>
  ```
  The file ends with a trailing newline, and values are XML-escaped. The `~/.local/bin` entry is the absolute resolved path.
- Owner marker: `<home>/computer/host-lifecycle-owner.json`, containing `{formatVersion:1, owner:"cli"|"app", enabled, dispatcherPath, label, definitionPath}`. The pending-replace/recovery record is `<home>/computer/host-lifecycle-pending-replace.json` (`:255-269, 34-41, 85-93`).
- Enable sequence (`:679-878`):
  1. `launchctl print gui/<uid>` (GUI-domain guard);
  2. `launchctl print gui/<uid>/<label>`;
  3. if the definition, the live job, and the dispatcher already match, just write the marker;
  4. otherwise: write the pending record, remove the marker, `bootout`, write the plist, read it back, `launchctl bootstrap gui/<uid> <plist>`, then `print` and check that the output contains the label and the dispatcher;
  5. write the marker; on failure, roll back.
- Disable/remove: `bootout`, delete the plist, verify it is gone (`:590-625`).
- `owner:"app"` means Raft Desktop (Electron) owns Launch-at-login. The CLI then removes its own carrier and only flips `enabled` (`:906-920`). The lib exports `convergeAppHostLifecycle` and `removeHostLifecycle` for Desktop (`lib/index.ts:88-102`).
- `assertNoMarkerlessLegacyDesktop` (`:438`) refuses to act when `/Applications/Raft Computer.app` (`build.raft.computer-app`) exists without a marker. Legacy → likely DROP, but it prevents double-start with the old Desktop. Decide consciously.
- K-specific parts to drop: the `deadlineAtMs` / `REPLACE_MINIMUM_BUDGET_MS` / `ROLLBACK_RESERVED_BUDGET_MS` "shared K resume deadline" budget logic (`:747-760, 811-818`), `refreshCliLoginCarrierIfOwned` (`:952-966`, called only from the K resume path via `kHostLifecycleRefresh.ts`), and the `RAFT_COMPUTER_DISPATCHER_PATH` env var (`:29-30, 141-150`).
- **Rust single-install equivalent:** `dispatcherPath = current_exe()` canonicalized. Keep the label, plist bytes, marker files, and the enable/disable readback. Pending-replace rollback can be kept as-is (cheap) or reduced to "bootout + rewrite + bootstrap".

### 2.4 Proposed Rust design (simplest, drop-in)
- `raft-computer start`: take the mutation lock; on darwin, converge the carrier (identical label/plist/marker); spawn `current_exe() __service` detached (Unix: `setsid` + stdio to `service.log`; Windows: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`); write `service.pid`; wait for readiness.
- `stop`: SIGTERM (Windows: `TerminateProcess`, or better a graceful IPC `shutdown` if the IPC agent adds one); disable the carrier.
- `restart`: IPC self-handoff.
- `status`: pidfile + `kill 0`.
- Linux/Windows reboot persistence: none today. If wanted later:
  - Linux: a `systemd --user` unit named `raft-computer-login-<hash>.service` with `Type=oneshot` + `RemainAfterExit=no`, running `raft-computer start`. It must not be `Restart=always`, because that would fight the self-restart.
  - Windows: an HKCU `Run` value or a logon-trigger task that runs `raft-computer start`.
  Do not reuse the retired names (`raft-computer-<hash>.service`, `\Raft-Computer-<hash>`, `build.raft.computer.<hash>`), so leftover retirement logic in old installers never mistakes them for legacy.
- Existing K installs: the plist already points at `~/.local/bin/raft-computer` (the dispatcher). Replacing that file with the Rust binary is enough. `<home>/computer/k/` can be left alone or archived by the installer (it already archives to `computer/k-quarantine/installer-<ver>.XXXXXX/k`, `install.sh:772-810`). A service currently running *from* `k/slots/stable/artifact.bin` must be stopped first; the installer's stop plus start handles that.

---

## 3. status / doctor / health / output

### 3.1 `raft-computer status` (text only; **there is no `--json`**, `cli.ts:450-459`)
The report comes from `buildStatusReport(home)` (`status.ts:271-325`) and is formatted at `:341-436`. The lib/IPC `service-status` returns the same object. Lines, in order:

| Line | Source | K-dependent? |
|---|---|---|
| blank, then `SLOCK_HOME:   <home>` | `resolveRaftHome` | no |
| `Logged in:    yes (user <id>)` / ``no — run `raft-computer login` `` / ``no — user session file is invalid; re-run `raft-computer login` `` | `user-session.json` (`kind=="user-session"` + `accessToken`) | no |
| `Login server: <url>` (optional) | session `serverUrl`, canonicalized | no |
| `CLI version:  <COMPUTER_VERSION>` | baked version | no |
| `Service:   running (pid N)` / ``stopped — run `raft-computer start` `` (3 spaces after the colon) | `run/service.pid` + liveness | no |
| `Service version: <v>` or `unknown (no live version evidence)` | `computer/service-version.json` when `evidence.pid==service pid` | no. **The installer parses `$3` of this line** (`install.sh:840-842`). |
| `  Warning: terminal shell environment import failed (<code>)…` | evidence `shellEnvironment` = `unavailable:<code>` | no |
| `  Warning: running service version X differs from this app/CLI version Y. …` | evidence vs CLI | no |
| `Service log: <home>/computer/run/service.log` | | no |
| `Upgrade: K operation in flight (id …)` + Target/Phase/Outcome/Acknowledge/Detail/Updated, **or** `Upgrade: none in flight` | `loadOperation(<home>/computer/k)` (`:155-180, 374-392`) | **yes**. Without K: always `Upgrade: none in flight`. Keep the constant line for drop-in output, or remove it. |
| `Host lifecycle: <status> (<errorCode>)` + ``  Run `raft-computer doctor` before retrying start or upgrade.``, else `Host lifecycle: healthy` | `host-lifecycle-pending-replace.json` (`readHostLifecycleRecoveryStatus`, `macosLoginCarrier.ts:317`) | no (macOS carrier) |
| blank, then `Attachments:  none — run …` or the table header `  SERVER(24)HEALTH(12)CONNECTED(12)DAEMON(24)MACHINE(38)URL` | `servers/*/runner.state.json` | no |
| row, `    Runner version: …`, `    Server runner log: …` | `servers/<id>/runner.pid` fallback chain, `runner.connected` marker, `runner-version.json`, `health.json` | no |
| degraded/unlinked notes | `health.ts` (`isDegraded`: ≥3 crashes in 60s, `:34-35, 188`; `readTerminalUnlinked` `:284`) | no |

Report object fields (for IPC/lib consumers): `slockHome, loggedIn, userId, userName, userDisplayName, userEmail, loginServerUrl, userSessionError, cliVersion, service{running,pid?,logPath,version{version,evidencePath,evidencePid,evidenceWrittenAt,shellEnvironment}}, upgrade|null, hostLifecycle|null, servers[{serverId,serverSlug,serverMachineId,machineId,serverUrl,attachedAt,serverRunnerLogPath,runnerVersion,daemon,health,serverConnected}]` (`status.ts:80-104, 216-262`). Without K, `upgrade` is always `null`.

### 3.2 `raft-computer doctor [serverSlug] [--fix] [--migration-details]`
The checks come from `runDoctorChecks` (`doctor.ts:129-265`); the renderer is `doctorCli.ts:23-98`. Output shape:
- blank line, `Using state at <home>`;
- per section: a heading line plus dashes;
- each check: `✓|✗ <name padded to 48> <detail>`, passed through `redactSecrets` (masks `sk_*`, JWT-ish strings, and hex ≥40; `doctor.ts:34-44`);
- then `All checks passed.` or `Some checks failed — see the actionable hints above.`;
- optionally recent crashes (60s window);
- optionally `Cleanup pass:` with the `--fix` results.

Checks:
- `SLOCK_HOME`
- `user session`
- `service`: always ok; ``stopped (run `raft-computer start` when you want background)``
- `macOS login carrier`: only when a recovery record exists
- **`K upgrade receipt`**: only when a K op is active or unacknowledged; **drop it**, since it never appears without K (`:168-178`)
- `attachments`
- per server: `server <label>` section, `attach`, `preflight` (live HTTP to the server with that server's `sk_computer_*`), `runner`, `identity` (legacy migration/regret → mostly the other agent's area and legacy)
- `--migration-details` → `runDoctorMigrationDetails` (legacy migration evidence; DROP or other agent).

Exit code: `process.exitCode = allOk ? 0 : 1` (`doctorCli.ts:100`). The "service" check is always ok, so a stopped service alone never fails doctor. With K gone, the "K upgrade receipt" check can no longer make doctor exit 1.

### 3.3 Error format (`output.ts`)
All failures go to **stderr** (`fail`, `:27-31`) in this format:
```
[Using state at <home>]            # only SETUP_/MIGRATE_/MIGRATION_/LEGACY_/NON_INTERACTIVE_SETUP_REQUIRES_FLAGS
What happened (<CODE>): <message, whitespace-collapsed>
Next: <cmd && cmd | fallback>
State: <guarantee>
Help: https://app.raft.build/s/community/
[(If you are sure you want a brand-new computer: raft-computer setup /x --fresh)]
```
- "Next" commands are extracted from backticked `raft-computer …` spans (longest fence first), then from a bare `raft-computer …` phrase, then from `fallbackNextCommand(code)` (`:56-109`).
- "State" comes from `stateGuarantee(code)` (`:111-126`).
- The exit code comes from `CliExit.exitCode` (default 1).
- Uncaught errors print `raft-computer: <message>` with exit 1 (`cli.ts:916-925`).
- `UPGRADE_*` and `CHANNEL_*` entries in the fallback/guarantee tables become dead code after the drop.

---

## 4. What the server expects (lifecycle / version)

- Protocol (`packages/shared/src/index.ts`):
  - `COMPUTER_LIFECYCLE_ACTIONS = start|stop|restart|upgrade` (`:422`)
  - `ComputerLifecycleExecutionAck {operationId?, requestId?, action, phase:"shutdown"|"ready", loadedComputerVersion?, serviceGeneration?, managedSetRevision?, oldProcessIdentitiesDead?, deadProcessIdentities?}` (`:432-445`)
  - server→machine: `computer:restart`, `computer:upgrade {operationId?, requestId?}`, `computer:lifecycle:receipt {operationId, phase}` (`:562-567`)
  - machine→server: `computer:upgrade:progress`, `computer:upgrade:done {requestId, ok, newVersion?, rolledBack?, error?}`, `computer:restart:done`, and `ready {capabilities, computerVersion, lifecycleAcks}`, plus `machine:shutdown {lifecycleAcks}` (`:877, 891-903`).
- Capability `COMPUTER_CAPABILITY_SUPERVISOR_MUTATIONS = "computer:supervisor-mutations-v1"` (`:418`).
  - The daemon adds it to `ready.capabilities` iff `computerControlViaSupervisor` is set (`packages/daemon/src/core.ts:4219-4230`).
  - The service sets that from `residentLifecycleBridge.supervisorMutationsAttested`, which is true when a live service attests `computerVersion == COMPUTER_VERSION` (`residentLifecycleBridge.ts:145-151`, `service.ts:462`).
  - Server: when the capability is present, the dispatch adapter is `supervisor-v1`, otherwise `runner-first-hop-v1` (`packages/server/src/routes/servers.ts:3070-3076`). This covers restart as well as upgrade, so **keep advertising it**, since restart still goes through the supervisor.
- Daemon handling (`core.ts:4102-4173`): dedupes by operationId, then calls `onComputerControl(action, ctx)`.
  - Computer impl (`service.ts:473-497`): enqueue a lifecycle op in `lifecycle-operations.json` (pending phases shutdown and ready), then:
    - `restart` → IPC `restart-service`;
    - `upgrade` → IPC `upgrade-start` (`serviceControl.ts:264-303`).
  - If the handler throws, the daemon sends `computer:upgrade:done {ok:false, error:"computer_control_failed"|"control_busy"|"self_relaunch_unavailable"}`.
- Server (`packages/server/src/services/agentOrchestrator.ts:6295-6308`): `computer:upgrade:done` with `ok:false` terminalizes the operation as `failed` (reason `upgrade_reported_failure`) and sends failure receipts. Web shows it. There is no special "unsupported" code.
- **What a no-update build should answer:** on `computer:upgrade` with a requestId, reply immediately with `computer:upgrade:done {requestId, ok:false, error:"upgrade_unsupported"}` (any string works; the server only branches on `ok`/`rolledBack`).
  - Do **not** enqueue a lifecycle op for `upgrade`. Otherwise the pending `ready` ack will never bind and will sit in `lifecycleAcks` forever (it is filtered out today only because K never promotes, `residentLifecycleBridge.ts:58-67`).
  - Alternatively, still enqueue it but mark it terminal. Simpler: don't enqueue, just send `done ok:false`.
  - Emit no `computer:upgrade:progress`.
  - The server's CDN-driven "update available" UI will keep offering upgrades. That is a server/web concern; optionally report a `computerVersion` that the server's policy considers current, but don't fake it.
- `onComputerUpgradeReconcile` (`service.ts:499-507`) and `reconcileComputerLifecycleOrigin` (`:461`): drop them (no-ops).
- `restart`, and the start/stop acks: keep the full path (pending-restart marker, `waitForRestartConvergence`, ready ack with `serviceGeneration` / `managedSetRevision` / `deadProcessIdentities`).
- Local CLI `raft-computer upgrade`, `channel …`, and `operation acknowledge`: drop them. For drop-in friendliness, keep a stub `upgrade` that prints a clear message (e.g. "This build does not self-update; reinstall via install.sh") with exit 1. Commander would otherwise print `error: unknown command 'upgrade'`.

---

## 5. Installers

### 5.1 `scripts/install.sh` (macOS + Linux)
- Usage: `curl -fsSL https://cdn.raft.build/computer/install.sh | sh [-s -- --channel alpha|--version X]` (`:5-7`).
- Env vars (`:13-34`):
  - `RAFT_COMPUTER_VERSION`
  - `RAFT_COMPUTER_INSTALL_DIR` (default `$HOME/.local/bin`)
  - `RAFT_COMPUTER_RELEASE_BASE` (default `https://cdn.raft.build/computer`)
  - `RAFT_COMPUTER_HANDS_ORIGIN` / `_HANDS_APP` (default `https://hands.build`, `raft-computer-cli`)
  - `RAFT_COMPUTER_RELEASE_BACKEND=legacy-cdn`
  - `RAFT_COMPUTER_INSTALL_CHANNEL`
  - `RAFT_COMPUTER_FORCE`
  - `RAFT_COMPUTER_NO_MODIFY_PATH`
  - state home: `RAFT_HOME` → `SLOCK_HOME` → `~/.slock`
- Target: `uname` gives `darwin|linux` + `arm64|x64`. Apple Silicon is forced to arm64 even under Rosetta (`:128-157`).
- Version resolution:
  - explicit version;
  - else a persisted `<home>/computer/channel` (`latest|alpha|pinned:<semver>`);
  - else Hands `GET {HANDS}/public/v2/apps/raft-computer-cli/latest?channel=main|alpha&product_type=cli-binary`, taking `build.version` (`:389-424`).
  - Pinned versions are attested via `…/updates/check?product_type=cli-binary&current_version=0.0.0&channel=main&platform=&arch=&sdk_version=0.5.1&version=` (`:488-493`).
- Manifest at `{BASE}/{version}/manifest.json` (format per `scripts/native/produce-manifest.mjs:10-20`):
  ```json
  {"version":"1.0.28","tag":"computer-v1.0.28",
   "targets":{"darwin-arm64":{"file":"raft-computer-darwin-arm64","sha256":"…","size":N,
                              "gz":{"file":"raft-computer-darwin-arm64.gz","sha256":"…","size":N}, ...notarization evidence (darwin)},
              "linux-x64":{...}, "win32-x64":{"file":"raft-computer-win32-x64.exe",...}},
   "photonWasm":{"file":"photon_rs_bg.wasm","sha256":"…","size":N}}
  ```
  Hands' asset sha256/size must equal the manifest's (`:662-685`).
- Downloads: the `.gz` if present, else the raw binary, verifying sha256 both before and after gunzip. Then `photon_rs_bg.wasm`, checking `file -b` Mach-O/ELF arch (`:742-765`), and the candidate's `--version` first token (`:305-310`, run with a cold probe `SLOCK_HOME` so a K dispatcher doesn't forward).
- Guards: refuses a downgrade unless `FORCE` (`:447-463`). `reset_k_state` (`:772-810`) stops the service, kills any `__k-upgrade` driver, and archives `<home>/computer/k` to `computer/k-quarantine/…`.
- Install: stage `.<name>.install.$$` in INSTALL_DIR, verify it, then `mv -f` both `raft-computer` and `photon_rs_bg.wasm` into `$INSTALL_DIR` (`:813-833`).
- After install:
  - persist the channel;
  - add to PATH: append `export PATH="$HOME/.local/bin:$PATH"` to `${ZDOTDIR:-$HOME}/.zshrc` or `~/.bashrc`, default dir only, idempotent (`:332-377`);
  - run `raft-computer __supervisor retire-legacy` (`:380-384`, DROP);
  - if a service was running: `start`, then `status` must show `Service version: <ver>` (`:837-846`);
  - warn about shadowing npm-global installs (`:848-866`).

### 5.2 `scripts/install.ps1` (Windows)
- Installs `raft-computer.exe` + `photon_rs_bg.wasm` to `%USERPROFILE%\.local\bin` (`:50-57`) from the same manifest. The asset name must match `^raft-computer-win32-(x64|arm64)\.exe$` (`:642-645`).
- Verifies sha256 and the PE machine type (`Assert-PeTarget`), and checks the version.
- If `<home>\computer\k` exists, it runs `candidate __installer-converge <ver> <sha> [--force-downgrade]` and expects the last line to be `converged` or `not-initialized` (`:681-697`, DROP).
- Adds the install dir to the **User** `Path` via `[Environment]::SetEnvironmentVariable('Path',…,'User')` and to the current process (`:419-444`).
- Runs `__supervisor retire-legacy` only when legacy is detected (`:100-102, 703-705`, DROP).
- Also locates Git Bash (`Find-GitBash`, `:159-180`; `RAFT_COMPUTER_GIT_BASH_PATH`). Git Bash is a prerequisite for agent runtimes on Windows.

### 5.3 `scripts/native/build.mjs` (SEA)
- Downloads the official Node binary, runs esbuild on `src/index.ts` into a single CJS bundle with **everything inlined**, including `@botiverse/raft-daemon`, the bundled `slock` CLI (`__cli`), and the pi runtime. Versions are baked through `define` (`__RAFT_COMPUTER_VERSION__`, `__RAFT_DAEMON_VERSION__`, `__RAFT_CLI_VERSION__`).
- Then `--experimental-sea-config` → postject `NODE_SEA_BLOB` (Mach-O segment `NODE_SEA`), with darwin ad-hoc codesign (`:1-30, 183-218, 273-328`).
- The formal workflow then signs with Developer ID, notarizes (`sign-notarize-macos.sh`), and regenerates the hashes.
- Emits the photon WASM sidecar (`:237-269`).
- Hidden verification mode: `__build-versions` prints `{computerVersion, daemonVersion, cliVersion}` (`cli.ts:876-884`).
- `ensureSeaRuntimePackageDir` writes `<home>/runtime-pkg/package.json` for pi (`service.ts:362-372`).

### 5.4 What a Rust release needs
- Per-target binaries `raft-computer-{darwin,linux}-{arm64,x64}` and `raft-computer-win32-{x64,arm64}.exe`, plus `.gz` and `.sha256`, and a `manifest.json` with the **same schema**. That lets old `install.sh` / `install.ps1` copies (and web copy-paste one-liners) keep working, as long as `photonWasm` is present (required by both scripts: `install.sh:548`, `install.ps1:638-641`). Either ship a dummy or real `photon_rs_bg.wasm`, or drop the check in new scripts.
- `raft-computer --version` must print the bare semver as the first token (Commander prints `1.0.28`).
- A simplified `install.sh` / `install.ps1`: drop Hands (or keep it if releases are still registered there), drop K reset/converge, drop retire-legacy. Keep the paths, the PATH edits, the downgrade guard, the sha/arch checks, the channel file (only if channels survive), and the stop/start + `Service version:` readback.
- macOS: Developer ID signing and notarization, otherwise Gatekeeper blocks it. Launchd `ProgramArguments` pointing at an unsigned or quarantined binary fails silently at login.

---

## 6. Test classification (84 files: 76 `src/**/*.test.ts` + 8 `scripts/native/*.test.mjs`)

Legend: **S** = spawns processes or runs real scripts/binaries; **C** = asserts on script/workflow/source file contents.

| Test file | Verdict | Reason |
|---|---|---|
| scripts/native/classify-object-inventory.test.mjs | DROP | R2/S3 release-publishing tooling (S) |
| scripts/native/dependency-freshness.test.mjs | DROP | SEA build freshness |
| scripts/native/photon-wasm-contract.test.mjs | DROP | SEA build/manifest photon sidecar (S, C) |
| scripts/native/publish-hands-release.test.mjs | DROP | Hands release publishing |
| scripts/native/register-existing-hands-release.test.mjs | DROP | Hands release publishing |
| scripts/native/rehearse-object-inventory.test.mjs | DROP | release object inventory (S) |
| scripts/native/staging-version.test.mjs | DROP | staging prerelease versioning (S) |
| scripts/native/verify-notarization-ticket.test.mjs | DROP | notarization tooling (re-create in Rust CI if notarizing) |
| src/apiClient.authclient.test.ts | PARTIAL | auth client KEEP; `ComputerLifecycleClient` upgrade-policy parts DROP (other agent) |
| src/attach.test.ts | KEEP | attach naming/errors (other agent) |
| src/browserHandoff.test.ts | KEEP | Enter-to-open URL |
| src/bundleConfig.test.ts | DROP | tsup bundling config (C) |
| src/channel.test.ts | DROP | release channel = update machinery |
| src/cleanup.test.ts | KEEP | doctor --fix cleanup semantics |
| src/cliCommandReferenceContract.test.ts | PARTIAL | "command refs point at registered commands" KEEP (re-implement over clap); OS-supervisor clause moot (C) |
| src/cliHelpCopy.test.ts | KEEP | help copy/grouping (drop-in text) |
| src/cliLifecycleContract.test.ts | PARTIAL | verb-surface invariants KEEP; detach/legacy parts DROP (C) |
| src/cliServerArgContract.test.ts | PARTIAL | server-arg contract KEEP; `channel versions` case DROP |
| src/cliUpgradeContract.test.ts | DROP | `upgrade` via K |
| src/computerExistingKMacosAcceptance.contract.test.ts | DROP | existing-K macOS acceptance script (S, C) |
| src/computerInstallerLiveCheck.contract.test.ts | DROP | private CI workflow contents (C; skipped in snapshot) |
| src/computerName.test.ts | KEEP | default name derivation |
| src/concurrency.test.ts | KEEP | mutation lock (S: child for lock-compromise) |
| src/doctor.test.ts | PARTIAL | doctor checks KEEP; K receipt / migration cases DROP |
| src/durableFile.test.ts | KEEP | durable writes |
| src/health.test.ts | KEEP | crash window / degraded / unlinked |
| src/importCycles.test.ts | DROP | TS import-cycle ratchet |
| src/installScriptContract.test.ts | PARTIAL→rewrite | 25 tests run the real install.sh/ps1 against fixtures (S, C). Keep downgrade guard, sha/arch checks, Hands/CDN identity (if kept), PATH/channel persistence, Rosetta arm64; drop K reset / retire-legacy / converge cases |
| src/internal/h-family-chaos.test.ts | DROP | chaos matrix tied to legacy/K identity (verify) |
| src/internal/ipc-server.test.ts | KEEP | IPC server (other agent) |
| src/internal/runner-log-diagnostics.test.ts | KEEP | bounded log tails (S) |
| src/kCarrierDependency.test.ts | DROP | k-carrier dependency pin |
| src/kConsent.test.ts | DROP | upgrade consent |
| src/kHostAdapter.test.ts | DROP | K host adapter (S) |
| src/kInstallerConvergence.test.ts | DROP | `__installer-converge` |
| src/kLifecycleSurface.test.ts | DROP | K readback surface |
| src/kOperationAcknowledgement.test.ts | DROP | `operation acknowledge` |
| src/kReleaseGzip.test.ts | DROP | K release gzip |
| src/kReleaseSource.test.ts | DROP | Hands/CDN release source |
| src/kResidentBinary.test.ts | DROP | slot selection |
| src/kUpgradeCoordinator.test.ts | DROP | upgrade driver |
| src/kUpgradeProcess.test.ts | DROP | upgrade process (S) |
| src/kUpgrader.test.ts | DROP | K construction |
| src/legacyKOriginAdoption.test.ts | DROP | legacy adoption |
| src/lib/actions.test.ts | KEEP | action descriptors (Desktop/lib) |
| src/lib/affordances.test.ts | KEEP | affordance model (Desktop/lib) |
| src/lib/api.test.ts | KEEP | ComputerApi |
| src/lib/api.trace.test.ts | PARTIAL | reset-service trace KEEP; `tryUpgradeViaService` DROP |
| src/lib/api.upgrade.test.ts | DROP | lib upgrade routing |
| src/lib/computerTracer.test.ts | KEEP | env gate |
| src/lib/ipc-client.test.ts | KEEP | IPC client (S; other agent) |
| src/lib/migration.test.ts | DROP | legacy migration detection (other agent may disagree for setup) |
| src/lib/runnerStateMachine.test.ts | KEEP | runner exit → state (S) |
| src/lib/serviceIdentity.test.ts | KEEP | pid/version publish after bind |
| src/lifecycleOperations.test.ts | PARTIAL | restart/start/stop ledger KEEP; upgrade intent/preparation DROP |
| src/login.test.ts | KEEP | login output (other agent) |
| src/logRotation.test.ts | KEEP | daily log rotation |
| src/logs.test.ts | KEEP | `logs` command |
| src/machineReadiness.test.ts | KEEP | readiness matrix |
| src/macosLoginCarrier.test.ts | PARTIAL | plist shape / RunAtLoad-no-KeepAlive / marker / readback KEEP; K deadline budget + legacy-Desktop cases DROP |
| src/nativePublish.contract.test.ts | DROP | SEA manifest/publish (S, C) |
| src/osSupervisor.test.ts | DROP | retired OS managers |
| src/osSupervisorWindowsAbsence.test.ts | DROP | retired Windows task retirement (S) |
| src/paths.test.ts | KEEP | home resolution |
| src/reset.test.ts | KEEP | reset-service / reset-runner (S) |
| src/residentLifecycleBridge.test.ts | PARTIAL | restart ready-ack KEEP; K ready-ack binding DROP |
| src/restartMarker.test.ts | KEEP | pending restart marker |
| src/runners.test.ts | KEEP | `runners` command |
| src/serverState.test.ts | PARTIAL | canonical attachment KEEP; legacy `attachment.json` migration DROP (other agent) |
| src/serverUrl.test.ts | KEEP | server URL resolution |
| src/service.test.ts | PARTIAL | 104 tests (S). Pidfile/liveness/spawn/runner supervision/self-restart KEEP; K slot, upgrade-start, runner hold, legacy takeover cases DROP |
| src/serviceProvenance.test.ts | KEEP | residentCoreIdentity (C: reads service.ts source; re-express) |
| src/serviceUpgradeStart.scope.test.ts | DROP | upgrade-start (S) |
| src/services/adoptLegacy.test.ts | DROP | legacy adoption (S) |
| src/services/attach.test.ts | KEEP | attach service (other agent) |
| src/services/diagnosticsPush.test.ts | KEEP | diagnostics upload |
| src/services/login.test.ts | KEEP | login service (other agent) |
| src/services/start.test.ts | KEEP (mostly) | start service (S); drop any K/legacy cases |
| src/services/stop.test.ts | KEEP | stop semantics/errors |
| src/setup.test.ts | PARTIAL | 69 tests (S); legacy-migration cases DROP (other agent) |
| src/shellEnvCapture.test.ts | KEEP | terminal env capture for the service (S; big, 51 tests) |
| src/status.test.ts | PARTIAL | status report KEEP; K upgrade-receipt cases DROP |
| src/targetServer.test.ts | KEEP | slug → serverId |
| src/version.test.ts | PARTIAL | baked-version logic → Rust `env!("CARGO_PKG_VERSION")`; trivial |

Contract tests asserting on script or source contents: `installScriptContract`, `computerExistingKMacosAcceptance.contract`, `computerInstallerLiveCheck.contract`, `nativePublish.contract`, `photon-wasm-contract`, `bundleConfig`, `cliCommandReferenceContract`, `cliLifecycleContract`, `serviceProvenance`, `importCycles`.
Tests that spawn real processes/scripts: `installScriptContract` (runs sh / pwsh), `service`, `services/start`, `setup`, `shellEnvCapture`, `concurrency`, `reset`, `ipc-client`, `runnerStateMachine`, `kHostAdapter`, `kUpgradeProcess`, `osSupervisorWindowsAbsence`, `adoptLegacy`, and `scripts/native/*`.
