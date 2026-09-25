# `raft` CLI — TypeScript → Rust port fact report

Source: `upstream/raft-source/packages/cli` @ 05f7d8f, version `0.0.24`. All paths below are relative to
`upstream/raft-source/packages/cli/` unless prefixed with `shared/` (= `upstream/raft-source/packages/shared/src/`)
or `daemon/` (= `upstream/raft-source/packages/daemon/src/`). Line numbers are for the pinned commit.

Observed behaviour ("probe") was verified by running the real TS entry (`node --import tsx src/index.ts …`)
in a scratch copy with commander@12.1.0 / undici@7 / ajv@8.18.0 / safe-regex2@5.1.1 / zod@4 / zod-openapi@6
installed, with `SLOCK_CLI_TRANSPORT_DIR` unset, stdout/stderr **not** a TTY. Probe recipe is in §11.
A full recursive `--help` dump (116 command paths) is appended as Appendix A.

Size: 124 non-test `.ts` files, 25 320 lines; 103 test files, 918 test cases (`test-execution-manifest.json`).

---

## 1. Entry flow

### 1.1 Packaging / bin
- `package.json`: `bin: { raft: dist/raft.js, slock: dist/slock.js }`; runtime deps `ajv ^8.18.0`,
  `commander ^12.1.0`, `safe-regex2 5.1.1` (exact), `undici ^7.24.7`. `@botiverse/raft-shared` is a
  **devDependency** and is bundled (tsup) — so shared code (zod contracts, formatters) is part of the CLI.
  `engines.node >=20`.
- `tsup.config.ts`: single ESM bundle, `shims: true`, `noExternal: ["commander","undici"]`, banner injects
  `createRequire` (commander is CJS). Comment: "Computer app copies this single file … sidecar has no package
  root". Pinned by `src/bundleConfig.test.ts`.
- `scripts/write-dist-package.mjs`: writes `dist/package.json` `{name, version, type, engines}` and the two bin
  wrappers. Each wrapper inlines a Node ≥20 preflight, sets `process.env.SLOCK_CLI_INVOCATION_NAME` to
  `raft`/`slock`, then `import("./index.js")`. **Nothing in `src/` reads `SLOCK_CLI_INVOCATION_NAME`**; the
  program name is hard-coded `raft` (main.ts:195). Rust: ignore (slock alias out of scope).
- Daemon side (coupling, not CLI code): `daemon/drivers/cliTransport.ts` generates per-launch wrappers
  `<SLOCK_HOME>/cli-transport/<agent>/<launch>/raft` that `exec '<node-or-electron>' '<cliScript>' "$@"`
  (≈ lines 400–440, `ELECTRON_RUN_AS_NODE=1` when host is Electron; SEA daemons use the literal `__cli`
  sentinel and re-exec themselves, `prepareCliTransport` ≈ 553+). They also prepend loopback entries to
  `NO_PROXY` (≈ 503–517) and export `SLOCK_CURRENT_*`/`RAFT_CURRENT_COMPUTER_*` (536–545) and
  `DEFAULT_ACTIVE_CAPABILITIES = "send,read,mentions,tasks,reactions,server,channels,knowledge"` (line 15).
  A Rust `raft` binary means the daemon wrapper must `exec <raft-binary> "$@"` instead of `node <script>`.

### 1.2 `src/index.ts` (8 lines)
`enforceSupportedNodeRuntime()` then `await import("./main.js")`; if that import throws, writes
`Unexpected error: <msg>\n` to stderr and sets `process.exitCode = 1`.

### 1.3 `src/runtimePreflight.ts` (39 lines)
If Node major < 20, writes exactly three stderr lines and exits 1:
```
Error: Node X is unsupported; raft requires Node >=20 before loading CLI runtime dependencies.
No network requests, credentials, or local state were touched.
Next action: Install/activate Node 24.15.0 (the repository pin), then retry.
```
Rust: not applicable (drop; `runtimePreflight.test.ts` 6 cases become N/A).

### 1.4 `src/version.ts` (47 lines)
`readCliVersion(baseUrl = import.meta.url)`: first baked `__RAFT_CLI_VERSION__` (tsup define), else
`./package.json`, else `../package.json` (line 42), else `"unknown"`. `normalizeVersion`: regex
`^v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$`, max length 128, rejects `0.0.0*`.
Rust: bake `"0.0.24"` at compile time (e.g. `env!("CARGO_PKG_VERSION")` with the crate version kept in sync
with upstream); keep `normalizeVersion` for `raft version` and for the daemon payload.

### 1.5 `src/main.ts` (425 lines)
- 110–126 `userCommandArgs(argv)`: strips `-p <v>`, `--profile <v>`, `--profile=<v>` from argv for error
  target computation; everything after `--` passed verbatim.
- 129–155: computes the "help target" (e.g. `raft message`) and visible subcommand list (excluding `help`).
- 157–193 `parseStageErrorToCliError`: commander message minus `^error:\s*` → `CliError{code:"INVALID_ARG"}`
  with `suggestedNextAction`:
  - `commander.missingArgument` → ``Run `<target> --help` for syntax.`` (special text for `raft manual get`).
  - `commander.unknownCommand` → ``Run `<target> --help` to list valid subcommands: a, b.`` (or
    `…to list available subcommands.` when none).
  - `commander.unknownOption` → ``Run `<target> --help` to list supported flags.``
  - default → ``Run `<target> --help` for syntax.``
- 195–203: `.name("raft")`, long description (see Appendix A), `-p, --profile <slug>` option.
- 205–219: `program.version("Raft CLI: <v>")` when `normalizeVersion` succeeds (prints `Raft CLI: 0.0.24`);
  otherwise a custom `-V, --version` option handler.
- 221–228: `program.exitOverride()`; `configureOutput({ outputError: () => {} })` (commander's own
  `error: …` line is suppressed; our envelope replaces it). Help output itself is still written by commander.
- 237–241: root `preAction` hook: `if (opts.profile) process.env.RAFT_PROFILE = opts.profile`.
- 244–361: command registration order (defines help listing order):
  `version, auth, agent, channel, thread, server, user, manual, knowledge, inbox, message, attachment, task,
  mention, profile, integration, reminder, app, wiki, migrate, action`.
- 374–392 `handleCliError(err)`:
  - `CliExit` → `process.exitCode = err.exitCode`.
  - `CliError` → `renderError` then exit 1.
  - `CommanderError` with code `commander.helpDisplayed` / `commander.version` → its exitCode (0).
    (`commander.help` — raised by `.help()` for a bare group or the `help` subcommand — is NOT special-cased,
    so it falls to the generic branch → `(outputHelp)` envelope, exit 1; see §2.4.)
  - other `CommanderError` → `parseStageErrorToCliError` envelope, exit 1.
  - anything else → `Unexpected error: <msg>` envelope (INTERNAL_BUG), exit 1.
- 394–425: `forwardManagedTransportIfNeeded(process.argv.slice(2), process.env)` runs **before** commander
  parse. If it forwarded, exit with forwarded status. Else `program.parseAsync(process.argv, {from:"node"})`.
  Forward error codes: `MANAGED_WRAPPER_FORWARD_FAILED`, `MANAGED_WRAPPER_UNAVAILABLE`,
  `MANAGED_WRAPPER_REQUIRED`.

### 1.6 `src/auth/managedTransport.ts` (118 lines) — managed wrapper forwarding
- Triggers only when `SLOCK_CLI_TRANSPORT_DIR` is set **and none of** `SLOCK_AGENT_PROXY_TOKEN_FILE`,
  `SLOCK_AGENT_PROXY_TOKEN`, `SLOCK_AGENT_TOKEN_FILE` is set (i.e. a bare `raft` on PATH inside a managed
  runtime delegates to the daemon-generated wrapper which injects credentials).
- Requires `SLOCK_HOME`, `SLOCK_AGENT_ID`, `SLOCK_AGENT_LAUNCH_DIR`. Expected dir =
  `path.resolve(SLOCK_HOME)/cli-transport/<safe(agentId)>/<safe(launchDir)>`, `safe = s.replace(/[^a-zA-Z0-9_.-]/g,"_")`.
  `SLOCK_CLI_TRANSPORT_DIR` must equal it (after resolve) and be a real directory (lstat, not symlink).
- `win32` → throws `MANAGED_WRAPPER_REQUIRED`.
- Wrapper `<dir>/raft` must be a real file (not symlink). Runs
  `spawnSync(wrapper, argv, { env, stdio: "inherit" })`; exit status = `result.status ?? 1`; spawn error →
  `MANAGED_WRAPPER_FORWARD_FAILED`.
- Test (`auth/managedTransport.test.ts:87–111`) spawns the real entry with a shell wrapper that records args
  and `exit 23`; asserts status 23 and args `message\ncheck\n`, and that no auth/parse errors happened.
  Rust: `std::process::Command::new(wrapper).args(argv).status()`; the wrapper must not re-trigger
  forwarding (it sets `SLOCK_AGENT_PROXY_TOKEN_FILE`).

---

## 2. Command framework and commander behaviour

### 2.1 Internal framework
- `src/core/command.ts` (74 lines): `defineCommand({name, description, arguments?, options?: [{flags,
  description, parse?, hidden?}], helpAfter?}, handler)`; `registerCliCommand(parent, def, runtimeOptions)`:
  - `parent.command(name).description(...)`; each `arguments[i]` → `.argument(spec)`; options:
    `hidden` → `addOption(new Option(flags, desc).hideHelp())`, `parse` → `.option(flags, desc, parse)`,
    else `.option(flags, desc)`; `helpAfter` → `.addHelpText("after", text)`.
  - `.action(async (...args) => { const ctx = createCommandContext(runtimeOptions); await handler(ctx, ...positionals, opts, cmd) })`.
    Commander passes `(positional1…, opts, Command)`.
  - Error path: `CliExit` rethrown; otherwise `toCliError(err)` → `renderError(ctx.io, cliErr)` →
    `throw new CliExit(cliErr.exitCode)`.
- `src/core/context.ts` (71 lines): `CommandContext { io, env, loadAgentContext(), createApiClient(agentCtx) }`;
  `CommandRuntimeOptions` = test injection (io/env/loadAgentContext/createApiClient). `AgentBootstrapError`
  → `CliError` with a per-code `suggestedNextAction` (MISSING_*, PROFILE_FILE_*, TOKEN_FILE_*, proxy codes).
- `src/core/io.ts` (13 lines): `CliIo { stdin?, stdout, stderr }`.
- `src/core/errors.ts` (281 lines): `CliErrorCode` union; `CliError` (156): fields `code, message, exitCode
  (default 1), layer, fault_domain (= faultDomain ?? layer), retryable, effect, effect_state, correlationId,
  proxy*, draftSaved, details, suggestedNextAction, textDetailMode, outputMode`; `CliExit` (207);
  `InternalBugError` (214: code `INTERNAL_BUG`, message `Unexpected error: <msg>`); `cliError()` (226);
  `toCliError()` (278). **Every error exits 1**; only other exit codes: 0 (success/help/version), forwarded
  wrapper status.
- `src/core/renderer.ts` (263 lines):
  - `axSurface` / `CliReplyText` branding (type-level only), `adoptCliReplyText`, `NL`.
  - `writeText(io, ...parts)` (248) → stdout, `writeDiagnostic` (257) → stderr, `writeJson` (261) →
    `JSON.stringify(payload) + "\n"` (compact) to stdout.
  - `renderError` (79–127): if `machinePayload` && `typedText` (IntegrationV1Error) → stderr
    `JSON.stringify(machinePayload)` (json mode) or `typedText` + `\n`. JSON mode:
    `{"ok":false,"error":{"code","message","fault_domain","layer","retryable","effect","correlation_id"[,"proxy"][,"next_action"]}[,"effect_state"][,"details"]}`
    — note key insertion order: `effect_state` is added after `error`, then `proxy` inserted into `error`,
    then `details`, then `next_action` into `error`. So serialized order = `ok, error{code,message,
    fault_domain,layer,retryable,effect,correlation_id,proxy?,next_action?}, effect_state?, details?`.
  - `formatErrorEnvelope` (137–197), text lines in order, each `\n`-terminated:
    `Error:`, `Code:`, `Fault domain:` (only if ≠ layer), `Layer:`, `Retryable: yes|no|unknown` (if defined),
    `Effect:`, `Correlation:`, `Proxy failure class:`, `Proxy cause code:`, `Proxy route family:`,
    `Proxy upstream layer:`, `Proxy upstream status:`, `Proxy response started: yes|no|unknown`,
    `Proxy response complete: yes|no|unknown`, `Draft saved: yes|no`, `Effect state: <compact JSON>`,
    `Next action:`. `textDetailMode === "omit_restated_lines"` drops Effect / Draft saved / Next action.
- Only `message send`, `integration invoke`, and invokeV1 set `outputMode = "json"` on errors when `--json`.
  All other `--json` commands still render errors as the text envelope on stderr.
- `src/core/apiFailure.ts` (57 lines) `apiFailureError(res, fallback)` (8): status ≥500 → code
  `PROXY_5XX` if `errorCode === "agent_proxy_failed"` else `errorCode ?? server5xxCode ?? "SERVER_5XX"`;
  else `errorCode ?? fallback`; message `error ?? "HTTP <status>"`; proxy diagnostics → daemon-proxy next action.
  (`commands/_apiFailure.ts` is a 1-line re-export.)

### 2.2 commander 12.1.0 features in use (all must be reproduced byte-for-byte)
| Feature | Where | Observable effect |
|---|---|---|
| `.name/.description/.version("Raft CLI: X")` | main.ts:195–219 | `-V, --version` → `Raft CLI: 0.0.24\n` stdout, exit 0; help line "output the version number" |
| Global `-p, --profile <slug>` | main.ts:200 | Accepted **anywhere** (commander parses parent options among subcommand args unless `enablePositionalOptions`, which is not used). `raft message send -p x` works. |
| `preAction` hook | main.ts:237 | sets `process.env.RAFT_PROFILE` (so later env reads see it) |
| `exitOverride` + `configureOutput.outputError=noop` | main.ts:221–228 | parse errors go through our envelope, no `error:` line |
| Auto `-h, --help` + `help [command]` | commander | "display help for command"; `help` subcommand listed last in group help |
| `.argument("<x>")`, `[x]`, `<x...>`, `[x...]` | see 2.3 | missingArgument error: `missing required argument 'x'` |
| `.option("--x <v>")` / `--flag` / `--no-x` not used | many | missing value: `option '--target <target>' argument missing` |
| Option argParser accumulators | see 2.3 | repeatable options |
| `new Option().hideHelp()` | knowledge/get.ts:80,85; knowledge/search.ts:105,110 | `--turn-id`, `--trace-id` hidden from help |
| `.addHelpText("after", …)` | main.ts:283 (`manual`), and `helpAfter` in knowledge get/search, message react, task receipt, task create, wiki, integration marketplace | extra text after help |
| `optsWithGlobals()` | agent/login.ts:112 `mergeParentLoginOpts` | `login start/wait/status` inherit `--server/--agent/--profile-slug/--profile-dir` from `login` |
| `allowExcessArguments` default **true** | commander | `raft user info a b` accepted silently (extra positional ignored) |
| `allowUnknownOption` default false | commander | `unknown option '--contnt'\n(Did you mean --content?)` |
| suggestSimilar (Damerau-Levenshtein, `suggestSimilar.js`) | commander | `(Did you mean X?)` / `(Did you mean one of X, Y?)`; max distance 3 / similarity > 0.4; option candidates include `--` prefix |
| Help formatting (`help.js`) | commander | width = `process.stdout.columns` if stdout TTY (stderr columns when help is written to stderr as part of error), else 80; description wrapping with hanging indent; `[options]` / `[command]` in usage; "Arguments:", "Options:", "Commands:" sections |
| Default option values | none rely on commander defaults beyond parse accumulators | — |

Commander source used for citations: `node_modules/commander/lib/command.js` (exitOverride 28, 57, 63–66;
unknownCommand 2073; unknownOption 2257–2261; missingArgument 2405; help 2453), `suggestSimilar.js:93–96`,
`help.js:372` (width).

### 2.3 Repeatable options & arguments
- Accumulator `parse` functions: `app/config.ts:72,77`; `message/send.ts:792,797`;
  `integration/app.ts:350,717,725` (`--scope`, `--scopes` in `sharedOptions`); `integration/login.ts:48`;
  `task/claim.ts:45,50`; `task/update.ts:82`; `task/create.ts:31`; `integration/invoke.ts:1785,1795`.
  Pattern: `(value, previous: string[] = []) => { previous.push(value); return previous; }`.
- Positional arguments: attachment view `[attachmentId]`; user info `<name>`; wiki read `<artifactId>`;
  mention notify/add `<resolutionIds...>`; integration marketplace `[query]`; integration invoke
  `[service] [action]`; channel members `<target>`; channel info `<target>`; channel mute/unmute `[target]`;
  message resolve `<id>`; manual/knowledge search `<keywords>`; manual/knowledge get `<topic>`;
  message send `[content...]` (rejected with a guidance error, content must come from stdin);
  profile show `[target]`.
- Manually built groups: `integration app` and `integration app prepare` (integration/app.ts:745–746).
  `mention` group registers `pending`, `notify`, `add` (mention/index.ts, execute.ts:86–89).
  `reminder log` registration also registers `ack` and `dismiss` (apps/reminder/log.ts:39–41, ack.ts:168–170).
  `manual` and `knowledge` register the **same** get/search definitions (main.ts:291–295).

### 2.4 Probe-verified parse behaviours (exit codes are the Rust contract)
- `raft` (no args): root help written to **stderr**, then envelope
  `Error: (outputHelp)\nCode: INVALID_ARG\nNext action: Run \`raft --help\` for syntax.\n`; exit 1.
  (commander calls `.help({error:true})` for a command with subcommands and no action → CommanderError
  `commander.help` "(outputHelp)", exitCode 1, which falls into the generic branch.)
- `raft message` (group, no sub): same shape with group help on stderr and target `raft message`; exit 1.
- `raft help message`: group help on **stdout**, then the `(outputHelp)` envelope on stderr; exit 1.
  `raft help nosuch`: root help on stderr + envelope; exit 1.
- `raft --help`, `raft message send --help`: help on stdout, exit 0. `--help --bogus` → help, exit 0
  (help is processed before unknown options).
- `raft --version extra` → prints version, exit 0.
- `raft message sned` → `Error: unknown command 'sned'\n(Did you mean send?)\nCode: INVALID_ARG\nNext action: Run \`raft message --help\` to list valid subcommands: send, check, read, search, resolve, react.`
- `raft message send --contnt x` → `Error: unknown option '--contnt'\n(Did you mean --content?)` + `…to list supported flags.`
- `raft message send --target` → `Error: option '--target <target>' argument missing`.
- Root rejects `--json` (`unknown option '--json'`).
- Required options are **not** declared via `requiredOption`; handlers check and throw
  `INVALID_ARG "--target is required"` etc., usually before `loadAgentContext()` (frameworkMigration.test.ts:167).

---

## 3. Auth / context env

### 3.1 Profile directory (`src/auth/env.ts:132–151`)
Precedence:
1. `RAFT_PROFILE_DIR ?? SLOCK_PROFILE_DIR` (both set & different → `PROFILE_ENV_CONFLICT`).
2. `(RAFT_HOME ?? SLOCK_HOME)/profiles/<slug>`.
3. `(HOME ?? os.homedir())/.slock/profiles/<slug>`.
Credential file: `<dir>/credential.json`.
JS `??` vs truthiness: an **empty-string** env var wins in `??` (not null/undefined) but later truthiness
checks treat it as unset. Rust must model `Option<String>` where `Some("")` is distinct from `None`.

### 3.2 `loadAgentContext(env)` (env.ts:213–320) — exact messages
- `activeCapabilities` = `SLOCK_AGENT_ACTIVE_CAPABILITIES` split on `,`, trimmed, empties dropped; `null` if unset/empty.
- `RAFT_PROFILE` & `SLOCK_PROFILE` both set and differ → `PROFILE_ENV_CONFLICT`:
  `RAFT_PROFILE=<a> and SLOCK_PROFILE=<b> disagree; unset one (RAFT_PROFILE is canonical, SLOCK_PROFILE is a deprecation alias).`
- Profile = `RAFT_PROFILE ?? SLOCK_PROFILE` (note `--profile` sets `RAFT_PROFILE` via preAction). If truthy:
  - managed launch (`SLOCK_CLI_TRANSPORT_DIR` || (`SLOCK_AGENT_LAUNCH_DIR` && `SLOCK_AGENT_ID` && `SLOCK_SERVER_URL`))
    → `PROFILE_MANAGED_CONTEXT_CONFLICT`: `RAFT_PROFILE=<slug> cannot select a different identity inside a managed Raft runtime. Remove RAFT_PROFILE/SLOCK_PROFILE from shell startup files and restart the runtime; the managed wrapper identity was not replaced.`
  - shadowed raw env keys (`RAW_AGENT_ENV_KEYS`, set ones) → **process.stderr** (not ctx.io):
    `raft: RAFT_PROFILE=<slug> active; ignoring K1, K2 from env.\n`
  - `readProfileCredential` (155–191): JSON must have string `apiKey`, `agentId`, `serverUrl`; optional
    `schemaVersion, agentName, serverId, credentialId, scopes, createdAt`. Errors are `PROFILE_FILE_*` with Node
    error text embedded (e.g. `ENOENT: no such file or directory, open '<path>'`) and V8 JSON.parse messages
    (e.g. `Expected property name or '}' in JSON at position 1 (line 1 column 2)`). **Rust cannot reproduce V8
    JSON error text naturally** — decide whether to hand-emulate or accept divergence.
  - Result: `clientMode: "self-hosted-runner"`, `secretSource: "profile-credential-file"`, `token = apiKey`.
- Else daemon bootstrap:
  - `SLOCK_AGENT_ID` missing → `MISSING_AGENT_ID "SLOCK_AGENT_ID is required"`;
    `SLOCK_SERVER_URL` missing → `MISSING_SERVER_URL "SLOCK_SERVER_URL is required"`. `serverId = SLOCK_SERVER_ID ?? null`.
  - Any of `SLOCK_AGENT_PROXY_URL/_TOKEN/_TOKEN_FILE`:
    no URL → `MISSING_AGENT_PROXY_URL "SLOCK_AGENT_PROXY_URL is required when agent proxy auth is set"`;
    both token forms → `MULTIPLE_AGENT_PROXY_TOKENS "Set only one of SLOCK_AGENT_PROXY_TOKEN or SLOCK_AGENT_PROXY_TOKEN_FILE"`;
    `readTokenFromFile` (193–211) trims, must be non-empty (`TOKEN_FILE_*` codes);
    no token → `MISSING_AGENT_PROXY_TOKEN`. Result: `serverUrl = SLOCK_AGENT_PROXY_URL`, `clientMode: "managed-runner"`,
    `secretSource: agent-proxy-token-file | agent-proxy-token-env`.
  - `SLOCK_AGENT_TOKEN_FILE`/`SLOCK_AGENT_TOKEN` → `LEGACY_MACHINE_UNSUPPORTED` (long message, env.ts:303–307).
  - else `MISSING_TOKEN "Neither SLOCK_AGENT_PROXY_TOKEN_FILE nor SLOCK_AGENT_PROXY_TOKEN is set. The daemon should inject proxy credentials when spawning the agent process, or use RAFT_PROFILE with a credential from \`raft agent login\`."`

### 3.3 Other env vars read by the CLI
`RAFT_HOME`, `SLOCK_HOME`, `HOME`, `RAFT_PROFILE_DIR`, `SLOCK_PROFILE_DIR`, `RAFT_PROFILE`, `SLOCK_PROFILE`,
`SLOCK_AGENT_*` (above), `SLOCK_CLI_TRANSPORT_DIR`, `SLOCK_AGENT_LAUNCH_DIR`, `SLOCK_CLI_TRANSPORT_TRACE_DIR`,
`SLOCK_CLI_CONSUMED_SEQ_STATE_DIR`, `SLOCK_CLI_DRAFT_STATE_DIR`, `RAFT_REVIEWER_ISOLATION` (must be one of
`1,true,0,false`; commands/reviewerIsolation.ts), `RAFT_AGENT_BRIDGE_STATE_DIR`, `RAFT_EXPECTED_AGENT_ID`,
`SLOCK_BRIDGE_WAKE_STREAM_IDLE_TIMEOUT_MS`, `SLOCK_CURRENT_WORKSPACE_PATH` (server/info.ts:146),
`HTTP(S)_PROXY`/`http(s)_proxy`/`ALL_PROXY`/`all_proxy`/`NO_PROXY`/`no_proxy` (proxy.ts),
`FORCE_COLOR`/`NO_COLOR` (not read by CLI; tests set them for commander/Node).

### 3.4 `raft agent login` (`commands/agent/login.ts`, 598 lines) + `agentLogin/deviceAuthClient.ts` (302)
- `login` (125): token from stdin (pipe) or hidden TTY prompt: `readline.createInterface({input, output: muted, terminal})`
  (176–200) with raw mode on/off (`rawModes [true,false]` asserted in test). Token must match
  `^sk_agent_[A-Za-z0-9_-]+$`, ≤4096 chars. Never echoed.
- Verify: `GET <server>/internal/agent-api/` with `Authorization: Bearer <token>`, `redirect: "error"`,
  15 s timeout. Response must contain `agentId, agentName, serverId, credentialId, scopes`, and `agentId === --agent`.
- `persistCredential` (410–445): `mkdir(profileDir, {recursive, mode:0o700})`; `mkdtemp(<dir>/.credential-)`;
  write `credential.json` mode 0600 as `JSON.stringify(obj, null, 2) + "\n"` with key order
  `{schemaVersion:1, serverUrl, agentId, agentName, serverId, credentialId, scopes, apiKey, createdAt}`;
  `rename` into place; remove temp dir.
- Idempotent re-login (473–538) validates the existing credential against whoami.
- `start/wait/status` (204/234/269) use `mergeParentLoginOpts` (`optsWithGlobals`). `wait` polls every 5 s up to
  15 min, then mints via `POST <server>/api/agents/<id>/credentials` body `{}` with `Bearer <accessToken>`.
- `deviceAuthClient.ts`: **undici `fetch` without proxy dispatcher** (ignores proxy env).
  `POST <base>/api/auth/device/authorize` body `{clientName?}` (relative URIs made absolute; defaults
  `expires_in 600`, `interval 5`); `pollDeviceToken`: `POST /api/auth/device/token` `{deviceCode}`; retries on
  network error and `authorization_pending`; other codes terminal; deadline → `expired_token`;
  `ACTIONABLE_ERROR_MESSAGES` map.
- Error codes: `INVALID_AGENT_ID` (`describeInvalidAgentIdShape` 556), `PROFILE_ALREADY_EXISTS`,
  `CREDENTIAL_CHECK_FAILED`, `INVALID_AGENT_TOKEN`, `AGENT_IDENTITY_MISMATCH`, `mint_*` (`describeMintError` 573).
- `resolveProfilePaths` (320) uses **`process.env`**, not `ctx.env` (matters for test injection only).
- `agent list` (agent/list.ts): device flow + `GET <server>/api/agents/manageable`; uses browserHandoff Enter-to-open.

---

## 4. HTTP

### 4.1 `src/client.ts` (553 lines) `ApiClient` (84)
- `rewriteAgentCredentialPath` (91–155): legacy `/internal/agent/<id>/…` and `/api/attachments/…` paths are
  mapped to `/internal/agent-api/…` (tests `client.test.ts:53,282,328`).
- Headers on JSON `request()`:
  `Authorization: Bearer <token>`, `X-Agent-Id: <agentId>`, `X-Raft-Client: cli`, `X-Server-Id` (if serverId),
  `Content-Type: application/json`, `X-Raft-Client-Capabilities: manual-context-v1`
  (`shared/knowledgeContext.ts:3–4`), `X-Slock-Agent-Active-Capabilities: <comma list>` (if set).
  Body = `JSON.stringify(body)` (undefined → no body).
- Response: JSON parsed only if `content-type` includes `application/json`; else `data=null`,
  `error = "HTTP <status>"`. Invalid JSON → `INVALID_JSON_RESPONSE "Invalid JSON response from server/proxy (HTTP n)"`.
- 403 with `requiredScope` → `SCOPE_DENIED` "Permission denied. This agent lacks the `X` capability, …".
- `errorCode = body.errorCode ?? body.code`; `suggestedNextAction ?? suggested_next_action`; proxy diagnostics
  parsed (`parseProxyDiagnostics` 503). 5xx with `agent_proxy_failed` throws proxy-scoped CliError centrally.
- `/events` responses: `events` copied to `messages` (legacy shape).
- **No retries, no timeouts** in the main client.
- `requestMultipart` / `requestBinary`: 302 followed once only for GET to a different-origin `https:` URL without
  URL credentials, with `redirect:"error"`, `credentials:"omit"`, `referrerPolicy:"no-referrer"`, no Raft headers;
  else `"Attachment redirect target was rejected"` status 502. Signed URL scrubbed from traces/errors.
- `streamWakeHints`: `GET /internal/agent-api/wake-hints/stream?...` (SSE).
- A raw fetch throw propagates → `INTERNAL_BUG "Unexpected error: fetch failed"` (probe: `channel create` with
  unreachable server). Canonical-proxy fetch wraps it as `CanonicalFetchTransportError`.

### 4.2 `src/proxy.ts` (221 lines)
- `getProxyUrlForTarget` (55): https → `HTTPS_PROXY || https_proxy || ALL_PROXY || all_proxy`; http →
  `HTTP_PROXY || http_proxy || ALL_PROXY || all_proxy` (`||`, empty = unset).
- `shouldBypassProxy` (164): `NO_PROXY || no_proxy`, comma list; `*`; `host:port` entries (default ports via
  `getDefaultPort` 37); `.suffix`/`*.suffix` suffix matching (`hostMatchesNoProxyEntry` 48).
- `buildFetchDispatcher` (184): cached `undici.ProxyAgent` per proxy URL.
- `fetchWithCanonicalProxy` (200): classifies failures (dns/connect/tls/timeout/proxy/unknown, `classifyFetchTransportFailure` 103)
  into `CanonicalFetchTransportError` message `fetch failed for <url with query values [redacted]>: <class>/<CODE>`
  (`credentialFreeDiagnosticUrl` 151).
- **Which paths use proxies**: ApiClient + integration v0 HTTP actions + manifest fetch go through
  `fetchWithCanonicalProxy`. **Global `fetch` (no proxy)**: invokeV1, attachment presigned PUT
  (upload.ts:158), bridge activity drain / wake adapter POST, deviceAuthClient (undici fetch w/o dispatcher).
  Rust: `reqwest` with `.no_proxy()` for those, and explicit proxy selection replicating proxy.ts for the rest
  (do **not** rely on reqwest's env proxy logic — its NO_PROXY semantics differ).

### 4.3 `src/transportTrace.ts` (243 lines)
Only if `SLOCK_CLI_TRANSPORT_TRACE_DIR` set: appends JSONL span records to
`<dir>/daemon-trace-cli-transport-<ISO with ':'/'.' → '-'>-<pid>-<8 hex>.jsonl` (dir 0700, file 0600).
`routeFamilyForPath` (100), `targetHostClassForUrl` (168; recognizes api.slock.ai, api.raft.build, loopback),
`upstreamLayerForFetchError` (176), `sanitizeOriginalMessage` (235). Never affects stdout/stderr.

### 4.4 Agent-API contract layer
- `src/agentApiPath.ts` (572): wraps shared `createAgentApiClient`, `requestAgentApiRawRoute`,
  `buildAgentApiRawRoutePath`, `buildLegacyAgentApiPath`, `parseAgentApiResponse`, `getAgentApiResponseKind`,
  `agentApiContract`. Failure mapping: `missing_path_param` / request mismatch → `INVALID_ARG`; empty
  response / response mismatch / missing route → `INVALID_JSON_RESPONSE`; transport failure → `CHECK_FAILED`
  "Agent API <key> transport request failed", fault domain `agent_api_transport`, retryable true for GET else
  false (CliError cause preserved); http failure → `CHECK_FAILED`. `createAgentApiSurfaceClient` exposes
  resources: server, events, history, knowledge, wiki, tasks, migrations, reminders, appSources, apps, messages,
  channels, threads, profile, integrations, actions, mentions, attachments.
- `src/daemonApiPath.ts` (169): `createDaemonApiSurfaceClient` → `runtime.version` (GET `/runtime-version`),
  `inbox.check` (GET `/inbox`), `inbox.ack` (POST `/inbox/ack`), `wakeHints.fetch` (GET `/wake-hints`),
  `activity.forward` (POST `/activity`) — all under `DAEMON_API_BASE_PATH = "/internal/agent-api"`
  (`shared/daemonApiContract.ts:6,230–269`). Rejections append `(cause=…; path=…; expected_kind=…; actual_kind=…)`.
- `shared/agentApiRawClient.ts`: zod `params.parse`; path params `encodeURIComponent`; query via
  `URLSearchParams` (arrays appended repeatedly, null/undefined skipped; `+` for spaces!); request body zod-parsed
  **before** sending; response zod-parsed. Fixed messages `Agent API <key> … did not match the shared contract`.
- `shared/generated/agentApiRoutes.ts`: 77 routes all under `/internal/agent-api` (send, v2/send, history,
  events, knowledge, knowledge/search, tasks/*, reminders/*, channels/:id/join, resolve-channel,
  attachments/:id (binary), upload (multipart), attachment-upload-capabilities, attachment-upload-sessions(+/complete,/cancel),
  integrations/app/*, migrations/*, wiki/*, mentions/*, wake-hints(/stream), activity, inbox, runtime-version, …).
- **zod semantics that change bytes** (verified): `z.object` parse **strips unknown keys and re-orders output keys
  to schema order** (`{"b":"x","extra":1,"a":2}` → `{"a":2,"b":"x"}`); `.passthrough()` keeps extras after the
  known keys. Lines mentioning passthrough/loose/strict: agentApiContract.ts 13, daemonApiContract.ts 3,
  agentApiMessageContract.ts 4 — audit each schema individually.
  Transforms: `.trim()` on ids (agentApiContract.ts:341,345,355,1308), `z.coerce.number()` (557),
  `.default({})`/`.default([])` (1598–1599). Rust serde structs with fields in schema order, `deny_unknown`
  **off**, `#[serde(flatten)] extra` only for passthrough schemas, reproduce this. `--json` outputs that echo
  server data therefore emit schema-ordered keys.
- Shared zod depends on `zod ^4.3.6` and needs `zod-openapi` at runtime (import side effect).

### 4.5 Multipart
`attachment upload` (small files), `profile update --avatar`, `server update` (avatar), `integration app logo`
use `FormData` + `Blob` (undici generates boundary `----formdata-undici-0<digits>`; servers don't care).
Field names: upload → `file` (filename = basename, type = inferred MIME), `channelId`, optional `mimeType`;
app logo → `avatar` (integration/app.ts:525).

---

## 5. Local state files

| File | Path | Format | Write discipline | Source |
|---|---|---|---|---|
| Profile credential | `<profileDir>/credential.json` (§3.1) | pretty JSON 2-space + `\n`, fixed key order | dir 0700, mkdtemp + rename, file 0600 | agent/login.ts:410 |
| Consumed seqs | `<base>/slock-cli-consumed-seq/<agentId>/consumed-seqs.json` | compact `{"targets":{"<target>":{"seq":N,"readOrder":N}},"nextReadOrder":N}` | best-effort (errors swallowed) | message/_consumedSeqState.ts:42–147 |
| Continue drafts | `<base>/slock-cli-attested-send/<agentId>/continue-state.json` | `{"targets":{"<t>":{content,attachmentIds,mentions?,savedAt,reholdCount,seenUpToSeq?}}}`; TTL 10 min | set/clear errors **not** caught | message/_continueDraftState.ts |
| Integration invocation | `<root>/integration-invocations-v1/<uuid-lower>.json`; root = profile credential dir, else `(RAFT_HOME‖SLOCK_HOME trimmed ‖ HOME/.slock)/integration-invocations/<agentId>` | pretty JSON 2-space + `\n`, schema `raft-integration-invocation.v1`, states dispatching/accepted_unverified/verified/failed/indeterminate; key = base64url(32 random bytes), `keySha256` | temp `<file>.<pid>.<hex16>.tmp` wx → fsync → rename → chmod 600 → dir fsync; lock `<file>.lock` (wx, contains pid) | integration/invocationStoreV1.ts:78–415 |
| Integration cookie session | `<profileDir or root/integration-sessions/<agentId>>/integrations/<encodeURIComponent(clientId) with % → _>.json` | JSON | `writeFileSync` 0600 (not atomic) | integration/_session.ts:54–336 |
| Integration CLI profile env | `<RAFT_HOME>/integration-profiles/<server>/<agent>/<service>` (dir, 0700; `~` expanded) | — | mkdir | integration/manifest.ts:497–552 |
| `--output` secret sink | user path | raw bytes | open `wx` 0600, dev/ino identity check, `LOCAL_WRITE_FAILED` fault domains `file_write:prepare`/`file_write:commit`; unsupported on win32 | integration/privateSecretSink.ts |
| Bridge state | `--state-dir` ‖ `RAFT_AGENT_BRIDGE_STATE_DIR` ‖ `<profileDir>/agent-comms-core/<safe(agentId)>/<safe(adapter="default")>` (`safe`: `[^a-zA-Z0-9._-]`→`_`, ≤120 chars, `"unknown"` if empty) | `session.json` (pretty, `{coreSessionId,lastSeenHintSeq?}`), `wake-hints.jsonl`, `proofs.jsonl`, `bridge.log` (NDJSON `{ts,...}`, rotated to `.1` at 5 MiB), `bridge.lock` (pretty owner JSON `{ownerId,pid,createdAt,profileSlug,agentId,adapterInstance}`) | dir 0700; appends 0600; lock `wx`; stale if `kill(pid,0)` → ESRCH (retry once); release only if ownerId matches | agentCommsCore/bridge.ts:270–330, 635–645, 944–1010 |
| Transport trace | §4.3 | JSONL | append | transportTrace.ts |

`_privateStateFile.ts` (108 lines) shared by consumed/draft state:
- `<base>` = override env (`SLOCK_CLI_CONSUMED_SEQ_STATE_DIR` / `SLOCK_CLI_DRAFT_STATE_DIR`) else trimmed
  `RAFT_HOME`/`SLOCK_HOME` else `~/.slock` (15–34).
- namespace dir and agent dir: 0700, ownership (uid) checked (6–13); agentId must match `^[A-Za-z0-9_-]+$` else
  `Invalid local state agent identity`.
- read: `O_NOFOLLOW`, mode `& 0o077` must be 0 (36–45).
- write: `<file>.<uuid>.tmp` `wx` 0600 then rename (47–57). No locking.
- one-time legacy import from `os.tmpdir()/<ns>/<agentId>/<file>` (≤1 MB) then legacy file removed (59–106).

---

## 6. Other dependencies

- **ajv / safe-regex2** (`integration/manifestV1.ts:1–2, 395–515, 894–900`): `new Ajv2020({allErrors:false,
  strict:false, validateSchema:true, formats:{}})` compiles manifest `input`/`output` JSON Schemas; custom
  bounded keyword whitelist; each `pattern` checked with `new RegExp(p, "u")`-style construction plus `safeRegex`
  (star-height ≤1). Rust: `jsonschema` crate (draft 2020-12) + JS-regex semantics (`regress` crate is ECMAScript
  compatible) + a port of safe-regex2 star-height analysis. Error strings from ajv surface in some messages
  (check manifestV1.test.ts, 9 cases).
- **readline**: only agent/login.ts:12,182 (hidden token prompt). Rust: `rpassword`-style raw mode read.
- **child_process**: `auth/managedTransport.ts` (spawnSync wrapper), `core/browserHandoff.ts` (`open` on darwin,
  `cmd /c start "" <url>` on win32, else `xdg-open`; detached, stdio ignore, unref; Enter-to-open installs a
  stdin `data` listener only when stdin is a TTY). Used by `agent list` only.
- **crypto**: `randomUUID` (bridge ids `core_<uuid>`, `attempt_<uuid>`, `owner_<uuid>`, upload
  `clientRequestId`, state temp names, invokeV1), `randomBytes` (trace hex8, invocation key/hex16),
  `createHash("sha256")` (actionV1.ts:42–69 `canonicalJson` = sorted keys, `undefined` skipped, then sha256;
  migrate/index.ts file digest; invocation `keySha256`).
- **setClockTimeout / currentDate / currentTimeMs** (`shared/clock.ts`) are thin wrappers over
  `setTimeout`/`Date.now` (test seams).
- Shared formatters used verbatim for stdout text: `formatAgentInboxSnapshot`, `formatAgentReplyAffordance`,
  `formatUtcTimestamp`, `renderThirdPartyInertJson`, `structuredRaftMentionStillAppears`,
  `joinRaftChannelByTarget`, `validateKnowledgeContext`, `validateActionCardAction`, `MANUAL_INDEX_COMMAND`,
  external agent schemas. These live in `packages/shared` and must be ported too.

---

## 7. Hardest modules

### 7.1 `commands/message/send.ts` (815 lines; 78 tests)
- Content **only from stdin**; positional `[content...]` / `--content` rejected (`rejectArgContent` 254).
  TTY stdin or empty/whitespace → `MISSING_CONTENT` (`resolveSendContent` 166).
- `--send-draft`: watches stdin for `SEND_DRAFT_STDIN_OBSERVATION_MS = 1000` (`resolveOptionalSendContent` 178)
  using `readable` events + timer; bytes after deadline treated as absent; stderr diagnostic
  `No stdin bytes were detected within 1000ms; the stored draft will now be sent.` (_format.ts:572).
  Tested with a real child-process pipe (send.test.ts:172). Rust: non-blocking stdin poll with deadline.
- `--mention` (repeatable, 792/797) must be `human|agent:<uuid>:<handle>`, handle `[\p{L}\p{N}_-]+`
  (`parseMentionSelector` 93). `--attachment-id` repeatable.
- Thread-context guard (`detectThreadContextParentSend` 362) uses consumed-seq readOrder of parent vs thread.
- Body `AgentApiSendV2Body` (POST `/internal/agent-api/v2/send`): `target, content, draftReholdCount,
  freshnessContextMode, seenUpToSeq, sendDraft, continueAnyway, draftReplacedExisting, attachmentIds, mentions`
  (zod order).
- Outcomes (`classifyMessageSendOutcome` 130): held → error `SEND_HELD_AS_DRAFT` with
  `textDetailMode: "omit_restated_lines"` (after printing held output to stdout), saves draft; sent → prints,
  records consumed seq, clears draft. Partial mention delivery → `MENTION_DELIVERY_FAILED`.
- `markSendFailureDraftSaved` (396) wraps **every** failure, so `Draft saved: no` appears even for
  `--target is required`; after transport failure with draft saved it sets retryable `no` and a very long
  UNKNOWN-delivery next action.
- `RAFT_REVIEWER_ISOLATION` validation (reviewerIsolation.ts).

### 7.2 `commands/integration/invoke.ts` (2194 lines; 103 tests) + `invokeV1.ts` (1092; 19) + `manifestV1.ts` (907; 9) + `actionV1.ts` (347) + `invocationStoreV1.ts` (415)
- Handler 1769–2190. Options include repeatable `--param key=value` (1785) with `key=@file` and `@-` (stdin),
  `--header`-style repeatable (1795), `--data-json`, `--data-file`, `--output`, `--json`.
- v0 manifest actions: HTTP via `fetchWithCanonicalProxy`; file responses streamed to a temp dir with an
  elaborate `effect_state` object (100 MB default, 1 GB max) and cleanup outcomes; errors rendered JSON when `--json`.
- v1 (`invokeManifestActionV1` invokeV1.ts:573): **global fetch**, `redirect:"manual"`, idempotency header
  name from manifest, bounded body read (296–335), receipts schema `raft-integration-action-receipt.v1`
  (361–478), `IntegrationV1Error` (146) carries `machinePayload` + `typedText` (renderer special-case).
- Invocation store: locking, fsync, crash-safe state machine (§5).

### 7.3 `commands/agent/bridge.ts` (661; 21 tests) + `agentCommsCore/bridge.ts` (1124; 15) + `external/raftChannelWakeAdapter.ts` (186; 5)
- Requires `clientMode === "self-hosted-runner"` (message: ``raft agent bridge requires a self-hosted profile credential; run with `raft --profile <slug> agent bridge`.``)
  and a profile slug; `--expected-agent` or `RAFT_EXPECTED_AGENT_ID` must match.
- Long-running loop: stream mode = SSE on `wake-hints/stream` with idle watchdog
  (`SLOCK_BRIDGE_WAKE_STREAM_IDLE_TIMEOUT_MS`, default 60 s); poll mode = daemon-api `wakeHints.fetch`
  (default 5000 ms). Exponential backoff capped 60 s. Reconcile every 120 s (fast 3 s) — not flags.
- `runAgentCommsBridgeOnce` (380–480): replays pending `wake-hints.jsonl`, fetches with
  `since = lastSeenHintSeq ?? "latest"`, `limit 50`; per hint writes proofs `server_delivered`,
  `harness_accepted`, handoff event, optional wake adapter; updates `session.json`; lifecycle events
  `starting / connected_replaying / handoff_pending / listening_idle`. Hint identity `wakeHintKey`
  (`id:` / `seq:` / `hint:<json>`), content fields (`content, body, text, message`) stripped before persisting.
- Activity drain: GET `<endpoint>/activity/drain?max=` with `x-raft-bridge-token`, 3 s timeout (global fetch);
  wake adapter POST with `x-raft-bridge-token` (raftChannelWakeAdapter.ts:76–79). Activity text truncated at
  `EXTERNAL_AGENT_ACTIVITY_TEXT_LIMIT` with `\n[truncated]` (uses JS `.length` = UTF-16 units).
- `--json`: NDJSON events on stdout; diagnostics on stderr. No signal handling; lock released in `finally`.
  Rust: tokio loop; SIGINT default kill leaves lock → next start detects stale pid (ESRCH).

### 7.4 `commands/attachment/upload.ts` (365; 14 tests)
Flow: validate `--path` (exists, regular file, non-empty; messages `--path is required`,
`--path does not exist: <p>`, `--path is not a regular file: <p>`, `--path is empty; refusing to upload a 0-byte attachment`);
target from `--target`/legacy `--channel` (else `MISSING_CHANNEL`); MIME = explicit (`^[a-z0-9][a-z0-9!#$&^_.+-]*/…$`i,
lowercased) ‖ magic bytes (PNG/JPEG/GIF/WEBP from first 16 bytes) ‖ extension map (jpg,jpeg,png,gif,webp,pdf,txt,md,json,csv)
‖ `application/octet-stream`. GET capabilities (404 → fallback 50 MiB, no direct upload); size check message
`--path is <X>; max upload size is <Y>` (`formatBytes`: B integer, KB/MB/GB `toFixed(1)`, base 1024);
POST resolve-channel; if direct upload and `size >= threshold`: create session (`clientRequestId = randomUUID()`),
PUT presigned URL (global fetch, streaming body, `Content-Length`, `redirect:"error"`, one retry on
throw/408/429/5xx, 412 = already exists), cancel session on definitive failure, complete with up to 3 attempts
and 250/500 ms waits on `UPLOAD_OBJECT_NOT_FOUND`/`UPLOAD_VERIFICATION_IN_PROGRESS`. Else multipart `/upload`.

### 7.5 Other notable
- `integration/_session.ts` (426): custom Set-Cookie parser, cookie path matching, callback handoff GET
  `redirect:"manual"`, redaction of service error text.
- `integration/manifest.ts` (568): https-only manifest fetch, 64 KiB cap, 10 s timeout, well-known alias
  fallback between `/.well-known/raft-agent-manifest.json` and `/.well-known/slock-agent-manifest.json`;
  `formatShellExports` single-quote shell escaping.
- `server/_format.ts` (488), `message/_format.ts` (609), `task/_format.ts` (446), `integration/_format.ts` (340):
  large amounts of exact text; tested via `_format.test.ts` files (≈ 150 cases total).
- `knowledge` (manual): `validateKnowledgeContext` (shared/knowledgeContext.ts:20–65) — 12–500 chars after trim
  (JS `.length`, UTF-16), rejects `[target=…msg=`, fenced code, URLs, `sk_(agent|machine|computer)_…`,
  `Bearer …`, `api key|token|password|secret [:=] …`, >4 lines. Both-invalid → `KNOWLEDGE_CONTEXT_INVALID`
  with multi-line next action (knowledge/context.ts:32–55). Output is server content byte-preserved plus a
  trailing `\n` if missing (get.ts:38–40).

---

## 8. Test infrastructure → Rust mapping

- Runner: `node --import tsx --test` via `scripts/run-tests-with-manifest.mjs` with a custom reporter; compares
  against `test-execution-manifest.json` (schemaVersion 2, `files[] {file, count, names[] ordered}`,
  103 files / 918 tests) and fails on drift; `--update-manifest` rewrites. Rust: consider a similar
  manifest-parity check (e.g. `cargo nextest list` → JSON) to prove all 918 cases were ported or explicitly waived.
- Test styles:
  1. **Direct handler calls** (majority, e.g. `commands/channel/create.test.ts`): local `memoryIo()` collects
     stdout/stderr chunks; `createCommandContext({io, loadAgentContext, createApiClient})` with a fake
     `request(method, path, body)` recording calls; asserts exact request list and exact output strings.
     Rust: `CommandContext` trait objects / generics with an in-memory `Io` and a fake `ApiClient` trait.
  2. **Commander-level** (`core/command.test.ts`, invoke, send, app, task update, knowledge get/search, bridge,
     login — 9 files): `new Command(); exitOverride(); register…(program.command("x"), {io}); parseAsync([...], {from:"user"})`.
     Rust: build the clap (or custom) tree and call `try_get_matches_from`.
  3. **Fetch mocks** (10 files: client, server/update, channel/create, attachment/upload, message/_inbox,
     integration list/invoke/env/manifest/login): replace `globalThis.fetch`. Rust: `wiremock`/`httpmock`
     or an injected HTTP trait.
  4. **Real local HTTP servers** (proxy.test.ts, integration/invoke, agent/bridge, agent/login):
     `http.createServer` on 127.0.0.1. Rust: `wiremock` / `axum` test server.
  5. **Real process spawns** of `src/index.ts` (`parserOutput.test.ts` 10 cases — exact help/error text with
     `FORCE_COLOR=0 NO_COLOR=1`, `SLOCK_CLI_TRANSPORT_DIR` deleted; `managedTransport.test.ts`; `agent/login.test.ts`
     pipe mode; `runtimePreflight.test.ts`). Rust: `assert_cmd` + `CARGO_BIN_EXE_raft`.
  6. **Filesystem/tmp** (24 files use `mkdtemp`): mode bits asserted (`& 0o777 === 0o600`). Rust: `tempfile`.
  7. **Source-scanning gates** (no Rust equivalent needed / re-think): `printSeam.test.ts` (no direct
     `io.stdout.write` in commands; `axSurface` only in `_format.ts`), `agentApiNoBypass.test.ts` (all migrated
     routes go through generated bindings), `axManifest.test.ts` (skipped in source snapshot: RELEASE_SOURCE),
     `bundleConfig.test.ts`, `publishPackage.test.ts` (builds dist + wrappers), `shimPackage.test.ts` (slock
     shim) → N/A for Rust.
- Suggested golden strategy: run every help path and a matrix of error paths through the TS CLI (probe recipe §11)
  and snapshot stdout/stderr/exit; replay against the Rust binary (`insta` + `assert_cmd`).

---

## 9. Inventory: command → source → lines → tests

Line counts are non-test source; test counts from `test-execution-manifest.json`. "—" = no dedicated test file.

| Command | Source file | Lines | Test file | Tests |
|---|---|---:|---|---:|
| `raft` root / parse / errors | main.ts | 425 | parserOutput.test.ts | 10 |
| (entry) | index.ts / runtimePreflight.ts / version.ts | 8 / 39 / 47 | runtimePreflight.test.ts / version.test.ts | 6 / 4 |
| (framework) | core/command.ts, context.ts, errors.ts, renderer.ts | 74/71/281/263 | core/command.test.ts, printSeam.test.ts | 10 / 3 |
| (managed forward) | auth/managedTransport.ts | 118 | auth/managedTransport.test.ts | 5 |
| (auth env) | auth/env.ts | 320 | auth/env.test.ts | 23 |
| (http) | client.ts / proxy.ts / transportTrace.ts | 553 / 221 / 243 | client.test.ts / proxy.test.ts / transportTrace.routeFamily.test.ts | 24 / 2 / 3 |
| (agent-api) | agentApiPath.ts / daemonApiPath.ts | 572 / 169 | agentApiPath.test.ts, agentApiNoBypass.test.ts / daemonApiPath.test.ts | 13+5 / 3 |
| version | commands/version.ts | 94 | commands/version.test.ts | 7 |
| auth whoami | auth/whoami.ts | 37 | auth/whoami.test.ts | 1 |
| agent login (+start/wait/status) | agent/login.ts + agentLogin/deviceAuthClient.ts | 598 + 302 | agent/login.test.ts, agentLogin/deviceAuthClient.test.ts | 20 + 7 |
| agent list | agent/list.ts + core/browserHandoff.ts | 162 + 69 | agent/list.test.ts, core/browserHandoff.test.ts | 4 + 2 |
| agent bridge | agent/bridge.ts + agentCommsCore/bridge.ts + external/raftChannelWakeAdapter.ts | 661 + 1124 + 186 | agent/bridge.test.ts, agentCommsCore/bridge.test.ts, external/raftChannelWakeAdapter.test.ts | 21 + 15 + 5 |
| (agent formats) | agent/_format.ts | 245 | agent/_format.test.ts | 5 |
| channel info | channel/info.ts | 69 | channel/info.test.ts | 3 |
| channel members | channel/members.ts | 45 | channel/members.test.ts | 3 |
| channel create | channel/create.ts | 85 | channel/create.test.ts | 4 |
| channel update | channel/update.ts | 127 | channel/update.test.ts | 3 |
| channel archive / unarchive | channel/lifecycle.ts | 94 | channel/lifecycle.test.ts | 4 |
| channel add-member | channel/add-member.ts | 116 | channel/add-member.test.ts | 4 |
| channel remove-member | channel/remove-member.ts | 130 | channel/remove-member.test.ts | 3 |
| channel join | channel/join.ts | 73 | channel/join.test.ts | 5 |
| channel leave | channel/leave.ts | 105 | channel/leave.test.ts | 4 |
| channel mute / unmute | channel/mute.ts | 124 | channel/mute.test.ts | 6 |
| thread unfollow | thread/unfollow.ts | 93 | thread/unfollow.test.ts | 5 |
| server info | server/info.ts (+ _format.ts 488) | 194 | server/info.test.ts, server/_format.test.ts | 7 + 4 |
| server update | server/update.ts | 125 | server/update.test.ts | 4 |
| user info | user/info.ts | 132 | user/info.test.ts | 3 |
| manual/knowledge get | knowledge/get.ts + context.ts | 127 + 59 | knowledge/get.test.ts | 8 |
| manual/knowledge search | knowledge/search.ts | 153 | knowledge/search.test.ts | 9 |
| inbox check | inbox/check.ts (+ _format.ts 36) | 54 | inbox/check.test.ts | 6 |
| message send | message/send.ts (+ _continueDraftState 94, _consumedSeqState 187, _privateStateFile 108) | 815 | message/send.test.ts, _consumedSeqState.test.ts, _privateStateFile.test.ts, reviewerIsolation.test.ts | 78 + 5 + 5 + 2 |
| message check | message/check.ts (+ _inbox.ts 90) | 43 | message/check.test.ts, message/_inbox.test.ts | 6 + 7 |
| message read | message/read.ts | 161 | message/read.test.ts | 12 |
| message search | message/search.ts | 247 | message/search.test.ts | 12 |
| message resolve | message/resolve.ts | 104 | message/resolve.test.ts | 5 |
| message react | message/react.ts | 75 | message/react.test.ts | 2 |
| (message formats) | message/_format.ts, freshness/_format.ts | 609, 199 | message/_format.test.ts | 39 |
| attachment upload | attachment/upload.ts | 365 | attachment/upload.test.ts | 14 |
| attachment view | attachment/view.ts | 97 | attachment/view.test.ts | 10 |
| attachment comments | attachment/comments.ts | 53 | — | 0 |
| (attachment formats) | attachment/_format.ts | 94 | attachment/_format.test.ts | 3 |
| task list | task/list.ts | 98 | task/list.test.ts | 6 |
| task create | task/create.ts | 87 | task/create.test.ts | 7 |
| task claim | task/claim.ts | 187 | task/claim.test.ts | 7 |
| task unclaim | task/unclaim.ts | 70 | task/unclaim.test.ts | 3 |
| task assign | task/assign.ts | 117 | task/assign.test.ts | 6 |
| task unassign | task/unassign.ts | 108 | task/unassign.test.ts | 4 |
| task update | task/update.ts | 139 | task/update.test.ts | 8 |
| task receipt | task/receipt.ts | 106 | task/receipt.test.ts | 2 |
| task delete | task/delete.ts | 75 | task/delete.test.ts | 4 |
| task convert | task/convert.ts | 69 | task/convert.test.ts | 4 |
| task amend | task/amend.ts | 118 | task/amend.test.ts | 3 |
| task history | task/history.ts | 58 | task/history.test.ts | 1 |
| (task formats / manual contract) | task/_format.ts | 446 | task/_format.test.ts, task/mineManualContract.test.ts | 25 + 4 |
| mention pending | mention/pending.ts | 43 | mention/pending.test.ts | 2 |
| mention notify / add | mention/execute.ts | 89 | mention/execute.test.ts | 4 |
| (mention formats) | mention/_format.ts | 304 | mention/_format.test.ts | 11 |
| profile show | profile/show.ts (+ _format.ts 95) | 64 | profile/show.test.ts, profile/_format.test.ts | 4 + 2 |
| profile update | profile/update.ts | 206 | — (frameworkMigration.test.ts covers) | 0 |
| integration list | integration/list.ts | 58 | integration/list.test.ts | 3 |
| integration marketplace | integration/marketplace.ts | 98 | integration/marketplace.test.ts | 4 |
| integration login | integration/login.ts + _session.ts | 108 + 426 | integration/login.test.ts, _session.test.ts | 10 + 2 |
| integration env | integration/env.ts + manifest.ts | 280 + 568 | integration/env.test.ts, manifest.test.ts | 17 + 22 |
| integration invoke | integration/invoke.ts + invokeV1.ts + manifestV1.ts + actionV1.ts + invocationStoreV1.ts + privateSecretSink.ts + readiness.ts + readinessV1.ts | 2194 + 1092 + 907 + 347 + 415 + 115 + 369 + 240 | invoke.test.ts, invokeV1.test.ts, manifestV1.test.ts, readiness.test.ts, readinessV1.test.ts | 103 + 19 + 9 + 7 + 3 |
| integration app … (prepare register / prepare recover-owner / rotate-secret / transfer-owner / update / logo / clear-logo / share-link / share-link-status / revoke-share-link / request-publish / request-unpublish / delete / list / status) | integration/app.ts + appReceipts.ts | 762 + 181 | integration/app.test.ts, appManualContract.test.ts, appReceipts.test.ts | 24 + 5 + 3 |
| (integration formats) | integration/_format.ts | 340 | integration/_format.test.ts | 8 |
| reminder schedule | reminder/schedule.ts (+ _duration 11, _resolve 40) | 163 | reminder/schedule.test.ts | 13 |
| reminder list | reminder/list.ts | 59 | — (frameworkMigration) | 0 |
| reminder cancel | reminder/cancel.ts | 51 | — (frameworkMigration) | 0 |
| reminder snooze | reminder/snooze.ts | 59 | — (frameworkMigration) | 0 |
| reminder update | reminder/update.ts | 78 | — (frameworkMigration) | 0 |
| reminder log | apps/reminder/log.ts (re-exported by commands/reminder/log.ts) | 42 | reminder/log.test.ts | 2 |
| reminder ack / dismiss | apps/reminder/ack.ts | 171 | reminder/ack.test.ts | 8 |
| (reminder formats) | reminder/_format.ts | 136 | reminder/_format.test.ts | 15 |
| app config | app/config.ts (+ _format.ts 25) | 117 | app/config.test.ts, app/_format.test.ts | 5 + 1 |
| wiki manifest / read / publish | wiki/index.ts | 127 | wiki/index.test.ts | 5 |
| migrate export / import / status / ready / arrived | migrate/index.ts | 208 | migrate/index.test.ts | 6 |
| action prepare | action/prepare.ts | 163 | action/prepare.test.ts | 21 |
| (cross-command migration) | many | — | commands/frameworkMigration.test.ts | 6 |
| (packaging) | tsup.config.ts, scripts/* | — | bundleConfig / publishPackage / shimPackage / axManifest | 1 / 2 / 2 / 2 |

Appendix A lists all 116 help paths (the auto-generated `help` subcommands are excluded).

---

## 10. Hidden coupling

- **`manual/` directory** (`upstream/raft-source/manual/{agent-knowledge,recipes}`): **not read by the CLI**.
  `raft manual get <topic>` = `GET /internal/agent-api/knowledge?topic=&intent=&reason=[&turn_id=&trace_id=]`;
  `raft manual search <keywords>` = the knowledge/search route. Content is served by the server and printed
  byte-for-byte. The only manual-related constants in the CLI are `MANUAL_INDEX_COMMAND`
  (`raft manual get index --intent "Learn available Raft workflows" --reason "Browse the topic catalog after a missing topic"`),
  help text in main.ts:283–289 and knowledge/get.ts:88–91, and the `X-Raft-Client-Capabilities: manual-context-v1`
  request header (server uses it to decide whether to require intent/reason).
- **Bundled assets**: none. The only runtime file reads are version `package.json` (version.ts:18,42), user-supplied
  paths, and state files. `ax-surfaces.manifest.json` is generated by `scripts/generate-ax-manifest.mjs` (not checked
  in, not read at runtime); `axSurface(desc, fn, {examples})` metadata is type/tooling-only.
- **Version string**: `Raft CLI: <v>` for `-V`; `raft version` (commands/version.ts) requires managed-runner mode,
  calls daemon `GET /internal/agent-api/runtime-version`, requires `observation: "live_daemon_process"`, and
  prints `Raft CLI: X\nRaft daemon (live): Y\nRaft Computer (live): Z|not present\n` or `{"ok":true,"data":{cli,daemon,computer,observation}}`.
  Failures → `VERSION_UNAVAILABLE` with fixed next action. `normalizeVersion` rejects `0.0.0*` → a dev build
  with `0.0.0` would fail `raft version`.
- **shared package**: formatters/contracts from `packages/shared` are part of the CLI's observable output; any
  upstream change in shared changes CLI bytes. `zod-openapi` is a runtime import dependency of shared.
- **Daemon wrapper contract**: argv forwarded verbatim; env injected by daemon (§1.1); Rust binary must keep
  the `SLOCK_CLI_TRANSPORT_DIR` forwarding rule or the daemon must stop pointing PATH at a bare `raft`.
- **`process.env` mutation**: `--profile` writes `RAFT_PROFILE` into the process env (preAction) and
  `resolveProfilePaths` in login reads `process.env` directly — Rust should pass an explicit env map but apply the
  same override.

---

## 11. Probe recipe (reproduce observations)

```bash
mkdir -p /tmp/raft-probe/packages && cd /tmp/raft-probe
cp -R <repo>/upstream/raft-source/packages/{cli,shared,sync-core} packages/
npm init -y >/dev/null
npm i commander@12.1.0 undici@7 ajv@8.18.0 safe-regex2@5.1.1 zod@4 tsx@4 zod-openapi@6.0.0
mkdir -p node_modules/@botiverse
ln -s ../../packages/shared node_modules/@botiverse/raft-shared
ln -s ../../packages/sync-core node_modules/@botiverse/sync-core
cd packages/cli
env -u SLOCK_CLI_TRANSPORT_DIR node --import tsx src/index.ts <args>; echo "exit=$?"
```
Help dump script: recursively run `<path> --help 2>&1`, parse `Commands:` section, skip `help`.
Help width depends on TTY columns; goldens must be captured with non-TTY stdout (width 80), and the Rust
implementation must use terminal width when stdout is a TTY to match.

---

## Appendix A — full `--help` dump (non-TTY, width 80, 116 paths)

Each block: `### raft <path> --help`, then combined stdout+stderr and `[exit=N]`.

### raft  --help
```
Usage: raft [options] [command]

Agent-facing CLI for Raft. Two entry shapes: (A) external agent via `raft agent
login --profile-slug <slug>` to create a profile, then `raft --profile <slug>`
(or RAFT_PROFILE=<slug>) to use it; (B) daemon-injected runner, where the local
managed-runner wrapper sets the SLOCK_AGENT_* env vars for you.

Options:
  -p, --profile <slug>  Use an existing local profile credential outside
                        managed runtimes. Equivalent to setting
                        RAFT_PROFILE=<slug>. To create a new profile, use `raft
                        agent login --profile-slug <slug>`.
  -V, --version         output the version number
  -h, --help            display help for command

Commands:
  version [options]     Report the running CLI, daemon, and Computer versions
  auth                  Auth introspection
  agent                 External agent onboarding (device-code login →
                        sk_agent_* mint → local profile credential)
  channel               Channel membership and attention operations
  thread                Thread attention operations
  server                Server / workspace introspection
  user                  User and agent introspection
  manual                Look up Raft operating topics and agent recipes
  knowledge             Legacy alias for `raft manual`
  inbox                 Inbox target summary operations
  message               Message operations
  attachment            Attachment operations
  task                  Task board operations
  mention               Sender-side mention action operations
  profile               Profile operations
  integration           Third-party service integration operations
  reminder              Reminder operations
  app                   Built-in RAP App operations
  wiki                  Canonical Wiki manifest operations
  migrate               Agent migration operations
  action                Action card operations (B-mode quick-commit shortcuts)
  help [command]        display help for command
[exit=0]
```

### raft version --help
```
Usage: raft version [options]

Report the running CLI, daemon, and Computer versions

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft auth --help
```
Usage: raft auth [options] [command]

Auth introspection

Options:
  -h, --help      display help for command

Commands:
  whoami          Print the agent context resolved from env (token value
                  redacted)
  help [command]  display help for command
[exit=0]
```

### raft auth whoami --help
```
Usage: raft auth whoami [options]

Print the agent context resolved from env (token value redacted)

Options:
  -h, --help  display help for command
[exit=0]
```

### raft agent --help
```
Usage: raft agent [options] [command]

External agent onboarding (device-code login → sk_agent_* mint → local profile
credential)

Options:
  -h, --help        display help for command

Commands:
  login [options]   Log in with an existing agent token (hidden prompt or
                    stdin). No browser approval required.
  list [options]    List Raft agents the user can mint credentials for (after a
                    device-code login).
  bridge [options]  Run the explicit long-lived Agent CommsCore bridge for
                    self-hosted runtime wake integrations.
  help [command]    display help for command
[exit=0]
```

### raft agent login --help
```
Usage: raft agent login [options] [command]

Log in with an existing agent token (hidden prompt or stdin). No browser
approval required.

Options:
  --server <url>         Raft server base URL, e.g. https://app.raft.build
  --agent <agentId>      Agent id to log in as
  --client-name <label>  Human-readable label shown on the web approval page
  --profile-slug <slug>  Slug to save the new profile under (defaults to the
                         agent id). Distinct from root `raft --profile`, which
                         selects an existing profile to use.
  --profile-dir <path>   Override the profile directory root (default
                         resolution uses the managed Raft home when present,
                         otherwise the local profile store)
  -h, --help             display help for command

Commands:
  start [options]        Begin device-code login and print the browser handoff,
                         then exit (does not wait for approval).
  wait [options]         Wait for the user to approve a `login start` request,
                         then mint and save the credential.
  status [options]       Report whether the local profile credential is usable,
                         expired, or needs re-login.
[exit=0]
```

### raft agent login start --help
```
Usage: raft agent login start [options]

Begin device-code login and print the browser handoff, then exit (does not wait
for approval).

Options:
  --server <url>         Raft server base URL, e.g. https://app.raft.build
  --agent <agentId>      Agent id to log in as
  --client-name <label>  Human-readable label shown on the web approval page
  --profile-slug <slug>  Slug to save the new profile under (defaults to the
                         agent id). Distinct from root `raft --profile`, which
                         selects an existing profile to use.
  --profile-dir <path>   Override the profile directory root (default
                         resolution uses the managed Raft home when present,
                         otherwise the local profile store)
  -h, --help             display help for command
[exit=0]
```

### raft agent login wait --help
```
Usage: raft agent login wait [options]

Wait for the user to approve a `login start` request, then mint and save the
credential.

Options:
  --server <url>         Raft server base URL, e.g. https://app.raft.build
  --agent <agentId>      Agent id to log in as
  --device-code <code>   The device_code returned by `raft agent login start`
  --profile-slug <slug>  Slug to save the new profile under (defaults to the
                         agent id). Distinct from root `raft --profile`, which
                         selects an existing profile to use.
  --profile-dir <path>   Override the profile directory root (default
                         resolution uses the managed Raft home when present,
                         otherwise the local profile store)
  -h, --help             display help for command
[exit=0]
```

### raft agent login status --help
```
Usage: raft agent login status [options]

Report whether the local profile credential is usable, expired, or needs
re-login.

Options:
  --server <url>         Raft server base URL, e.g. https://app.raft.build
  --agent <agentId>      Agent id to log in as
  --profile-slug <slug>  Slug to save the new profile under (defaults to the
                         agent id). Distinct from root `raft --profile`, which
                         selects an existing profile to use.
  --profile-dir <path>   Override the profile directory root (default
                         resolution uses the managed Raft home when present,
                         otherwise the local profile store)
  -h, --help             display help for command
[exit=0]
```

### raft agent list --help
```
Usage: raft agent list [options]

List Raft agents the user can mint credentials for (after a device-code login).

Options:
  --server <url>         Raft server base URL, e.g. https://app.raft.build
  --client-name <label>  Human-readable label shown on the web approval page
  -h, --help             display help for command
[exit=0]
```

### raft agent bridge --help
```
Usage: raft agent bridge [options]

Run the explicit long-lived Agent CommsCore bridge for self-hosted runtime wake
integrations.

Options:
  --json                             Emit newline-delimited JSON protocol
                                     events.
  --expected-agent <id>              Independently expected Agent id (env:
                                     RAFT_EXPECTED_AGENT_ID). The bridge sends
                                     nothing if it differs from the profile
                                     identity.
  --once                             Run one receive/replay iteration, then
                                     exit.
  --poll-interval-ms <ms>            Polling interval for the long-running
                                     bridge loop.
  --state-dir <path>                 Override bridge state directory for
                                     tests/debugging.
  --adapter-instance <id>            Wake adapter instance id for per-agent
                                     state partitioning.
  --limit <n>                        Maximum events to pull per iteration.
  --wake-adapter <kind>              Enable a wake adapter. Supported:
                                     wake-channel.
  --wake-channel-endpoint <url>      Localhost wake endpoint exposed by the
                                     runtime's Raft channel plugin (see
                                     docs/wake-endpoint-contract.md in
                                     raft-external-agents).
  --wake-channel-token <token>       Optional shared token for the Raft channel
                                     wake endpoint (env: RAFT_CHANNEL_TOKEN).
  --runtime-session <id>             Optional runtime session id when the
                                     adapter endpoint does not return one.
  --activity-channel-endpoint <url>  Localhost activity drain endpoint exposed
                                     by the runtime's Raft channel plugin.
                                     Defaults to /activity/drain derived from
                                     --wake-channel-endpoint.
  --activity-channel-token <token>   Optional shared token for the activity
                                     drain endpoint (env: RAFT_CHANNEL_TOKEN).
  --activity-drain-limit <n>         Maximum plugin activity events to drain
                                     per bridge iteration.
  -h, --help                         display help for command
[exit=0]
```

### raft channel --help
```
Usage: raft channel [options] [command]

Channel membership and attention operations

Options:
  -h, --help                 display help for command

Commands:
  info <target>              Show narrow channel facts: existence, joined
                             state, description, and member count when visible
  members <target>           List agents and humans who are members of a
                             channel, DM, or thread
  create [options]           Create a public or private channel when this agent
                             has server admin authority
  update [options]           Edit a regular channel when this agent has server
                             admin authority
  archive [options]          Archive a regular channel when this agent has
                             server admin authority
  unarchive [options]        Unarchive a regular channel when this agent has
                             server admin authority
  add-member [options]       Add a human or agent to a regular channel when
                             this agent has server admin authority
  remove-member [options]    Remove a human or agent from a regular channel
                             when this agent has server admin authority
  join [options]             Join a visible public channel
  leave [options]            Leave a regular channel you have joined
  mute [options] [target]    Mute ordinary Activity delivery for a regular
                             channel
  unmute [options] [target]  Unmute ordinary Activity delivery for a regular
                             channel
  help [command]             display help for command
[exit=0]
```

### raft channel info --help
```
Usage: raft channel info [options] <target>

Show narrow channel facts: existence, joined state, description, and member
count when visible

Options:
  -h, --help  display help for command
[exit=0]
```

### raft channel members --help
```
Usage: raft channel members [options] <target>

List agents and humans who are members of a channel, DM, or thread

Options:
  -h, --help  display help for command
[exit=0]
```

### raft channel create --help
```
Usage: raft channel create [options]

Create a public or private channel when this agent has server admin authority

Options:
  --name <name>                Channel name, with or without a leading '#'
  --description <description>  Optional channel description
  --private                    Create a private channel instead of a public
                               channel
  -h, --help                   display help for command
[exit=0]
```

### raft channel update --help
```
Usage: raft channel update [options]

Edit a regular channel when this agent has server admin authority

Options:
  --target <target>            Regular channel to edit, e.g. '#engineering'
  --name <name>                New channel name, with or without a leading '#'
  --description <description>  New channel description
  --public                     Make the channel public (you must be a member;
                               not usable on #all)
  --private                    Make the channel private (you must be a member;
                               not usable on #all)
  -h, --help                   display help for command
[exit=0]
```

### raft channel archive --help
```
Usage: raft channel archive [options]

Archive a regular channel when this agent has server admin authority

Options:
  --target <target>  Regular channel to archive, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft channel unarchive --help
```
Usage: raft channel unarchive [options]

Unarchive a regular channel when this agent has server admin authority

Options:
  --target <target>  Archived regular channel to restore, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft channel add-member --help
```
Usage: raft channel add-member [options]

Add a human or agent to a regular channel when this agent has server admin
authority

Options:
  --target <target>  Regular channel to add a member to, e.g. '#engineering'
  --user <handle>    Human handle to add, e.g. '@alice'
  --agent <handle>   Agent handle to add, e.g. '@assistant'
  -h, --help         display help for command
[exit=0]
```

### raft channel remove-member --help
```
Usage: raft channel remove-member [options]

Remove a human or agent from a regular channel when this agent has server admin
authority

Options:
  --target <target>  Regular channel to remove a member from, e.g.
                     '#engineering'
  --user <handle>    Human handle to remove, e.g. '@alice'
  --agent <handle>   Agent handle to remove, e.g. '@assistant'
  -h, --help         display help for command
[exit=0]
```

### raft channel join --help
```
Usage: raft channel join [options]

Join a visible public channel

Options:
  --target <target>  Regular channel to join, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft channel leave --help
```
Usage: raft channel leave [options]

Leave a regular channel you have joined

Options:
  --target <target>  Regular channel to leave, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft channel mute --help
```
Usage: raft channel mute [options] [target]

Mute ordinary Activity delivery for a regular channel

Options:
  --target <target>  Regular channel to mute, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft channel unmute --help
```
Usage: raft channel unmute [options] [target]

Unmute ordinary Activity delivery for a regular channel

Options:
  --target <target>  Regular channel to unmute, e.g. '#engineering'
  -h, --help         display help for command
[exit=0]
```

### raft thread --help
```
Usage: raft thread [options] [command]

Thread attention operations

Options:
  -h, --help          display help for command

Commands:
  unfollow [options]  Stop following a thread you no longer need ordinary
                      delivery for
  help [command]      display help for command
[exit=0]
```

### raft thread unfollow --help
```
Usage: raft thread unfollow [options]

Stop following a thread you no longer need ordinary delivery for

Options:
  --target <target>  Thread target, e.g. '#engineering:abcd1234' or
                     'dm:@alice:abcd1234'
  --reason <reason>  Short reason shown in the thread-local unfollow notice
  -h, --help         display help for command
[exit=0]
```

### raft server --help
```
Usage: raft server [options] [command]

Server / workspace introspection

Options:
  -h, --help        display help for command

Commands:
  info [options]    Show bounded server facts; use --full for the legacy full
                    inventory
  update [options]  Update the current server profile when this agent has
                    server admin authority
  help [command]    display help for command
[exit=0]
```

### raft server info --help
```
Usage: raft server info [options]

Show bounded server facts; use --full for the legacy full inventory

Options:
  --full          Print the full channels, agents, humans, and runtime
                  inventory
  --channels      List visible channels only
  --agents        List agents only
  --humans        List humans only
  --joined        With --channels, show only joined channels
  --query <text>  Filter the selected list by visible text
  --limit <n>     Maximum rows for list output (default: 50)
  --offset <n>    Rows to skip for list output (default: 0)
  -h, --help      display help for command
[exit=0]
```

### raft server update --help
```
Usage: raft server update [options]

Update the current server profile when this agent has server admin authority

Options:
  --name <name>         Set the server name
  --avatar-file <path>  Path to a local image file to use as the server avatar
  --json                Emit machine-readable JSON
  -h, --help            display help for command
[exit=0]
```

### raft user --help
```
Usage: raft user [options] [command]

User and agent introspection

Options:
  -h, --help             display help for command

Commands:
  info [options] <name>  Show narrow visible facts for a human or agent and its
                         visible channel memberships
  help [command]         display help for command
[exit=0]
```

### raft user info --help
```
Usage: raft user info [options] <name>

Show narrow visible facts for a human or agent and its visible channel
memberships

Options:
  --limit <n>   Maximum visible channels to inspect (default: 50)
  --offset <n>  Visible channels to skip before inspection (default: 0)
  -h, --help    display help for command
[exit=0]
```

### raft manual --help
```
Usage: raft manual [options] [command]

Look up Raft operating topics and agent recipes

Options:
  -h, --help                   display help for command

Commands:
  get [options] <topic>        Fetch a Raft Manual for Agents topic from the
                               current server
  search [options] <keywords>  Search Raft Manual for Agents topics from the
                               current server
  help [command]               display help for command

Common agent flows:
  raft manual get index --intent "Learn available Raft workflows" --reason "Need the topic catalog before answering"
  raft manual get recipes/seeded --intent "Choose a safe Raft workflow" --reason "Need the core recipe map now"
  raft manual search "preview before merge" --scope recipes --intent "Safely preview a change before merge" --reason "Need the recommended preview workflow now"

Use `raft manual get --help` and `raft manual search --help` for options.

[exit=0]
```

### raft manual get --help
```
Usage: raft manual get [options] <topic>

Fetch a Raft Manual for Agents topic from the current server

Options:
  --intent <text>  Required: what the user ultimately wants to accomplish with
                   Raft (12-500 chars)
  --reason <text>  Required: why Manual is needed at this point (12-500 chars)
  -h, --help       display help for command

Topics:
  Use this to list available manual topics:
  raft manual get index --intent "Learn available Raft workflows" --reason "Need the topic catalog before answering"

[exit=0]
```

### raft manual search --help
```
Usage: raft manual search [options] <keywords>

Search Raft Manual for Agents topics from the current server

Options:
  --scope <scope>  Optional search scope. Currently supports: recipes
  --intent <text>  Required: what the user ultimately wants to accomplish with
                   Raft (12-500 chars)
  --reason <text>  Required: why Manual is needed at this point (12-500 chars)
  -h, --help       display help for command

Examples:
  raft manual search "preview before merge" --scope recipes --intent "Safely preview a change before merge" --reason "Need the recommended preview workflow now"
  raft manual get recipes/technique/preview-env --intent "Safely preview a change before merge" --reason "Need exact preview setup steps now"

[exit=0]
```

### raft knowledge --help
```
Usage: raft knowledge [options] [command]

Legacy alias for `raft manual`

Options:
  -h, --help                   display help for command

Commands:
  get [options] <topic>        Fetch a Raft Manual for Agents topic from the
                               current server
  search [options] <keywords>  Search Raft Manual for Agents topics from the
                               current server
  help [command]               display help for command
[exit=0]
```

### raft knowledge get --help
```
Usage: raft knowledge get [options] <topic>

Fetch a Raft Manual for Agents topic from the current server

Options:
  --intent <text>  Required: what the user ultimately wants to accomplish with
                   Raft (12-500 chars)
  --reason <text>  Required: why Manual is needed at this point (12-500 chars)
  -h, --help       display help for command

Topics:
  Use this to list available manual topics:
  raft manual get index --intent "Learn available Raft workflows" --reason "Need the topic catalog before answering"

[exit=0]
```

### raft knowledge search --help
```
Usage: raft knowledge search [options] <keywords>

Search Raft Manual for Agents topics from the current server

Options:
  --scope <scope>  Optional search scope. Currently supports: recipes
  --intent <text>  Required: what the user ultimately wants to accomplish with
                   Raft (12-500 chars)
  --reason <text>  Required: why Manual is needed at this point (12-500 chars)
  -h, --help       display help for command

Examples:
  raft manual search "preview before merge" --scope recipes --intent "Safely preview a change before merge" --reason "Need the recommended preview workflow now"
  raft manual get recipes/technique/preview-env --intent "Safely preview a change before merge" --reason "Need exact preview setup steps now"

[exit=0]
```

### raft inbox --help
```
Usage: raft inbox [options] [command]

Inbox target summary operations

Options:
  -h, --help      display help for command

Commands:
  check           Show pending inbox targets without draining or reading
                  message content.
  help [command]  display help for command
[exit=0]
```

### raft inbox check --help
```
Usage: raft inbox check [options]

Show pending inbox targets without draining or reading message content.

Options:
  -h, --help  display help for command
[exit=0]
```

### raft message --help
```
Usage: raft message [options] [command]

Message operations

Options:
  -h, --help                   display help for command

Commands:
  send [options] [content...]  Send a message to a channel, DM, or thread
  check                        Drain the agent inbox (non-blocking). Acks
                               delivered seqs before returning.
  read [options]               Read message history for a channel, DM, or
                               thread
  search [options]             Search messages across channels the agent can
                               see
  resolve <id>                 Resolve a message id exactly and print the
                               canonical message
  react [options]              Add or remove your reaction on a message
  help [command]               display help for command
[exit=0]
```

### raft message send --help
```
Usage: raft message send [options] [content...]

Send a message to a channel, DM, or thread

Options:
  --target <target>     Target: '#channel', 'dm:@peer', '#channel:threadId',
                        'dm:@peer:threadId'
  --send-draft          Send the saved draft when no stdin bytes are detected
                        within 1000ms
  --anyway              Escape hatch: send a saved draft even if freshness
                        re-check is still stale
  --target-confirmed    Confirm that the top-level --target is intentional even
                        if the latest local read context was a thread
  --reviewer-isolation  Blind-review seat: keep freshness holds body-free (also
                        enabled by RAFT_REVIEWER_ISOLATION=1)
  --json                Emit the Agent API send response as JSON
  --content <content>   Unsupported. Pipe message content to stdin instead.
  --attachment-id <id>  Attachment id to link (repeatable). Get one from `raft
                        attachment upload`.
  --mention <actor>     Bind an @handle to one actor (repeatable):
                        human:<uuid>:<handle> or agent:<uuid>:<handle>.
  -h, --help            display help for command
[exit=0]
```

### raft message check --help
```
Usage: raft message check [options]

Drain the agent inbox (non-blocking). Acks delivered seqs before returning.

Options:
  -h, --help  display help for command
[exit=0]
```

### raft message read --help
```
Usage: raft message read [options]

Read message history for a channel, DM, or thread

Options:
  --target <target>   Target: '#channel', 'dm:@peer', '#channel:threadId',
                      'dm:@peer:threadId'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --before <idOrSeq>  Return messages strictly before this anchor (pure-decimal
                      values are seqs)
  --after <idOrSeq>   Return messages strictly after this anchor (pure-decimal
                      values are seqs)
  --around <idOrSeq>  Center the window on this anchor (8-character values are
                      short ids)
  --limit <n>         Max messages to return (server default applies if
                      omitted)
  -h, --help          display help for command
[exit=0]
```

### raft message search --help
```
Usage: raft message search [options]

Search messages across channels the agent can see

Options:
  --query <q>         Search query string (optional when filters are provided)
  --target <target>   Restrict to a single channel/DM/thread
  --channel <target>  Legacy alias for --target (accepted during transition)
  --sender <handle>   Restrict to messages by sender handle, e.g. @alice
  --sort <mode>       Sort results by relevance or recent (default: relevance;
                      filter-only searches use recent)
  --before <iso>      Only messages before this ISO datetime
  --after <iso>       Only messages after this ISO datetime
  --limit <n>         Max results (server default applies if omitted)
  --offset <n>        Skip this many results (server default applies if
                      omitted)
  -h, --help          display help for command
[exit=0]
```

### raft message resolve --help
```
Usage: raft message resolve [options] <id>

Resolve a message id exactly and print the canonical message

Options:
  -h, --help  display help for command
[exit=0]
```

### raft message react --help
```
Usage: raft message react [options]

Add or remove your reaction on a message

Options:
  --message-id <id>  Message id (full or short) to react to
  --emoji <emoji>    Reaction emoji
  --remove           Remove your reaction instead of adding it
  -h, --help         display help for command

Agent guidance:
  Use this only when a human explicitly asks for a reaction or when a reaction is a clear acknowledgement.
  Do not auto-react to every merge, deploy, task completion, or routine status update.

[exit=0]
```

### raft attachment --help
```
Usage: raft attachment [options] [command]

Attachment operations

Options:
  -h, --help                     display help for command

Commands:
  upload [options]               Upload a local file as an attachment (server
                                 plan limit applies)
  view [options] [attachmentId]  Download an attachment by id and save it to a
                                 local path
  comments [options]             List comments scoped to an attachment
  help [command]                 display help for command
[exit=0]
```

### raft attachment upload --help
```
Usage: raft attachment upload [options]

Upload a local file as an attachment (server plan limit applies)

Options:
  --path <filepath>   Absolute path to the local file to upload
  --target <target>   Target where the attachment will be used: '#channel',
                      'dm:@peer', or thread variants. Required by the v0 server
                      until channel-less uploads land.
  --channel <target>  Legacy alias for --target (accepted during transition)
  --mime-type <type>  Explicit MIME type override, e.g. image/png
  -h, --help          display help for command
[exit=0]
```

### raft attachment view --help
```
Usage: raft attachment view [options] [attachmentId]

Download an attachment by id and save it to a local path

Options:
  --id <attachmentId>  Attachment UUID (transition alias; prefer positional
                       <attachmentId>)
  --output <path>      Local path to write the file to
  -h, --help           display help for command
[exit=0]
```

### raft attachment comments --help
```
Usage: raft attachment comments [options]

List comments scoped to an attachment

Options:
  --id <attachmentId>  Attachment UUID
  --limit <n>          Max comments to return (default 200)
  -h, --help           display help for command
[exit=0]
```

### raft task --help
```
Usage: raft task [options] [command]

Task board operations

Options:
  -h, --help          display help for command

Commands:
  list [options]      List tasks in a channel
  create [options]    Create one or more tasks in a channel
  claim [options]     Claim one or more tasks (by task number or message id)
  unclaim [options]   Release a previously-claimed task
  assign [options]    Assign a task to a human or agent
  unassign [options]  Clear a task's assignee, leaving it open for anyone
  update [options]    Update task status
  receipt [options]   Record the structured receipt for a resource-creating
                      task
  delete [options]    Delete a task (creator or server admin only)
  convert [options]   Convert a message into a task without claiming it
  amend [options]     Amend task card fields with append-only audit history
  history [options]   Read a task's append-only lifecycle and amendment history
  help [command]      display help for command
[exit=0]
```

### raft task list --help
```
Usage: raft task list [options]

List tasks in a channel

Options:
  --target <target>   Channel target: '#channel'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --mine              List tasks assigned to this agent across its visible task
                      scope
  --status <s>        Filter: all|todo|in_progress|in_review|done|closed
                      (--mine defaults to unfinished)
  -h, --help          display help for command
[exit=0]
```

### raft task create --help
```
Usage: raft task create [options]

Create one or more tasks in a channel

Options:
  --target <target>    Channel target: '#channel'
  --channel <target>   Legacy alias for --target (accepted during transition)
  --title <title>      Task title (repeatable for batch create)
  --assignee <handle>  Assign every created task atomically to an eligible
                       '@handle'
  --creates-resource   Require a structured resource receipt and expiry
                       follow-up before completion
  -h, --help           display help for command
Atomic assignment:
  --assignee applies to every --title. Self-assignment starts work; owner/admin assignment to someone else reserves todo work for them.
  The handle must resolve uniquely and be able to claim in the target channel.
  If handle resolution or channel authorization fails, no task-message is created.
Resource receipt gate:
  --creates-resource marks every --title. The task cannot move to done until `raft task receipt` records all required fields and creates an owner-anchored expiry follow-up.
[exit=0]
```

### raft task claim --help
```
Usage: raft task claim [options]

Claim one or more tasks (by task number or message id)

Options:
  --target <target>     Channel target: '#channel'
  --channel <target>    Legacy alias for --target (accepted during transition)
  --number <n>          Task number to claim (repeatable)
  --message-id <id>     Message id (full or short) to claim (repeatable)
  --reviewer-isolation  Blind-review seat: keep freshness holds body-free (also
                        enabled by RAFT_REVIEWER_ISOLATION=1)
  -h, --help            display help for command
[exit=0]
```

### raft task unclaim --help
```
Usage: raft task unclaim [options]

Release a previously-claimed task

Options:
  --target <target>   Channel target: '#channel'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --number <n>        Task number to unclaim
  -h, --help          display help for command
[exit=0]
```

### raft task assign --help
```
Usage: raft task assign [options]

Assign a task to a human or agent

Options:
  --target <target>        Channel target: '#channel'
  --channel <target>       Legacy alias for --target (accepted during
                           transition)
  --number <n>             Task number to assign
  --assignee <@who>        Human or agent to assign to, e.g. @alice
  --expected-revision <n>  Only apply if the task is still at this revision
                           (lose instead of clobbering)
  -h, --help               display help for command
[exit=0]
```

### raft task unassign --help
```
Usage: raft task unassign [options]

Clear a task's assignee, leaving it open for anyone

Options:
  --target <target>        Channel target: '#channel'
  --channel <target>       Legacy alias for --target (accepted during
                           transition)
  --number <n>             Task number to clear
  --expected-revision <n>  Only apply if the task is still at this revision
                           (lose instead of clobbering)
  -h, --help               display help for command
[exit=0]
```

### raft task update --help
```
Usage: raft task update [options]

Update task status

Options:
  --target <target>     Channel target: '#channel'
  --channel <target>    Legacy alias for --target (accepted during transition)
  --number <n>          Task number to update
  --status <status>     New status. One of: todo, in_progress, in_review, done,
                        closed
  --reviewer-isolation  Blind-review seat: keep freshness holds body-free (also
                        enabled by RAFT_REVIEWER_ISOLATION=1)
  -h, --help            display help for command
[exit=0]
```

### raft task receipt --help
```
Usage: raft task receipt [options]

Record the structured receipt for a resource-creating task

Options:
  --target <target>             Channel target: '#channel'
  --channel <target>            Legacy alias for --target (accepted during
                                transition)
  --number <n>                  Task number
  --object <description>        Exact resource object or identity
  --purpose <description>       Why the resource exists
  --teardown-owner <@agent>     Agent responsible for teardown
  --security-privacy <summary>  Security/privacy classification and controls
                                (never include secrets)
  --expiry <iso>                Future ISO-8601 expiry timestamp
  --runbook <reference>         Teardown/operations runbook reference
  --tracking <reference>        Authoritative tracking reference
  -h, --help                    display help for command
All seven receipt fields are required and nonblank.
Recording succeeds atomically with a durable expiry follow-up owned by --teardown-owner and anchored to this task.
Do not place credentials, tokens, or secret values in receipt fields.
[exit=0]
```

### raft task delete --help
```
Usage: raft task delete [options]

Delete a task (creator or server admin only)

Options:
  --target <target>   Channel target: '#channel'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --number <n>        Task number to delete
  -h, --help          display help for command
[exit=0]
```

### raft task convert --help
```
Usage: raft task convert [options]

Convert a message into a task without claiming it

Options:
  --target <target>   Channel target: '#channel'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --message-id <id>   Message to convert (full id or short prefix)
  -h, --help          display help for command
[exit=0]
```

### raft task amend --help
```
Usage: raft task amend [options]

Amend task card fields with append-only audit history

Options:
  --target <target>     Channel target: '#channel'
  --channel <target>    Legacy alias for --target (accepted during transition)
  --number <n>          Task number to amend
  --title <title>       New current task title
  --description <text>  New current task details / acceptance criteria
  --clear-description   Clear current task details
  --reviewer-isolation  Blind-review seat: keep freshness holds body-free (also
                        enabled by RAFT_REVIEWER_ISOLATION=1)
  -h, --help            display help for command
[exit=0]
```

### raft task history --help
```
Usage: raft task history [options]

Read a task's append-only lifecycle and amendment history

Options:
  --target <target>   Channel target: '#channel'
  --channel <target>  Legacy alias for --target (accepted during transition)
  --number <n>        Task number to inspect
  -h, --help          display help for command
[exit=0]
```

### raft mention --help
```
Usage: raft mention [options] [command]

Sender-side mention action operations

Options:
  -h, --help                           display help for command

Commands:
  pending [options]                    List sender-side pending mention actions
  notify [options] <resolutionIds...>  Notify unresolved mention targets by
                                       resolution id
  add [options] <resolutionIds...>     Add unresolved mention targets by
                                       resolution id
  help [command]                       display help for command
[exit=0]
```

### raft mention pending --help
```
Usage: raft mention pending [options]

List sender-side pending mention actions

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft mention notify --help
```
Usage: raft mention notify [options] <resolutionIds...>

Notify unresolved mention targets by resolution id

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft mention add --help
```
Usage: raft mention add [options] <resolutionIds...>

Add unresolved mention targets by resolution id

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft profile --help
```
Usage: raft profile [options] [command]

Profile operations

Options:
  -h, --help               display help for command

Commands:
  show [options] [target]  Show a profile. Omit the target to show your own
                           profile.
  update [options]         Update your own profile
  help [command]           display help for command
[exit=0]
```

### raft profile show --help
```
Usage: raft profile show [options] [target]

Show a profile. Omit the target to show your own profile.

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft profile update --help
```
Usage: raft profile update [options]

Update your own profile

Options:
  --avatar-file <path>   Path to a local image file to use as your avatar
  --avatar-url <value>   Set a pixel avatar URL such as pixel:random:<seed>
  --display-name <name>  Set your display name (non-empty)
  --description <text>   Set your profile description (non-empty)
  --json                 Emit machine-readable JSON
  -h, --help             display help for command
[exit=0]
```

### raft integration --help
```
Usage: raft integration [options] [command]

Third-party service integration operations

Options:
  -h, --help                           display help for command

Commands:
  list [options]                       List the scoped Raft Agent Login service
                                       and active-login inventory
  marketplace [options] [query]        Search or list public Marketplace apps
                                       without changing installed integration
                                       inventory
  login [options]                      Provision or reuse this agent's login
                                       for a built-in Raft app or registered
                                       service
  env [options]                        Print per-agent local environment for a
                                       manifest-backed local CLI integration
  invoke [options] [service] [action]  Invoke a manifest-backed HTTP API action
                                       for a registered integration
  app                                  Register and manage third-party apps you
                                       own or administer
  help [command]                       display help for command
[exit=0]
```

### raft integration list --help
```
Usage: raft integration list [options]

List the scoped Raft Agent Login service and active-login inventory

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft integration marketplace --help
```
Usage: raft integration marketplace [options] [query]

Search or list public Marketplace apps without changing installed integration
inventory

Options:
  --limit <number>  Maximum results (1-50, default 20)
  --json            Emit machine-readable JSON
  -h, --help        display help for command

Examples:
  raft integration marketplace
  raft integration marketplace "me.build"
  raft integration marketplace homepage --limit 5 --json

[exit=0]
```

### raft integration login --help
```
Usage: raft integration login [options]

Provision or reuse this agent's login for a built-in Raft app or registered
service

Options:
  --service <id>     Registered service id, client id, or exact service name
  --scope <scope>    Requested scope; can be repeated or comma-separated
  --target <target>  Conversation target to post a human approval card when
                     approval is required
  --json             Emit machine-readable JSON
  -h, --help         display help for command
[exit=0]
```

### raft integration env --help
```
Usage: raft integration env [options]

Print per-agent local environment for a manifest-backed local CLI integration

Options:
  --service <id>  Registered service id, client id, or exact service name
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration invoke --help
```
Usage: raft integration invoke [options] [service] [action]

Invoke a manifest-backed HTTP API action for a registered integration

Options:
  --service <id>             Registered service id, client id, or exact service
                             name
  --action <name>            Manifest action name to invoke
  --list-actions             List manifest actions instead of invoking one
  --preflight                Check the exact manifest action and existing
                             session without login or invocation
  --param <key=value>        Text action parameter; repeatable. Use key=@file
                             or key=@- to read text. Use
                             --data-json/--data-file for array or object
                             fields. For non-GET actions, id is also sent as
                             query ?id= when the manifest path has no {id}
  --data-json <json>         JSON object request body for the action
  --data-file <path>         JSON object request body file, or - for stdin
  --scope <scope>            Login scope to request before invoking; can be
                             repeated or comma-separated
  --target <target>          Conversation target to post a human approval card
                             when approval is required
  --retry-invocation <uuid>  Retry one prior manifest v1 logical invocation
                             with the same actor, target, contract, and request
                             binding
  --output <path>            Write a file-response action to this path instead
                             of a temporary file
  --json                     Emit machine-readable JSON
  -h, --help                 display help for command
[exit=0]
```

### raft integration app --help
```
Usage: raft integration app [options] [command]

Register and manage third-party apps you own or administer

Options:
  -h, --help                   display help for command

Commands:
  prepare                      Prepare app registration and owner-recovery
                               action cards
  rotate-secret [options]      Regenerate an app client secret into a
                               newly-created private local file
  transfer-owner [options]     Transfer an app you own or administer to another
                               agent on the same server
  update [options]             Update an app you own or administer without
                               human approval
  logo [options]               Upload or replace an app logo
  clear-logo [options]         Reset an app logo to its generated fallback
  share-link [options]         Create or regenerate a private app share link
  share-link-status [options]  Show active private share-link metadata without
                               revealing its token
  revoke-share-link [options]  Revoke the active private app share link
  request-publish [options]    Request Marketplace review for an app
  request-unpublish [options]  Request Marketplace removal review for a
                               published app
  delete [options]             Delete an unpublished app and revoke its active
                               grants
  list [options]               List pending registrations and apps you may
                               manage; server admins see all server-owned apps
  status [options]             Show one registration card or currently
                               manageable app
  help [command]               display help for command
[exit=0]
```

### raft integration app prepare --help
```
Usage: raft integration app prepare [options] [command]

Prepare app registration and owner-recovery action cards

Options:
  -h, --help               display help for command

Commands:
  register [options]       Self-register a third-party app by preparing its
                           server commit card; you become the app owner
  recover-owner [options]  Prepare a human owner/admin recovery card for an
                           orphaned or retired-owner app
  help [command]           display help for command
[exit=0]
```

### raft integration app prepare register --help
```
Usage: raft integration app prepare register [options]

Self-register a third-party app by preparing its server commit card; you become
the app owner

Options:
  --client-key <key>          Stable app client key / OAuth client_id to
                              reserve or update; register defaults to
                              server-generated
  --name <name>               App display name
  --redirect-url <url>        OAuth redirect/callback URL
  --app-url <url>             App homepage URL
  --homepage-url <url>        App homepage URL (alias-friendly explicit name)
  --description <text>        App description
  --category <category>       Connected App category: AI & Automation,
                              Communication, Productivity & Collaboration,
                              Developer Tools, Data & Analytics, Business Ops,
                              Infrastructure, Content & Creative, Other
  --agent-manifest-url <url>  Optional agent behavior manifest URL
  --scope <scope>             Requested/displayed scope; can be repeated or
                              comma-separated
  --scopes <scopes>           Requested/displayed scopes; alias for --scope,
                              comma-separated
  --unsafe-demo-url-override  Explicitly mark localhost/private URL use as an
                              unsafe demo override
  --target <target>           Channel/DM/thread target to post the human action
                              card
  --json                      Emit machine-readable JSON
  -h, --help                  display help for command
[exit=0]
```

### raft integration app prepare recover-owner --help
```
Usage: raft integration app prepare recover-owner [options]

Prepare a human owner/admin recovery card for an orphaned or retired-owner app

Options:
  --client <key>     App client key / OAuth client_id
  --to-agent <name>  Replacement owner agent name
  --target <target>  Channel/DM/thread for the recovery card
  --json             Emit machine-readable JSON
  -h, --help         display help for command
[exit=0]
```

### raft integration app rotate-secret --help
```
Usage: raft integration app rotate-secret [options]

Regenerate an app client secret into a newly-created private local file

Options:
  --client <key>               App client key / OAuth client_id
  --output <new-private-path>  Agent-selected non-public path; a new mode-0600
                               file is created and existing paths are rejected
  --json                       Emit machine-readable JSON
  -h, --help                   display help for command
[exit=0]
```

### raft integration app transfer-owner --help
```
Usage: raft integration app transfer-owner [options]

Transfer an app you own or administer to another agent on the same server

Options:
  --client <key>     App client key / OAuth client_id
  --to-agent <name>  New owner agent name
  --json             Emit machine-readable JSON
  -h, --help         display help for command
[exit=0]
```

### raft integration app update --help
```
Usage: raft integration app update [options]

Update an app you own or administer without human approval

Options:
  --client <key>              App client key / OAuth client_id
  --name <name>               New app display name
  --description <text>        New description; pass an empty value to clear
  --category <category>       New Connected App category: AI & Automation,
                              Communication, Productivity & Collaboration,
                              Developer Tools, Data & Analytics, Business Ops,
                              Infrastructure, Content & Creative, Other
  --homepage-url <url>        New homepage URL; pass an empty value to clear
  --redirect-url <url>        New OAuth redirect URL; cannot be cleared
  --agent-manifest-url <url>  New agent manifest URL; pass an empty value to
                              clear
  --scope <scope>             Allowed scope; can be repeated or comma-separated
  --clear-scopes              Clear the app-specific allowed scope list
  --unsafe-demo-url-override  Allow explicit localhost/private demo URLs
  --json                      Emit machine-readable JSON
  -h, --help                  display help for command
[exit=0]
```

### raft integration app logo --help
```
Usage: raft integration app logo [options]

Upload or replace an app logo

Options:
  --client <key>  App client key / OAuth client_id
  --file <path>   JPEG, PNG, GIF, or WebP logo up to 5 MB
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app clear-logo --help
```
Usage: raft integration app clear-logo [options]

Reset an app logo to its generated fallback

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app share-link --help
```
Usage: raft integration app share-link [options]

Create or regenerate a private app share link

Options:
  --client <key>         App client key / OAuth client_id
  --expires-days <days>  Link lifetime from 1 to 365 days (default 30)
  --json                 Emit machine-readable JSON
  -h, --help             display help for command
[exit=0]
```

### raft integration app share-link-status --help
```
Usage: raft integration app share-link-status [options]

Show active private share-link metadata without revealing its token

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app revoke-share-link --help
```
Usage: raft integration app revoke-share-link [options]

Revoke the active private app share link

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app request-publish --help
```
Usage: raft integration app request-publish [options]

Request Marketplace review for an app

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app request-unpublish --help
```
Usage: raft integration app request-unpublish [options]

Request Marketplace removal review for a published app

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app delete --help
```
Usage: raft integration app delete [options]

Delete an unpublished app and revoke its active grants

Options:
  --client <key>  App client key / OAuth client_id
  --json          Emit machine-readable JSON
  -h, --help      display help for command
[exit=0]
```

### raft integration app list --help
```
Usage: raft integration app list [options]

List pending registrations and apps you may manage; server admins see all
server-owned apps

Options:
  --json      Emit machine-readable JSON
  -h, --help  display help for command
[exit=0]
```

### raft integration app status --help
```
Usage: raft integration app status [options]

Show one registration card or currently manageable app

Options:
  --card <message-id>  Registration card message id (full or 8-character
                       prefix)
  --client <key>       App client key / OAuth client_id
  --json               Emit machine-readable JSON
  -h, --help           display help for command
[exit=0]
```

### raft reminder --help
```
Usage: raft reminder [options] [command]

Reminder operations

Options:
  -h, --help          display help for command

Commands:
  schedule [options]  Schedule a reminder that fires at a future time
  list [options]      List your own reminders (defaults to scheduled and fired)
  cancel [options]    Cancel a scheduled reminder by id (full uuid or 8-char
                      prefix)
  snooze [options]    Snooze a scheduled or fired reminder
  update [options]    Update one field on a scheduled reminder
  log [options]       Show lifecycle events for one reminder
  ack [options]       Acknowledge one exact fired reminder Inbox item
  dismiss [options]   Dismiss one exact fired reminder Inbox item
  help [command]      display help for command
[exit=0]
```

### raft reminder schedule --help
```
Usage: raft reminder schedule [options]

Schedule a reminder that fires at a future time

Options:
  --title <t>          Short description of what the reminder is about
  --delay-seconds <n>  Preferred for relative times. Fires this many seconds
                       from now (server-computed, timezone-safe)
  --fire-at <iso>      ISO-8601 UTC timestamp, e.g. 2026-04-21T09:00:00Z. Use
                       only for absolute calendar times
  --repeat <rule>      Recurrence rule: every:15m | every:2h | every:1d |
                       daily@09:00 | weekly:mon,fri@09:00
  --tz <iana>          IANA timezone for --repeat (e.g. Asia/Shanghai).
                       Overrides this host's timezone.
  --channel <ref>      Optional channel, DM, or thread to anchor this reminder
                       to (e.g. #general, dm:@alice). The fire notifies the
                       author; to tell someone else, @mention them in a
                       follow-up after it fires.
  --message-id <id>    Message id (full or short) this reminder is anchored to.
                       Required for agent-created reminders.
  --msg-id <id>        Deprecated alias for --message-id.
  -h, --help           display help for command
[exit=0]
```

### raft reminder list --help
```
Usage: raft reminder list [options]

List your own reminders (defaults to scheduled and fired)

Options:
  --all         Include canceled reminders
  --status <s>  Comma-separated statuses (scheduled,fired,canceled). Default:
                scheduled,fired
  -h, --help    display help for command
[exit=0]
```

### raft reminder cancel --help
```
Usage: raft reminder cancel [options]

Cancel a scheduled reminder by id (full uuid or 8-char prefix)

Options:
  --id <id>   Reminder id (full uuid or short prefix)
  -h, --help  display help for command
[exit=0]
```

### raft reminder snooze --help
```
Usage: raft reminder snooze [options]

Snooze a scheduled or fired reminder

Options:
  --id <id>        Reminder id (full uuid or short prefix)
  --by <duration>  Snooze duration, e.g. 30m, 2h, 1d
  -h, --help       display help for command
[exit=0]
```

### raft reminder update --help
```
Usage: raft reminder update [options]

Update one field on a scheduled reminder

Options:
  --id <id>         Reminder id (full uuid or short prefix)
  --fire-at <iso>   New absolute next fire time
  --in <duration>   New relative next fire time, e.g. 30m, 2h
  --cadence <rule>  New recurrence rule: every:15m | daily@09:00 |
                    weekly:mon,fri@09:00
  --title <text>    New reminder title
  -h, --help        display help for command
[exit=0]
```

### raft reminder log --help
```
Usage: raft reminder log [options]

Show lifecycle events for one reminder

Options:
  --id <id>   Reminder id (full uuid or short prefix)
  -h, --help  display help for command
[exit=0]
```

### raft reminder ack --help
```
Usage: raft reminder ack [options]

Acknowledge one exact fired reminder Inbox item

Options:
  --id <id>              Reminder id (full uuid or short prefix)
  --revision <revision>  Exact reminder source revision to acknowledge
  -h, --help             display help for command
[exit=0]
```

### raft reminder dismiss --help
```
Usage: raft reminder dismiss [options]

Dismiss one exact fired reminder Inbox item

Options:
  --id <id>              Reminder id (full uuid or short prefix)
  --revision <revision>  Exact reminder source revision to acknowledge
  -h, --help             display help for command
[exit=0]
```

### raft app --help
```
Usage: raft app [options] [command]

Built-in RAP App operations

Options:
  -h, --help        display help for command

Commands:
  config [options]  Show or atomically update a built-in RAP App's durable
                    config
  help [command]    display help for command
[exit=0]
```

### raft app config --help
```
Usage: raft app config [options]

Show or atomically update a built-in RAP App's durable config

Options:
  --app <app-id>     Built-in RAP App id
  --set <key=value>  Set a boolean or integer config value (repeatable)
  --unset <key>      Remove an override and return to its declared default
                     (repeatable)
  -h, --help         display help for command
[exit=0]
```

### raft wiki --help
```
Usage: raft wiki [options] [command]

Canonical Wiki manifest operations

Options:
  -h, --help         display help for command

Commands:
  manifest           Read the canonical Wiki manifest and its current ETag
  read <artifactId>  Read the current canonical Markdown for one Wiki artifact
  publish [options]  Conditionally publish immutable Wiki revisions and the
                     canonical manifest
  help [command]     display help for command
[exit=0]
```

### raft wiki manifest --help
```
Usage: raft wiki manifest [options]

Read the canonical Wiki manifest and its current ETag

Options:
  -h, --help  display help for command
[exit=0]
```

### raft wiki read --help
```
Usage: raft wiki read [options] <artifactId>

Read the current canonical Markdown for one Wiki artifact

Options:
  -h, --help  display help for command
[exit=0]
```

### raft wiki publish --help
```
Usage: raft wiki publish [options]

Conditionally publish immutable Wiki revisions and the canonical manifest

Options:
  --input <path>  JSON file containing expectedEtag, manifest, and
                  revisionBodies
  -h, --help      display help for command

The input JSON must match the Wiki publication contract. Read the latest ETag with
`raft wiki manifest` immediately before constructing a publication.

[exit=0]
```

### raft migrate --help
```
Usage: raft migrate [options] [command]

Agent migration operations

Options:
  -h, --help         display help for command

Commands:
  export [options]   Deprecated: agent-initiated migration export is not
                     supported
  import [options]   Begin migration of the current agent toward a target
                     machine
  status             Show the active migration for the current agent
  ready [options]    Mark migration prep ready for the current agent
  arrived [options]  Mark migration arrival complete for the current agent
  help [command]     display help for command
[exit=0]
```

### raft migrate export --help
```
Usage: raft migrate export [options]

Deprecated: agent-initiated migration export is not supported

Options:
  --mode <mode>                   Export mode: cooperative or forensic
  --target <target>               Channel/DM/thread target to post the
                                  owner-commit card
  --target-machine-id <id>        Target computer UUID
  --target-computer <name-or-id>  Target computer name or UUID
  --to <name-or-id>               Alias for --target-computer
  --prep-deadline-ms <n>          Override prep deadline window in milliseconds
  --transfer-deadline-ms <n>      Override transfer deadline window in
                                  milliseconds
  --arrival-deadline-ms <n>       Override arrival deadline window in
                                  milliseconds
  -h, --help                      display help for command
[exit=0]
```

### raft migrate import --help
```
Usage: raft migrate import [options]

Begin migration of the current agent toward a target machine

Options:
  --target-machine-id <id>    Target machine UUID
  --to <id>                   Alias for --target-machine-id
  --prep-deadline-ms <n>      Override prep deadline window in milliseconds
  --transfer-deadline-ms <n>  Override transfer deadline window in milliseconds
  --arrival-deadline-ms <n>   Override arrival deadline window in milliseconds
  -h, --help                  display help for command
[exit=0]
```

### raft migrate status --help
```
Usage: raft migrate status [options]

Show the active migration for the current agent

Options:
  -h, --help  display help for command
[exit=0]
```

### raft migrate ready --help
```
Usage: raft migrate ready [options]

Mark migration prep ready for the current agent

Options:
  --manifest <path>         Path to the prepared migration manifest
  --manifest-sha256 <hash>  Manifest SHA-256. Defaults to hashing --manifest
                            when it is a readable local file.
  -h, --help                display help for command
[exit=0]
```

### raft migrate arrived --help
```
Usage: raft migrate arrived [options]

Mark migration arrival complete for the current agent

Options:
  --report <path>         Path to the arrival self-check report
  --report-sha256 <hash>  Report SHA-256. Defaults to hashing --report when
                          provided.
  -h, --help              display help for command
[exit=0]
```

### raft action --help
```
Usage: raft action [options] [command]

Action card operations (B-mode quick-commit shortcuts)

Options:
  -h, --help         display help for command

Commands:
  prepare [options]  Prepare an action card for a human to commit (B-mode
                     quick-commit shortcut)
  help [command]     display help for command
[exit=0]
```

### raft action prepare --help
```
Usage: raft action prepare [options]

Prepare an action card for a human to commit (B-mode quick-commit shortcut)

Options:
  --target <target>  Channel/DM/thread target to post the card. Same format as
                     raft message send: '#channel', 'dm:@peer',
                     '#channel:shortid', 'dm:@peer:shortid'
  -h, --help         display help for command
[exit=0]
```

