# TypeScript → Rust mapping guide

Every translator follows this guide and every reviewer checks against it. When a
rule here conflicts with `decisions.md`, the per-item decision wins for the item
it names; report the conflict so the guide can be fixed.

Source of truth: `upstream/raft-source` at `05f7d8f` and `upstream/oar` at
`v0.0.7`. The TypeScript code is the behavioral oracle: `tools/oracle.sh` runs it
on Node 24.15.0 (upstream's `.nvmrc`, also the Node the SEA build embeds) with
upstream's locked dependencies: commander 12.1.0, zod 4.3.6, ajv 8.20.0, ws
8.20.0, proper-lockfile 4.1.2, and undici 7.24.7 (cli, daemon), 7.24.8
(computer), and 7.24.4 (bundled with Node, behind global `fetch`).

## 1. Shape of a translated file

1. **One TS module → one Rust module, same relative path.**
   `packages/cli/src/commands/message/send.ts` →
   `crates/raft-cli/src/commands/message/send.rs`. File and directory names
   become `snake_case` (`agentApiPath.ts` → `agent_api_path.rs`,
   `apps/reminder/reminderCache.ts` → `apps/reminder/reminder_cache.rs`).
   `index.ts` barrels become `mod.rs` re-exports only when the TS barrel is
   imported; otherwise drop them.
2. **Keep declaration order and names.** Functions, types and constants appear
   in the same order as the source and keep their names, converted to Rust case:
   `camelCase` → `snake_case` for functions and variables, `PascalCase` stays for
   types, `SCREAMING_CASE` stays for constants. A reviewer must be able to put the
   two files side by side.
3. **Header line.** Each Rust file starts with one comment naming its source:
   `//! Port of packages/cli/src/commands/message/send.ts`. No other provenance
   comments.
4. **Comments.** Port only comments that state a constraint the code cannot
   show (protocol requirements, ordering requirements, platform quirks). Drop
   narrative, history and ticket references.
5. **Dropped code is deleted, never stubbed.** When a branch belongs to a
   dropped feature (see `README.md` §Scope), or a D-record declares it
   unreachable (for example D10's unknown-version branch), delete the branch and
   list it in the file's review notes. Never leave `todo!()`, `unimplemented!()`,
   empty functions, or `// TODO: port` placeholders. If something in scope cannot
   be translated yet, stop and report it.
6. **Tests live next to the code.** `send.test.ts` →
   `commands/message/send_tests.rs`, included from `send.rs` with
   `#[cfg(test)] #[path = "send_tests.rs"] mod tests;`. Each upstream test, as
   the oracle lists it (template titles expanded, one entry per loop
   iteration; `decisions.md` D9), becomes one Rust test. Its function name is
   the title transliterated to ASCII, lowercased, with every run of other
   characters replaced by `_`, prefixed with `t_` when it starts with a digit,
   truncated to 80 chars, and deduped with `_2`. The key goes in a
   `// test: <describe path> > <title>` line above it (just `<title>` when there
   is no describe), exactly as the oracle reports it; `tools/test-parity`
   matches on that line (see §12).

   Clarification (how `tools/test-parity` reads this rule; print a name with
   `node tools/test-parity/check.mjs --name "<title>"`). "Transliterated to
   ASCII" means NFKD normalization with combining marks U+0300–U+036F removed
   and nothing else: every other non-ASCII character (`§`, `→`, `—`, CJK) is one
   of the "other characters", so `§10 x` gives `_10_x`, not `ss10_x`; leading
   and trailing runs become `_` too. `t_` is added before truncating. The dedupe
   scope is the Rust test file, i.e. one upstream test file: its cases are taken
   in oracle order, ported or not, and the second case with the same base name
   gets `_2`, the third `_3`, whatever `mod` blocks or order the Rust file uses.
   In the key, `\n`, `\r` and tab in a title are written `\n`, `\r`, `\t`. A test
   file named `a.b.test.ts` becomes `a_b_tests.rs` next to `a.rs`, included from
   `a.rs` as `#[cfg(test)] #[path = "a_b_tests.rs"] mod b_tests;`; `.test.mjs`
   is stripped like `.test.ts`. oar's `tests/<p>.test.ts` has no sibling source:
   it becomes `crates/oar/src/tests/<p>_tests.rs`, compiled through
   `#[cfg(test)] mod tests;` in `lib.rs` and `mod` lines in `src/tests/mod.rs`.
   A `*_tests.rs` file that no `mod` reaches does not count as ported.

## 2. Workspace and crate boundaries

| TS package | Rust crate | Notes |
|---|---|---|
| `packages/shared` (used closure only) | `raft-shared` | also hosts the JS helpers and value model (§6, D3), the schema runtime (§5, D2), the JSON Schema validator (D15), and the `http`, `process`, `env`, and `node_compat` helpers |
| `packages/trace-client` | `raft-trace-client` | depends on `raft-shared` (`BasicTracer`) |
| `@botiverse/oar` v0.0.7 (used parts) | `oar` | Apache-2.0; sessions/pi/observe out of scope |
| commander 12.1.0 (used subset) | `commander` | byte-compatible parser + help, see `decisions.md` D1 |
| `packages/cli` | `raft-cli` | lib + `raft` bin; lib entry `run(argv, env, io) -> i32` |
| `packages/daemon` (DaemonCore closure) | `raft-daemon-core` | lib only |
| `packages/computer` | `raft-computer` | bin; links `raft-cli` (`__cli`) and `raft-daemon-core` (`__run`) |

Imports: `@botiverse/raft-shared` → `raft_shared::…` using the same module
path as the TS source file (`shared/src/raftRefs.ts` → `raft_shared::raft_refs`).
Symbols the TS barrel `index.ts` defines inline go in `raft_shared::index`
split by topic only if the file exceeds ~1500 lines; record the split in
`decisions.md`.

## 3. Types

| TypeScript | Rust |
|---|---|
| `string` | `String` (owned in structs), `&str` in parameters. Text sliced by UTF-16 index before it reaches a sink is `js::JsString` (D3) |
| `number` from user input, `parseInt`/`Number`, or JSON the source does not validate as an integer | `f64`; convert to an integer only through `js::to_integer_or_infinity` / `js::is_integer` at the source's own check |
| `number` the source itself produces as an integer and never takes from such input (counts, seq, clock ms, status) | `i64` (`u16` for HTTP status) |
| `number` that may be fractional (ratios, `usedRatio`) | `f64`, serialized with the JS number formatter (§6.3) |
| `boolean` | `bool` |
| `T \| undefined`, optional property `x?: T` | `Option<T>`; serialize with `skip_serializing_if = "Option::is_none"` |
| `T \| null` | `Option<T>` serialized as `null` (no skip) |
| `x?: T \| null` (both occur and are distinguished) | `Option<Option<T>>` with `serde_with::rust::double_option` |
| a key explicitly set to `undefined` in an object that is spread or stringified | `js::Value::Undefined` in a `js::Object` (§6.8) |
| string-literal union `"a" \| "b"` | `enum` with `#[serde(rename_all = …)]` or per-variant `rename`; keep a `fn as_str(&self) -> &'static str` |
| discriminated union `{kind:"a",…} \| {kind:"b",…}` | `#[serde(tag = "kind")] enum` |
| open wire unions (server may add variants) | tagged enum plus `#[serde(other)] Unknown`, or decode to `js::Value` first; never fail the connection on an unknown `type` |
| `Record<string, T>` with keys from data | `js::Object` (V8 key order, D3); `IndexMap<String, T>` only when every key is a fixed non-numeric literal |
| `Map<K, V>` | `IndexMap<K, V>` (JS `Map` iterates in insertion order); `HashMap` only with a review note stating iteration is never observed |
| `Set<T>` | `IndexSet<T>`; `HashSet` only with the same review note |
| `unknown` / `any` JSON | `js::Value` |
| unvalidated JSON the source only casts (`as T`) | `js::Value`, read with explicit JS coercions (`js::to_display_string`, the `??`/`\|\|` helpers); typed deserialization only right after a schema parse that guarantees the shape (§5) |
| `Uint8Array` / `Buffer` | `Vec<u8>` / `bytes::Bytes` |
| `Date` | `chrono::DateTime<Utc>` for instants, serialized through `js::iso_ms`; keep raw `String` when the source passes the string through |
| branded types (`Brand<string,"MessageId">`) | plain `String` newtype only if the brand prevents a real mix-up in the translated file; otherwise `String` |
| `interface` with methods used for dependency injection | `trait` (object-safe; `Box<dyn Trait>` / `Arc<dyn Trait>`) |
| class with state | `struct` + `impl`; private `#field` → private field |

Typed values reach output only through `js::to_value` (serde `Serializer` into
`js::Value`, fields in declaration order) and then `js::json_stringify`.

## 4. Control flow and errors

1. **Exceptions → `Result`.** Every function that can throw returns
   `Result<T, E>`. `try/catch` → `match`/`?` with the same scope. `finally` →
   explicit code on every exit path or a drop guard; say which in review notes.
2. **Thrown value types.**
   - A caught `unknown` → `anyhow::Error`.
   - A TS error class → a Rust struct implementing `std::error::Error` with
     the same name and fields (`CliError`, `AgentBootstrapError`, …).
   - `err instanceof X` → `err.downcast_ref::<X>()`.
   - `err.message` → the struct's `message` field or `err.to_string()`. The
     `Display` output must equal the TS `.message` exactly.
   - Every error type exposes `js_name()`, the TS `name` (`Error`, `TypeError`,
     `CliError`, …). `String(err)` and `${err}` → `js::error_to_string(&err)`,
     which renders `"<name>: <message>"`.
   - `err.stack` (printed only under `RAFT_COMPUTER_DEBUG_STACK`,
     `computer/src/cli.ts:922-925`) → `js::error_to_string(&err)` alone. V8's
     frame lines cannot be reproduced; this is the one accepted divergence for
     stacks.
3. **Node errors.** When a Node `fs`/`net`/fetch error message reaches output
   (e.g. `ENOENT: no such file or directory, open '<path>'`), produce it with
   `raft_shared::node_compat` (D4): `fs_error_message(op, path, &io::Error)`,
   its two-path form, or `fetch_error(&reqwest::Error)`. Never print
   `io::Error`'s or `reqwest::Error`'s own `Display` where the source printed a
   Node message. Where no helper exists yet, add it there and cite the Node
   wording.
4. **`process.exit(n)` / `process.exitCode = n`** → return the code up to the
   binary's `main`. Only `main` calls `std::process::exit`. Exceptions, each at
   the same point as the source: `__run`'s exits 77/78, the resident's
   `onHandshakeRejected` exit 77 and its `EX_CONFIG` exit (`service.ts:415,440`),
   the service's exit 0 at `service.ts:743`, `serviceShutdown`'s exit after its
   barrier (`lib/serviceShutdown.ts`), and print-env's exits 0 and 8
   (`index.ts:300-302`). The `commander` crate never exits (D1).
5. **Asserts.** A TS `assert`/invariant check that throws in production stays
   a runtime check (`if !cond { return Err(…) }` or `panic!` if the source
   crashes the process). Never use `debug_assert!` for it.
6. **Arithmetic.** The workspace sets `overflow-checks = true` in
   `[profile.release]`, so debug and release builds behave the same. Where the
   source relies on `>>> 0` or `| 0` wraparound (the SHA-256 in
   `apmHeldFreshness.ts`), use `u32`/`i32` `wrapping_*`. Other integer
   arithmetic that can overflow uses `checked_*` with the source's error.
   `Math.floor(a / b)` → `js::math_floor_div`, never integer `/`. No bare `as`
   casts between floats and integers; use the `js` conversions. Translated code
   never branches on `cfg!(debug_assertions)`.

## 5. Contracts (zod)

Follow `decisions.md` D2. In short:

- Each zod schema is translated to a `raft_shared::schema` builder expression
  in the same order and with the same combinators (`object`, `passthrough`,
  `strict`, `string().trim().min(1)`, `optional`, `nullable`, `default`,
  `discriminated_union`, …). The schema runtime works on `js::Value` and
  reproduces zod v4.3.6 parse output (stripping, V8 key order, coercion,
  transforms, defaults) and issues (messages, fields, key order).
- Code that reads parsed values uses typed structs deserialized from the
  schema's parsed value (`schema.parse(&v)?` then `js::from_value`). Struct
  fields are declared in schema order. Values the source casts without a schema
  stay `js::Value` (§3).
- Anything sent on the wire is the schema's parsed output, exactly as the TS
  `requestAgentApiRawRoute` sends the parsed body. Path params are substituted
  with `JsRegex::replace_with` (`agentApiRawClient.ts:144`). The query follows
  `agentApiRawClient.ts:168-180`: iterate the parsed object in V8 key order,
  skip `null`/`undefined`, `append` each array item as `String(item)`, `set`
  scalars as `String(value)`, and serialize as `URLSearchParams` does (§6.10).

## 6. JavaScript semantics that change bytes

Use the helpers in `raft_shared::js` (D3). Do not re-implement them inline.

1. **String length and slicing**: TS `.length`, `.slice`, `.substring`,
   `.padStart`, `charAt`, regex match `index` are UTF-16 code units. Use
   `js::utf16_len`, `js::utf16_slice(s, start, end)` (JS clamping and negative
   index rules), `js::pad_start`. A slice result is a `js::JsString`; it becomes
   UTF-8 with U+FFFD for lone surrogates when written to a stream, file,
   header, or body, and `\udxxx` escapes inside `json_stringify`. Byte indexing
   (`&s[..n]`) is forbidden on text that came from users, the server, or files.
2. **Regex**: every regex translated from TS uses `js::JsRegex` (backed by the
   `regress` ECMAScript engine running on UTF-16) with the same pattern text and
   flags. It provides `test`, `exec`, `match_all`, `replace`, `replace_all`,
   `replace_with` (callback replacers), and `split` with JS semantics
   (`$1`/`$&` replacement syntax, `lastIndex` for `g`/`y`, UTF-16 indices). Do
   not use the `regex` crate for translated patterns.
3. **JSON output**: `JSON.stringify(v)` → `js::json_stringify(&v)`;
   `JSON.stringify(v, null, 2)` → `js::json_stringify_pretty(&v, 2)`. They
   reproduce V8 number formatting (every number through f64: `1` not `1.0`,
   exponent at ≥1e21, `-0` as `0`), V8 key order, `undefined` omission, and
   string escaping (including lone surrogates as `\udxxx`).
   `serde_json::to_string` is allowed only for data that never reaches
   stdout/stderr, disk files read by other programs, or the wire. When in
   doubt, use the `js` functions.
4. **JSON input**: `JSON.parse` and `res.json()` → `js::json_parse`, always,
   never `serde_json::from_str`. It returns a `js::Value` (f64 numbers, lone
   surrogates allowed, no depth limit) and V8-worded errors (D3). `res.text()`
   → `js::response_text` (UTF-8 lossy, BOM stripped, charset ignored).
5. **Dates**: `new Date(x).toISOString()` → `js::to_iso_string` (always
   milliseconds, `Z`); `Date` fields serialize through `js::iso_ms`.
   `new Date(string)` / `Date.parse` → `js::date_parse` (V8-compatible: ISO
   forms, date-only = UTC, date-time without offset = local time, plus the
   legacy formats V8 accepts). Local fields (`getHours`, `getTimezoneOffset`,
   …) → `js::local_offset_minutes(ms, &env)` and its getters.
   `Intl.DateTimeFormat().resolvedOptions().timeZone` →
   `js::resolved_time_zone(&env)`; ICU time-zone validation →
   `js::is_valid_time_zone` (the raw user string is still what is sent);
   `toLocaleDateString("en-US", {month:"short", day:"numeric", timeZone:"UTC"})`
   → `js::to_locale_date_string_en_us_utc`. `Date.now()` →
   `raft_shared::clock::current_time_ms()`.
6. **Numbers**: `String(n)`, template-literal `${n}`, `n.toFixed(k)` →
   `js::number_to_string`, `js::to_fixed` (JS negative-zero rules).
   `parseInt(x)` / `parseInt(x, r)` → `js::parse_int(x, None | Some(r))`,
   `Number(x)` → `js::to_number` (JS whitespace trimming, hex/binary/octal
   literals, `""` → 0, `NaN` handling), both returning `f64`. `Math.round` →
   `js::math_round`. Keep the NaN checks the source has. See also §4.6.
7. **Truthiness and `??`**: `a ?? b` treats only `null`/`undefined` as
   missing; `a || b` also treats `""`, `0`, `false`, `NaN`. Translate each
   operator exactly; on `js::Value` use `js::is_nullish` and `js::is_truthy`.
   For env vars this means `Option<String>` where `Some("")` is distinct from
   `None` (see §7). `String(x)` or a template literal on a non-string value →
   `js::to_display_string` (`[object Object]`, `null`, `5`, …).
8. **Object spread and key order**: `{...a, ...b}` → `js::object_assign` on
   `js::Object`: `b`'s keys overwrite in place (an overwritten key keeps its
   original position), a `b` key set to `undefined` overwrites too, and array-
   index keys always iterate first in ascending order. `Object.keys`,
   `Object.entries`, and `for…in` iterate in that order.
9. **Sorting**: `Array.prototype.sort` without a comparator sorts by UTF-16
   string order → `js::sort_default`. With a comparator, use `sort_by` (both
   stable). `a.localeCompare(b)` → `js::locale_compare(a, b)`, never
   `str::cmp`.
10. **URLs**: `encodeURIComponent` → `js::encode_uri_component`;
    `URLSearchParams` → `form_urlencoded::Serializer` (space → `+`).
    `new URL(s)` → `url::Url`, read as WHATWG does: `href` = `as_str()`,
    `protocol` = scheme + `:`, `hostname` = `host_str()` (IPv6 bracketed),
    `port` = `""` for the scheme's default port, `host` = hostname plus
    `:port` when non-default, `origin`, `search` = `?` + query or `""`.
    `url.searchParams.set(…)` re-serializes the whole query with
    `form_urlencoded`, as WHATWG does.
11. **String API**: `s.split(sep, limit)` → `js::split` (split everywhere, then
    truncate); a string-pattern `replace` → `js::string_replace_first` and
    `replaceAll` → `js::string_replace_all`, both with `$` expansion. `splitn`,
    `str::replace`, and `replacen` are forbidden for translated string-API
    calls.
12. **Unicode**: `normalize("NFKC")`/`normalize("NFKD")` →
    `js::normalize_nfkc`/`js::normalize_nfkd`; `toLocaleLowerCase("en-US")` →
    `js::to_locale_lower_case_en_us`.

## 7. Environment and process state

- `process.env` → `raft_shared::env::ProcessEnv`: one process-wide, cloneable
  handle with interior mutability, built in each binary's `main` (after D14's
  steps in `raft-computer`). Code reads it at the same point the TS reads
  `process.env` and mutates it at the same points the TS mutates `process.env`:
  `--profile` sets `RAFT_PROFILE` in `preAction` (`cli/src/main.ts:240`);
  `__service` sets `SLOCK_HOME` and `RAFT_COMPUTER_OS_SUPERVISOR_KIND`
  (`computer/src/cli.ts:790,796`); `ensureSeaRuntimePackageDir` sets
  `PI_PACKAGE_DIR` (`service.ts:370`); `start` sets and restores the
  parent-lock marker (`services/start.ts:471-476`); DaemonCore sets
  `SLOCK_HOME` when no `slockHome` option is given (`daemon/src/core.ts:1523`).
  A value the TS copies (`{...process.env}`) is an `Env` snapshot taken at that
  point, and a function that takes an `env` parameter takes `&Env`. Tests inject
  their own `ProcessEnv`. The OS environment is not written after start-up
  (D14).
- `Env` is an ordered map; missing ≠ empty. On Unix, keys and values are
  decoded from the OS lossily as UTF-8, as Node does. On Windows, lookups are
  case-insensitive and inserts keep each key's spelling, so `{...env, PATH: x}`
  can hold both `Path` and `PATH`, as in TS.
- Child processes always get an explicit environment through
  `raft_shared::process::spawn` (§9): the `Env` the source passed, or the current
  `ProcessEnv` snapshot when the source inherited `process.env`. On Windows the
  spawn serializer does what Node and libuv do: sort keys by UTF-16 order, keep
  the first of each case-insensitive group (so `PATH` beats `Path`), then add
  any of libuv's `required_vars` missing from it from the parent (`HOMEDRIVE`,
  `HOMEPATH`, `LOGONSERVER`, `PATH`, `SYSTEMDRIVE`, `SYSTEMROOT`, `TEMP`,
  `USERDOMAIN`, `USERNAME`, `USERPROFILE`, `WINDIR`).
- `process.platform` → `raft_shared::platform::PLATFORM` (`"darwin" | "linux" |
  "win32"`) where the string is observable; `cfg!(target_os = …)` otherwise.
  Platform branches stay runtime `if` unless they need platform-only APIs, in
  which case use `#[cfg]` on the smallest item possible.
- `os.homedir()` → `raft_shared::platform::home_dir(&env)`, as libuv does it:
  on Unix `HOME` when set (an empty value gives `""`), else the passwd entry; on
  Windows `USERPROFILE` when set, else the OS profile directory. `HOME` is never
  read on Windows.
- `process.pid` → `std::process::id()`; `process.execPath` →
  `raft_shared::process::node_exec_path()`: `fs::canonicalize(current_exe())` on
  macOS and Linux (libuv realpaths it), and `current_exe()` unchanged on
  Windows (`GetModuleFileNameW`, no `\\?\` prefix, junctions not resolved).
  `path.resolve(process.execPath)` stays a lexical resolve of that value.

## 8. Async, timers, and concurrency

- Runtime: `tokio`. The `raft` CLI and `raft-computer __cli` use a
  current-thread runtime; the computer service and `__run` use multi-thread.
- `async function` → `async fn`.
- `Promise.all` rejects at the first rejection in time while the other
  promises keep running. Translate it as: spawn every element, then await the
  handles in completion order; on the first error return it and leave the rest
  running; on success return the results in input order. Inside `CoreActor` the
  elements are message-driven (D6), so the equivalent is a join counter in
  actor state. `join_all`/`try_join_all` without spawning are allowed only when
  no element has side effects after the first failure; say so in review notes.
- `Promise.race` → `tokio::select!` over `&mut` futures or spawned handles.
  After the winner, keep the losers running (spawn them or keep polling them)
  unless the source aborts them: TS lets them finish. Examples:
  `withComputerMutationLock`'s `fn` runs until it sees its abort signal
  (`computer/src/concurrency.ts:147-157`), and `bridge.ts:488` leaves
  `reader.read()` pending. Each site gets a review note.
- `setTimeout(fn, ms)` whose handle is cleared later → a spawned task or
  `tokio::time::sleep` inside `select!`, cancelled through a
  `CancellationToken` or `AbortHandle` stored where the TS stored the handle.
  `.unref()` timers must not keep the process alive: spawn them and let
  runtime shutdown drop them.
- Node keeps the process alive while ref'd handles remain (pending sockets,
  ref'd timers, child processes), so fire-and-forget work holding one finishes
  before exit. Before `main` returns, await the ref'd work the source leaves
  pending; only `.unref()`'d work is dropped.
- Fire-and-forget (`void promise`, `.then()` without await) whose completion
  mutates shared state → spawn the future and deliver the result to the
  owning actor as a message. Never mutate shared state from the spawned task
  directly.
- Event emitters (`on("data")`, `on("exit")`) → channels (`mpsc`/`broadcast`)
  or explicit callback traits; document delivery order if the source depends
  on it.
- Shared mutable state that TS mutates from many callbacks on one event loop →
  one owning task (actor) per component, per `decisions.md` D6. Use
  `Arc<Mutex<…>>` only for small leaf state that is never held across
  `.await`.
- `AbortController`/`AbortSignal` → `tokio_util::sync::CancellationToken`.

## 9. I/O

| Node | Rust |
|---|---|
| `fs.readFileSync` / `fs.promises.readFile` | `std::fs::read` / `tokio::fs::read` (match sync vs async of the source) |
| `writeFile(p, data, {mode})` | `OpenOptions` + `OpenOptionsExt::mode` on unix; mode ignored on Windows exactly where Node ignores it |
| `open(p, "wx")` | `OpenOptions::new().write(true).create_new(true)` |
| `O_NOFOLLOW` | `OpenOptionsExt::custom_flags(libc::O_NOFOLLOW)` on unix |
| `fsync` / dir fsync | `File::sync_all`; dir fsync via opening the dir on unix, no-op on Windows |
| `rename` | `std::fs::rename` |
| `mkdtemp(prefix)` | `tempfile::Builder::new().prefix(…).tempdir_in(dir)` then `into_path()` (keep the directory as the source does) |
| `lstat` / `realpath` | `symlink_metadata` / `std::fs::canonicalize` (Windows: `dunce::canonicalize`) |
| `process.kill(pid, 0)` | `raft_shared::process::pid_alive(pid)` (EPERM = alive, as the source treats it) |
| `child_process.spawn` / `spawnSync` | `raft_shared::process::spawn`, which reproduces libuv and Node: the explicit env (§7); `stdio: "pipe"` → pipes and `"inherit"` → the real fds; `detached` → `setsid` on Unix and `DETACHED_PROCESS \| CREATE_NEW_PROCESS_GROUP` on Windows; `windowsHide` → `STARTF_USESHOWWINDOW` + `SW_HIDE`, plus `CREATE_NO_WINDOW` only when no stdio slot is an inherited fd; `shell: true` on Windows → `cmd.exe /d /s /c "<joined args>"` with verbatim arguments; other Windows arguments quoted with libuv's `quote_cmd_arg`; libuv's Windows executable search (the current directory, then each `PATH` entry of the child env; the name as given, then with `.com`, then `.exe`). Golden-tested on the Windows CI leg |
| Node `readline` (`cli/src/commands/agent/login.ts:182`) | `js::ReadlineSplitter` (splits on `\r\n`, `\n`, lone `\r`; lossy UTF-8). Agent stdout and stderr use the runtime-session splitter (D7). Never `BufReader::lines()` |
| `StringDecoder("utf8")` | `raft_shared::js::Utf8StreamDecoder` (replacement char for invalid bytes, carries partial sequences) |
| stdout/stderr writes | through the injected `Io` trait (`stdout`, `stderr`, optional `stdin`) so tests capture them; `Io` can hand a child the real fds (D8) |

## 10. HTTP, WebSocket, and servers

- HTTP client: `raft_shared::http` (D5), never a bare `reqwest::Client`. It is
  HTTP/1.1 only and applies proxies explicitly by translating the source's
  proxy selection (`packages/cli/src/proxy.ts`, `packages/daemon/src/proxy.ts`,
  `packages/computer/src/proxy.ts`). Paths that used `fetch` without a
  dispatcher use no proxy, except as D5 says for `NODE_USE_ENV_PROXY`.
- Headers: every call site names its `FetchFlavor` (global `fetch` →
  `NodeGlobal`, the npm `undici` `fetch` → `NpmUndici`). Send the source's
  headers, same names and values; the wrapper adds undici's defaults
  (`accept`, `accept-language`, `sec-fetch-mode`, `user-agent`,
  `accept-encoding`) where the source did not set them. Add nothing else.
- Redirects: default and `redirect: "follow"` → D5's policy (20 hops,
  fetch-spec header stripping); `redirect: "error"` → `redirect::Policy::none()`
  plus treating any 3xx as an error with the source's message; `"manual"` →
  `Policy::none()` and handle the 3xx as the source does.
- Timeouts: undici's defaults always apply (D5). Translate
  `AbortSignal.timeout(ms)` / explicit timers to `tokio::time::timeout` around
  the same span of work (headers only vs full body — match the source).
- Response bodies: `res.text()` → `js::response_text`; `res.json()` →
  `js::json_parse(js::response_text(…))`. Iterating a `Headers` object →
  `http::fetch_headers_entries()`.
- `FormData` multipart → D5's undici-compatible multipart builder, with the
  same field names, file names, and content types.
- WebSocket client → the D5 client (permessage-deflate, 100 MiB messages),
  custom headers on the handshake request, proxy via CONNECT when the source's
  `buildWebSocketOptions` selects one.
- Local HTTP servers (credential proxy, MCP proxy, test servers) → `hyper` 1.x
  via `axum` where routing helps. Bind `127.0.0.1:0` exactly as the source.
  Responses reproduce Node's `http.ServerResponse`: user-set header names keep
  their case, then Node's defaults follow in Node's order: `Date`,
  `Connection: keep-alive`, `Keep-Alive: timeout=5`, and
  `Transfer-Encoding: chunked` (or `Content-Length` where Node computes one,
  when the body is ended in one call). Golden tests capture raw response bytes.

## 11. Output and help text

- All user- or agent-visible strings are copied character for character from
  the source, including punctuation (`·`, `…`, backticks), trailing spaces,
  and `\n` placement. Use raw strings for multi-line text.
- Never reformat, re-wrap, or "fix" copy. Typos in the source stay.
- CLI parsing and help go through the `commander` crate (D1); declare
  commands, options, arguments, and help text in the same order as the source.

## 12. Tests

- Translate each TS test into a Rust test with the same inputs and the same
  assertions (§1.6). Fakes become trait impls or in-memory structs with the
  same recorded fields. `assert.deepEqual` on objects → compare `js::Value`s or
  derive `PartialEq`.
- HTTP fakes: tests that replaced `globalThis.fetch` inject a fake transport;
  tests that ran a real local server use a local `axum`/`hyper` server on
  `127.0.0.1:0`.
- Process tests that spawned `src/index.ts` spawn `env!("CARGO_BIN_EXE_raft")`
  (or `raft-computer`) with the same env and args.
- Fake timers → `tokio::time::pause()` / `advance()`; wall-clock seams use the
  component's `Clock` trait.
- A test that upstream skips on some OS keeps the same condition as
  `#[cfg_attr(<cfg>, ignore = "upstream skip: <reason>")]` (D9).
- Clarification on location: a test that spawns the binary
  (`env!("CARGO_BIN_EXE_raft")`, `CARGO_BIN_EXE_raft-computer`) must be a
  Cargo integration test, because Cargo sets that variable only there. Such a
  test file lives under `crates/<crate>/tests/` at its §1.6 path without the
  leading `src/` (for example `crates/raft-cli/tests/commands/agent/login_tests.rs`,
  included from a `tests/<top>.rs` root with `#[path] mod`), or flat as
  `crates/<crate>/tests/<name>_tests.rs`. Everything else stays next to the
  code. A case keyed in both places fails the gate. Opt-in upstream skips
  (env var gates) are ported as a runtime check of the same variable, not as
  an ignore attribute (`decisions.md` D9).
- Clarification for Gate 5: a Rust test that ports no upstream test (for
  example the D3 golden and unit tests under `crates/raft-shared/src/js/`) is
  allowed only in a location listed in `EXTRA_TEST_LOCATIONS` in
  `tools/test-parity/check.mjs`, with the record that asks for it. Such tests
  are reported in their own `extra` column and are not part of the equality
  "keyed Rust tests = ported = in-scope − waived", which stays strict. An
  unkeyed test anywhere else fails the gate. When a ported test leaves out one
  upstream assertion (or injected input) that belongs to a dropped feature,
  the case still counts as ported and `tests-waived.md` gets a
  `<key> :: assert <what>` row, category `dropped`.
- Tests that inspect TypeScript source text, bundling, Node version
  preflight, npm packaging, or dropped features are not ported. Each one is
  listed with its reason in `tests-waived.md`; nothing is skipped silently.
- Golden fixtures captured from the oracle live in `tests/golden/<crate>/…`
  with the exact command that produced them in a sibling `.cmd` file.
- `tools/test-parity` compares the oracle's test list per OS (keyed by file +
  describe path + title, D9) with the `// test:` lines in the Rust sources plus
  `tests-waived.md`; the gate fails on any TS test that is neither ported nor
  waived.

## 13. Dependencies (pinned in the workspace `Cargo.toml`)

Versions are pinned in `[workspace.dependencies]`; features are as listed.
Adding any other crate or feature needs a note in `decisions.md`.

- Async: `tokio`, `tokio-util`, `futures`.
- HTTP: `reqwest` with `default-features = false` and `rustls-tls-webpki-roots`,
  `stream`, `multipart`, `gzip`, `brotli`, `deflate`, `zstd`, `socks` (not
  `http2`, `default-tls`, `charset`, or `macos-system-configuration`);
  `webpki-roots`, `rustls-native-certs` (`NODE_USE_SYSTEM_CA`); `url`,
  `form_urlencoded`, `percent-encoding`; `hyper`/`axum`; `rmcp` (MCP proxy
  only). WebSocket client: chosen per D5 when `connection.ts` is translated and
  recorded here.
- Data: `serde`, `serde_json` (`float_roundtrip`; never `arbitrary_precision`;
  used only behind the `js` bridges), `serde_with`, `indexmap`.
- JS semantics: `regress` (`utf16` feature), `icu_collator` and `icu_locale`
  (`locale_compare`), `unicode-normalization`, `jiff` (time zones, `TZ`, local
  offsets), `chrono`.
- Misc: `sha2`, `base64`, `uuid` (v4), `rand`, `hex`, `flate2`, `tar` (only if
  migration stays in scope), `tempfile`, `libc` (unix), `windows-sys` (windows),
  `anyhow`, `thiserror`, `dunce`. Locks are hand-rolled per D12.

Workspace profile: `[profile.release] overflow-checks = true` (§4.6).
