# TypeScript → Rust mapping guide

Every translator follows this guide and every reviewer checks against it. When a
rule here conflicts with `decisions.md`, the per-item decision wins for the item
it names; report the conflict so the guide can be fixed.

Source of truth: `upstream/raft-source` at `05f7d8f` and `upstream/oar` at
`v0.0.7`. The TypeScript code is the behavioral oracle: `tools/oracle.sh` runs it
with upstream's locked dependencies (commander 12.1.0, zod 4.3.6, ajv 8.18.0,
undici 7).

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
   dropped feature (see `README.md` §Scope), delete the branch and list it in
   the file's review notes. Never leave `todo!()`, `unimplemented!()`, empty
   functions, or `// TODO: port` placeholders. If something in scope cannot be
   translated yet, stop and report it.
6. **Tests live next to the code.** `send.test.ts` →
   `commands/message/send_tests.rs`, included from `send.rs` with
   `#[cfg(test)] #[path = "send_tests.rs"] mod tests;`. Each TS `test("…")`
   becomes one Rust test whose function name is the snake_case of the title
   (truncate to 80 chars, dedupe with `_2`). The exact TS title goes in a
   `// test: <title>` line above it; `tools/test-parity` matches on that line
   (see §12).

## 2. Workspace and crate boundaries

| TS package | Rust crate | Notes |
|---|---|---|
| `packages/shared` (used closure only) | `raft-shared` | also hosts the JS-semantics helpers (§6) and the schema runtime (§5) |
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
| `string` | `String` (owned in structs), `&str` in parameters |
| `number` that is always an integer in practice (counts, seq, ms, status) | `i64` (`u16` for HTTP status). Use `f64` only where the source does arithmetic that can produce fractions |
| `number` that may be fractional (ratios, `usedRatio`) | `f64`, serialized with the JS number formatter (§6.3) |
| `boolean` | `bool` |
| `T \| undefined`, optional property `x?: T` | `Option<T>`; serialize with `skip_serializing_if = "Option::is_none"` |
| `T \| null` | `Option<T>` serialized as `null` (no skip) |
| `x?: T \| null` (both occur and are distinguished) | `Option<Option<T>>` with `serde_with::rust::double_option` |
| string-literal union `"a" \| "b"` | `enum` with `#[serde(rename_all = …)]` or per-variant `rename`; keep a `fn as_str(&self) -> &'static str` |
| discriminated union `{kind:"a",…} \| {kind:"b",…}` | `#[serde(tag = "kind")] enum` |
| open wire unions (server may add variants) | tagged enum plus `#[serde(other)] Unknown`, or decode to `serde_json::Value` first; never fail the connection on an unknown `type` |
| `Record<string, T>` | `IndexMap<String, T>` (insertion order matters for output) |
| `Map<K, V>` | `HashMap<K, V>`; `IndexMap` if iteration order is observable |
| `Set<T>` | `HashSet<T>`; `IndexSet` if iteration order is observable |
| `unknown` / `any` JSON | `serde_json::Value` |
| `Uint8Array` / `Buffer` | `Vec<u8>` / `bytes::Bytes` |
| `Date` | `chrono::DateTime<Utc>` for instants; keep raw `String` when the source passes the string through |
| branded types (`Brand<string,"MessageId">`) | plain `String` newtype only if the brand prevents a real mix-up in the translated file; otherwise `String` |
| `interface` with methods used for dependency injection | `trait` (object-safe; `Box<dyn Trait>` / `Arc<dyn Trait>`) |
| class with state | `struct` + `impl`; private `#field` → private field |

`serde_json` is built with `preserve_order`; all JSON objects that reach output
keep insertion order.

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
3. **Node errors.** When a Node `fs`/`net` error message reaches output
   (e.g. `ENOENT: no such file or directory, open '<path>'`), produce it with
   `raft_shared::node_compat::fs_error_message(op, path, &io::Error)`. Never
   print `io::Error`'s own `Display` where the source printed a Node message.
   Where no helper exists yet, add it there and cite the Node wording.
4. **`process.exit(n)` / `process.exitCode = n`** → return the code up to the
   binary's `main`. Only `main` calls `std::process::exit`. Exception: code the
   source runs inside a child/service entry that must exit immediately (e.g.
   exit 77/78 from `__run`) may call `std::process::exit` at that same point.
5. **Asserts.** A TS `assert`/invariant check that throws in production stays
   a runtime check (`if !cond { return Err(…) }` or `panic!` if the source
   crashes the process). Never use `debug_assert!` for it.

## 5. Contracts (zod)

Follow `decisions.md` D2. In short:

- Each zod schema is translated to a `raft_shared::schema` builder expression
  in the same order and with the same combinators (`object`, `passthrough`,
  `strict`, `string().trim().min(1)`, `optional`, `nullable`, `default`,
  `discriminated_union`, …). The schema runtime reproduces zod v4.3.6 parse
  output (stripping, key order, transforms, defaults) and issue messages.
- Code that reads parsed values uses typed structs deserialized from the
  schema's parsed `Value` (`schema.parse(v)?` then `serde_json::from_value`).
  Struct fields are declared in schema order.
- Anything sent on the wire is the schema's parsed output, exactly as the TS
  `requestAgentApiRawRoute` sends the parsed body.

## 6. JavaScript semantics that change bytes

Use the helpers in `raft_shared::js`. Do not re-implement them inline.

1. **String length and slicing**: TS `.length`, `.slice`, `.substring`,
   `.padStart`, `charAt`, regex match `index` are UTF-16 code units. Use
   `js::utf16_len`, `js::utf16_slice(s, start, end)` (JS clamping and negative
   index rules), `js::pad_start`. Byte indexing (`&s[..n]`) is forbidden on
   text that came from users, the server, or files.
2. **Regex**: every regex translated from TS uses `js::JsRegex` (backed by the
   `regress` ECMAScript engine) with the same pattern text and flags. It
   provides `test`, `exec`, `match_all`, `replace`, `replace_all`, `split` with
   JS semantics (`$1`/`$&` replacement syntax, `lastIndex` for `g`/`y`). Do not
   use the `regex` crate for translated patterns.
3. **JSON output**: `JSON.stringify(v)` → `js::json_stringify(&v)`;
   `JSON.stringify(v, null, 2)` → `js::json_stringify_pretty(&v, 2)`. They
   reproduce V8 number formatting (`1` not `1.0`, exponent at ≥1e21),
   integer-like key hoisting, `undefined` omission, and string escaping
   (including lone surrogates as `\udxxx`). `serde_json::to_string` is allowed
   only for data that never reaches stdout/stderr, disk files read by other
   programs, or the wire. When in doubt, use the `js` functions.
4. **JSON input**: `JSON.parse` → `serde_json::from_str::<Value>`. Where the
   V8 error message is printed, use `js::json_parse` which returns V8-worded
   errors (see D3).
5. **Dates**: `new Date(x).toISOString()` → `js::to_iso_string` (always
   milliseconds, `Z`). `new Date(string)` / `Date.parse` → `js::date_parse`
   (V8-compatible: ISO forms, date-only = UTC, date-time without offset =
   local time, plus the legacy formats V8 accepts). `Date.now()` →
   `raft_shared::clock::current_time_ms()`.
6. **Numbers**: `String(n)`, template-literal `${n}`, `n.toFixed(k)` →
   `js::number_to_string`, `js::to_fixed`. `parseInt`/`Number(x)` →
   `js::parse_int`, `js::to_number` (JS whitespace trimming, `""` → 0,
   `NaN` handling). Keep the NaN checks the source has.
7. **Truthiness and `??`**: `a ?? b` treats only `null`/`undefined` as
   missing; `a || b` also treats `""`, `0`, `false`, `NaN`. Translate each
   operator exactly. For env vars this means `Option<String>` where `Some("")`
   is distinct from `None` (see §7).
8. **Object spread and key order**: `{...a, ...b}` → insert `a`'s keys, then
   `b`'s keys, overwriting in place (an overwritten key keeps its original
   position). Use `js::object_assign` on `Map<String, Value>`.
9. **Sorting**: `Array.prototype.sort` without a comparator sorts by UTF-16
   string order → `js::sort_default`. With a comparator, use `sort_by` (both
   stable).
10. **`encodeURIComponent`** → `js::encode_uri_component`; `URLSearchParams`
    → `form_urlencoded::Serializer` (space → `+`).

## 7. Environment and process state

- `process.env` is read only at entry points (`main` of each bin, and the
  `__run`/`__service`/`__cli` entries), captured into `raft_shared::env::Env`
  (an ordered map of `OsString` keys → `String`, missing ≠ empty). Everything
  else receives `&Env`.
- Code that mutates `process.env` (e.g. `--profile` sets `RAFT_PROFILE`,
  DaemonCore sets `SLOCK_HOME`) mutates the `Env` value it owns and passes it
  down. Child processes are spawned with `env_clear()` + the explicit `Env`
  when the source passed an explicit env; when the source inherited
  `process.env`, pass the current `Env` snapshot.
- `process.platform` → `raft_shared::platform::PLATFORM` (`"darwin" | "linux" |
  "win32"`) where the string is observable; `cfg!(target_os = …)` otherwise.
  Platform branches stay runtime `if` unless they need platform-only APIs, in
  which case use `#[cfg]` on the smallest item possible.
- `os.homedir()` → `raft_shared::platform::home_dir(&env)` (same precedence as
  Node: `HOME`/`USERPROFILE`, then the OS lookup).
- `process.pid` → `std::process::id()`; `process.execPath` →
  `std::env::current_exe()`.

## 8. Async, timers, and concurrency

- Runtime: `tokio`. The `raft` CLI uses a current-thread runtime; the computer
  service and `__run` use multi-thread.
- `async function` → `async fn`. `Promise.all` → `futures::future::try_join_all`
  / `join_all` (preserve result order). `Promise.race` → `tokio::select!`.
- `setTimeout(fn, ms)` whose handle is cleared later → a spawned task or
  `tokio::time::sleep` inside `select!`, cancelled through a
  `CancellationToken` or `AbortHandle` stored where the TS stored the handle.
  `.unref()` timers must not keep the process alive: spawn them and let
  runtime shutdown drop them.
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
| `child_process.spawn` | `tokio::process::Command`; `stdio: "pipe"` → `Stdio::piped()`; `detached`/`windowsHide` mapped explicitly |
| `spawnSync(…, {stdio:"inherit"})` | `std::process::Command::status()` |
| `readline` line splitting | `tokio::io::BufReader::lines()` over the same stream; a final line without `\n` is still delivered, as Node does |
| `StringDecoder("utf8")` | `raft_shared::js::Utf8StreamDecoder` (replacement char for invalid bytes, carries partial sequences) |
| stdout/stderr writes | through the injected `Io` trait (`stdout`, `stderr`, optional `stdin`) so tests capture them |

## 10. HTTP, WebSocket, and servers

- HTTP client: `reqwest` (rustls). Build clients with `.no_proxy()` and apply
  proxies explicitly by translating the source's proxy selection
  (`packages/cli/src/proxy.ts`, `packages/daemon/src/proxy.ts`). Paths that
  used global `fetch` without a dispatcher use a no-proxy client.
- `redirect: "error"` → `redirect::Policy::none()` plus treating any 3xx as an
  error with the source's message; `"manual"` → `Policy::none()` and handle
  the 3xx as the source does.
- Timeouts: translate `AbortSignal.timeout(ms)` / explicit timers to
  `tokio::time::timeout` around the same span of work (headers only vs full
  body — match the source).
- Headers: send exactly the headers the source sends, same names and values.
  Do not add `User-Agent` or other defaults the source did not send where the
  server could observe the difference; `reqwest` adds none by default, keep it
  that way.
- `FormData` multipart → `reqwest::multipart::Form` with the same field names,
  file names, and content types.
- WebSocket client → `tokio-tungstenite` (rustls), custom headers on the
  handshake request, proxy via CONNECT when the source's `buildWebSocketOptions`
  selects one.
- Local HTTP servers (credential proxy, MCP proxy, test servers) → `hyper` 1.x
  via `axum` where routing helps. Bind `127.0.0.1:0` exactly as the source.

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
  same recorded fields. `assert.deepEqual` on objects → compare
  `serde_json::Value`s or derive `PartialEq`.
- HTTP fakes: tests that replaced `globalThis.fetch` inject a fake transport;
  tests that ran a real local server use a local `axum`/`hyper` server on
  `127.0.0.1:0`.
- Process tests that spawned `src/index.ts` spawn `env!("CARGO_BIN_EXE_raft")`
  (or `raft-computer`) with the same env and args.
- Fake timers → `tokio::time::pause()` / `advance()`; wall-clock seams use the
  component's `Clock` trait.
- Tests that inspect TypeScript source text, bundling, Node version
  preflight, npm packaging, or dropped features are not ported. Each one is
  listed with its reason in `tests-waived.md`; nothing is skipped silently.
- Golden fixtures captured from the oracle live in `tests/golden/<crate>/…`
  with the exact command that produced them in a sibling `.cmd` file.
- `tools/test-parity` compares the TS test manifest (titles per file) with the
  `// test:` lines in the Rust sources plus `tests-waived.md`; the gate fails
  on any TS test that is neither ported nor waived.

## 13. Dependencies (pinned in the workspace `Cargo.toml`)

`tokio`, `tokio-util`, `futures`, `reqwest` (rustls, stream, multipart, json),
`tokio-tungstenite` (rustls), `hyper`/`axum`, `rmcp` (MCP proxy only),
`serde`/`serde_json` (`preserve_order`)/`serde_with`, `indexmap`, `regress`,
`chrono`, `sha2`, `base64`, `uuid` (v4), `rand`, `hex`, `flate2`, `tar` (only
if migration stays in scope), `jsonschema` (draft 2020-12), `tempfile`,
`libc` (unix), `windows-sys` (windows), `anyhow`, `thiserror`, `percent-encoding`,
`form_urlencoded`, `dunce`, `fd-lock` or hand-rolled locks per D-records.
Adding any other crate needs a note in `decisions.md`.
