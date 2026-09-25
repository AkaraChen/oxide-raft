# Round 1 dispositions

Each finding was checked against the cited source, the oracle, or Node 24.15.0
with upstream's `node_modules` before it was applied. Totals: 65 fixed, 4
fixed-differently, 0 rejected, 1 needs-user.

## Needs user

**B33: how `attach` writes its state file (now `decisions.md` O7).** Upstream
writes the attach file in place with `writeFile` followed by `chmod 0600`
(`computer/src/services/attach.ts:276-300`). The earlier D12 had the Rust port
write a temp file and rename it, which closes a race with the service's startup
quarantine but changes on-disk behavior (a new inode, and the mode set at
create) that a TypeScript co-tenant on the same home can observe. The drop-in
contract forbids that without sign-off. Options: (a) write in place exactly as
upstream, keeping the race; (b) use temp file plus rename and record it as the
one intentional divergence from upstream. D12 now points to O7, and O7 says the
translation writes in place until you decide, because that is upstream's
behavior.

## Reviewer A

- **A1**: fixed. D5 now states undici's default request headers, their order, and the per-call-site `user-agent` (`node` vs `undici`), and it adds `FetchFlavor` plus transparent decoding. Guide §10 "Headers" is reversed to match. Verified by capturing raw requests from both fetch flavors.
- **A2**: fixed. D5 and guide §10/§13 require `http1_only()` and reqwest with `default-features = false` (no `http2`, `default-tls`, `charset`, or `macos-system-configuration`).
- **A3**: fixed. D3 adds `js::JsString` (UTF-16) as the result of code-unit slicing, with U+FFFD on UTF-8 sinks and `\udxxx` in `json_stringify`; guide §3 (string row) and §6.1 follow. This is the reviewer's first option.
- **A4**: fixed. Gate 3 in the README changed: `raft-computer --help` must match the oracle with exactly the `channel`, `operation`, and `upgrade` lines removed, and the diff is reviewed once. D13 keeps "not registered" and cross-references the gate. Oracle output confirmed that the column width is unchanged.
- **A5**: fixed. D1 now lists negatable options, hidden commands, the `alias()` getter, introspection, per-stream help width, the ambient exit code, parent-option consumption, `(value, previous)` parsers, and a wrap built on `JsRegex`, and it states that the crate never exits. The `option:version` listener is handled by deleting its unreachable branch (D10) rather than by porting option events.
- **A6**: fixed. D2 deletes the false codex claim (the daemon imports no zod), rewrites the passthrough ordering fact, and adds coercion, refine-abort, issue key order, the missing combinators, and a closure rule for re-exported schemas. All of this was verified with zod 4.3.6.
- **A7**: fixed. D5 states undici's 10 s / 300 s / 300 s defaults and translates them, emitting `UND_ERR_*` codes into D4's chain.
- **A8**: fixed. D3 makes `json_parse` the only translation of `JSON.parse`/`res.json()`, returning a `js::Value` (f64 numbers, lone surrogates, no depth limit) with V8 messages; this is reachable from stdin via `action/prepare.ts:94-100`. Guide §6.4 and §13 (`float_roundtrip`, no `arbitrary_precision`) follow.
- **A9**: fixed. D3 adds `locale_compare`, `resolved_time_zone`, `is_valid_time_zone`, local-offset getters, `to_locale_date_string_en_us_utc`, NFKC/NFKD, `to_locale_lower_case_en_us`, and `iso_ms`. Guide §6.5/§6.9/§6.12 and §13 follow; `jiff` replaces the suggested `chrono-tz` + `iana-time-zone` pair.
- **A10**: fixed. New D15 ports the used Ajv 2020 subset, and `jsonschema` is removed from §13. The guide header pins ajv 8.20.0, which is what pnpm resolves (the old "8.18.0" was the package.json range).
- **A11**: fixed. D4 adds the synthetic `fetch_error` Node error chain, two-path syscall messages, and Windows `uv_translate_sys_error` mapping.
- **A12**: fixed. D5 "TLS" honors `NODE_EXTRA_CA_CERTS`, `NODE_USE_SYSTEM_CA=1`, `NODE_TLS_REJECT_UNAUTHORIZED=0` (with Node's warning), and `NODE_USE_ENV_PROXY=1`, with Node 24.15.0's bundled roots as the default. `NODE_OPTIONS` is explicitly not interpreted.
- **A13**: fixed. D5 requires CONNECT tunneling for all proxied targets with undici's CONNECT headers, SOCKS, and WHATWG proxy-URL parsing, and it lists all three `proxy.ts` copies and the undici version per package (confirmed in `proxy-agent.js:106-195`).
- **A14**: fixed. D5 and guide §10 add `fetch_headers_entries()` and Node's `http.ServerResponse` defaults, which were verified on raw bytes: `Date`, `Connection: keep-alive`, `Keep-Alive: timeout=5`, `Transfer-Encoding: chunked`. Guide §6.10 maps `URL` onto `url::Url`, and §5 adds the query stringification rule from `agentApiRawClient.ts:168-180`.
- **A15**: fixed. D5 requires an undici-compatible multipart body. The boundary format `----formdata-undici-0` + 11 digits was verified from the oracle.
- **A16**: fixed-differently. D5 states the requirement (offer and implement `permessage-deflate; client_max_window_bits`, 100 MiB messages, `https-proxy-agent` CONNECT bytes, golden-tested), but it defers the crate choice to the `connection.ts` work unit, to be recorded in §13, instead of naming a crate now.
- **A17**: fixed. D3 adds `split`, `string_replace_first`/`string_replace_all`, and `JsRegex::replace_with`; guide §6.11 forbids `splitn`, `str::replace`, and `replacen`.
- **A18**: fixed. D3 requires `regress`'s `utf16` feature (`find_from_ucs2` without `u`, `find_from_utf16` with it), UTF-16 indices, `g`/`y`/`d` handled in `JsRegex`, and a Unicode-table golden. The APIs were confirmed in regress 0.11.1.
- **A19**: fixed. D3's `js::Object` keeps V8 order (array-index keys defined exactly) for all values, and D2, guide §3, and §6.8 use it.
- **A20**: fixed. Guide §3 splits the number rows (f64 for user input and unvalidated JSON), and §6.6 and §4.6 add `parse_int` with a radix, `math_round`, `math_floor_div`, and the ban on bare `as` casts. The helpers are in D3.
- **A21**: fixed. Guide §4.6 and §13 set `overflow-checks = true` in release and require `wrapping_*` for `>>> 0` code; the SHA-256 in `apmHeldFreshness.ts` is reachable through its exported functions.
- **A22**: fixed. Guide §3 adds a row for unvalidated JSON that is only cast (it stays `js::Value` with JS coercions), and §5 allows typed deserialization only after a schema parse. D3 adds `to_display_string`, `is_truthy`, and `is_nullish`.
- **A23**: fixed. Guide §7 specifies Windows case-insensitive `Env`, Node's spawn-key dedupe, libuv's `required_vars`, and `home_dir` per platform. The §9 spawn row adds `shell: true` command-line construction, `quote_cmd_arg`, and libuv's search order. Confirmed that an empty `HOME` gives `""`.
- **A24**: fixed. Guide §8 maps `Promise.all` to spawn-and-join, leaving the rest running after the first error, and `Promise.race` to `select!` that keeps losers running unless the source aborts them. Merged with B19.
- **A25**: fixed. D9 enumerates titles from the oracle per OS, keyed by file + describe path + title, and ports upstream skips as `upstream-skipped`. Guide §1.6 defines the identifier rule, and README gate 6 compares counts per OS.
- **A26**: fixed. D3 adds `ReadlineSplitter` and the guide §9 readline row points to it (verified: Node splits on a lone `\r` and decodes lossily). Agent output uses D7's splitter instead.
- **A27**: fixed. D5 adds the 20-hop redirect policy with fetch-spec header stripping, and D3 adds `response_text` (UTF-8 lossy, BOM stripped, charset ignored); guide §10 applies both.
- **A28**: fixed. D3's value model has `Undefined`; `object_assign` overwrites with it, and `json_stringify` omits it in objects and writes `null` in arrays. Guide §3 and §6.8 follow.
- **A29**: fixed. Guide §4.2 requires `js_name()` and `js::error_to_string`, and it decides `RAFT_COMPUTER_DEBUG_STACK` output as the `"<name>: <message>"` line only, the one accepted divergence for stacks.
- **A30**: fixed. Guide §13 is rewritten with features on and off, the added crates, and the release profile. §3 defaults `Map`/`Set` to `IndexMap`/`IndexSet`, with `HashMap` allowed only with a review note.
- **A31**: fixed. The README now says "detached background service (D11)", marks `raft migrate` blocked on O1 (and gate 3's `migrate` line), lists `__print-env` and `__build-versions` (D10 decides the latter's output), and assigns golden capture to the orchestrator. The guide header pins Node 24.15.0 and the per-package undici versions, and D1 states the no-exit rule.

## Reviewer B

- **B1**: fixed. D6 tags child events with `process_instance_id` and compares it where TS compares object identity. Confirmed with `rebindLaunch` (`agentProcessManager.ts:2519`), which changes the launch id of a live process.
- **B2**: fixed. D6 and D7 place the driver and session state in `CoreActor`, with per-process reader and ordered writer tasks; `send`/`parse_line` are synchronous in the actor.
- **B3**: fixed. D6 makes `get_computer_lifecycle_ready_acks` async, spawned, with a generation-fenced `ReadyAcksResolved` continuation (confirmed: `core.ts:1015` returns a Promise).
- **B4**: fixed. D11 replaces "accept and ignore" with the exact `parseLegacyOsSupervisorInvocation` refusal line plus the non-legacy `--os-supervised` validation (`index.ts:369-378`, `cli.ts:784-799`), and the README's dropped list notes the kept parser.
- **B5**: fixed. D13 translates `core.ts`'s handling and the computer hook straight through; the `upgrade-start` IPC keeps only the `K_COORDINATOR_SEA_ONLY` rejection, so the wire text matches a non-SEA TS service. The hook still enqueues the operation, as TS does, and its unbound `ready` phase stays pending, as on a non-SEA TS host.
- **B6**: fixed. D6 adds the rule: a real I/O or timer await becomes a continuation at the back of the queue with its re-checks translated, in-process awaits are not split, and each split point is listed in review notes.
- **B7**: fixed. D6 starts a new process's reader task only after the post-start continuation has run.
- **B8**: fixed-differently. D6 sets the timer fencing key by where the TS stores the handle (state-union timers by generation, per-process timers by `process_instance_id`) and requires the APM translation's review notes to list every timer kind, instead of enumerating the roughly 15 kinds in D6 now.
- **B9**: fixed. In D6, handlers run as independent tasks, registration is a direct registry update, and each synchronous segment is one actor request (the `PlanSideEffect` example).
- **B10**: fixed. D6 makes both proxies lazily created process singletons that are never closed and never keep the runtime alive (confirmed `unref()` and module globals).
- **B11**: fixed. In D7, `spawn` and `encode_stdin_message` take `DriverIo`; the codex and grok microtask writes and kimi's in-`spawn` writes are placed; and queued writes are pushed before the call's events are dispatched, line by line.
- **B12**: fixed. D7 adds the sub-decision for the model-detection clients.
- **B13**: fixed. D8 is rewritten: there is no PATH lookup, a SEA host always raises `NodeHostUnavailableError`, and the codex/gemini/opencode outcomes are listed (gemini and opencode fail only on their JS-entry paths, a narrower scope than the finding stated). `resolveOpencliBinPath` translates Node's CJS lookup from the executable's directory, confirmed by the SEA build's `import.meta.url` define (`build.mjs:198-206`).
- **B14**: fixed. D8 lists every `isSeaBinary`/`isSeaEntry`/`detectNodeHostKind` site with the branch taken, written as a list to match the document's style.
- **B15**: fixed. D8 keeps `ensureSeaRuntimePackageDir` (the file and `PI_PACKAGE_DIR`), because agents of kept runtimes observe it; the README's dropped list notes the exception.
- **B16**: fixed. D7 specifies the runtime-session splitter (split on `\n`, keep `\r`, skip whitespace-only lines, no trailing flush, chunk-trimmed stderr), and guide §9 separates it from readline.
- **B17**: fixed-differently. Instead of listing every consumer of each mutation, guide §7 makes `process.env` a process-wide `ProcessEnv` handle, read and mutated at the same points as TS (the mutations are listed), with snapshots only where TS copies. This removes the stale-snapshot problem for all consumers.
- **B18**: fixed. Guide §7 specifies Windows case-insensitive lookups, Node's sort-then-keep-first spawn serialization, and lossy UTF-8 on Unix.
- **B19**: fixed. Merged with A24 in guide §8, citing `concurrency.ts:147-157` and `stopAll`.
- **B20**: fixed. D13 drops the action-based retirement and translates `retireCompletedUpgradeShutdownsFromLog` as is. Confirmed that it removes only `shutdown` phases (`lifecycleOperations.ts:273-291`).
- **B21**: fixed. D13 says the capability is advertised under the same condition as TS.
- **B22**: fixed. D13 lists each K hook in kept code (not provided / replaced / dropped / kept) with its observable effect, including the co-tenant case; `readKRunnerHold` is kept.
- **B23**: fixed. D14's facts now state the gate (`__service` plus the `launchd-user`/`systemd-user` kind), its side effects, and the shell allowlist, and D11 and D14 read the same kind.
- **B24**: fixed. D14 specifies a single-threaded `poll(2)` capture with the same timeout, cap, and outcome rules, replace-not-merge via `remove_var`, and hands the delayed SIGKILL deadline to the runtime so start-up does not wait, matching the unref'd TS timer.
- **B25**: fixed. D11 removes "coalesced" and translates the TS schedule (startup reconcile, 5 s interval, independent timeouts), with continuations so IPC keeps being served.
- **B26**: fixed. D12 corrects the lock path to `<computerDir>/.lock`, lists every lock site with its options, and enumerates proper-lockfile's behaviors, including the `signal-exit` cleanup (clamping and `onExit` confirmed in `lockfile.js:219-221,331`).
- **B27**: fixed. D12 adds the daemon machine lock protocol with its own golden test.
- **B28**: fixed. Guide §7 defines `node_exec_path()` (realpath on macOS and Linux, raw on Windows), and D8/D11 use it; D11 keeps the dispatcher override.
- **B29**: fixed. D11 and the guide §9 spawn row use libuv's flags: `SW_HIDE` for `windowsHide`, and `CREATE_NO_WINDOW` only when no stdio slot is an inherited fd (so not for the service or `__run`).
- **B30**: fixed. D12 pins the named-pipe options to libuv's (first-instance flag only on the first, no remote-client rejection; to be checked against libuv source before implementing) and golden-tests the Windows handoff. Guide §9 covers `shell: true` and `quote_cmd_arg` inside `raft_shared::process::spawn` rather than a separate `js::` helper.
- **B31**: fixed. In D14, step 1 matches `__print-env` anywhere in argv.
- **B32**: fixed. D8's `__cli` uses the real stdio, returns `status ?? 1`, runs a current-thread runtime, and records the removal of `enforceSupportedNodeRuntime` (it cannot fire in a build embedding Node 24.15.0).
- **B33**: needs-user. See the section at the top; it is now `decisions.md` O7.
- **B34**: fixed. D6 lists every host hook with its sync/async shape, and the K-only hooks are marked as not provided.
- **B35**: fixed-differently. D6 keeps the open check and replay queue in the actor and delivers close as a `CoreMsg`, but it does not requeue frames the writer failed to flush: TS queues only when the socket is not `OPEN` (`connection.ts:196-222`), and a frame `ws.send` accepted is not replayed.
- **B36**: fixed. D8 and guide §8 give `__cli` its own current-thread runtime.
- **B37**: fixed. Guide §8 requires awaiting ref'd pending work before `main` returns.
- **B38**: fixed. Guide §7 gives the per-platform `home_dir` order (merged with A23).
- **B39**: fixed. The README covers the OS-service wording, `__print-env`, and the `lib/` wording. Guide §4.4 lists the service, resident, shutdown, and print-env exits. D6 spawns `on_computer_control` and has its emitters post to the actor. D8 keeps `upgradeExistingAgentWrappers` and `regenerateExistingOpencliWrappers`.
