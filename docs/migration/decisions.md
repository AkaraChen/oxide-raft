# Per-item decision records

Each record names the items it governs, the facts it rests on (with sources),
and the decision. Translators apply it; reviewers check it. Research notes with
full citations are in `research/`.

---

## D1. Command-line parsing: port commander 12.1.0

**Governs:** `crates/commander`; every command definition in `raft-cli` and
`raft-computer`.

**Facts.**
- The `raft` output contract includes commander's own text: help layout and
  wrapping (the columns of the stream the help goes to, stdout for `--help` and
  stderr for error help, when that stream is a TTY; otherwise 80;
  `command.js:61-67,2257-2261`), `error: unknown option '--x'` plus
  `(Did you mean --y?)` (Damerau-Levenshtein, max distance 3, similarity
  > 0.4), `missing required argument 'x'`, `option '--t <t>' argument missing`,
  excess positionals accepted silently, global `-p/--profile` accepted after
  subcommands, bare groups printing help to stderr then the `(outputHelp)`
  envelope with exit 1 (`research/cli.md` §2.2, §2.4; Appendix A has all 116
  help outputs).
- `raft` uses `exitOverride` + a silenced `outputError`, and renders parse errors
  through its own envelope. `raft-computer` uses neither, so commander's default
  path is observable there too: the `error: …` line on stderr, then exit 1,
  reached through `_exit`, which calls `process.exit` (`command.js:507-512`).
  `help()` exits with `process.exitCode || 0`, forced to 1 only for error help
  (`command.js:2395-2405`).
- Both CLIs use commander beyond parsing. `raft` walks `cursor.commands`
  comparing `name()` and `alias()` (`cli/src/main.ts:134,146`), calls
  `createHelp().visibleCommands()` (`:150`) and `program.opts()` (`:238`), and
  reads `CommanderError.code`/`.exitCode`. `raft-computer` declares negatable
  options (`--no-start`, `computer/src/cli.ts:333,360`), hidden commands
  (`{ hidden: true }`, `:784-864`), and a parser without accumulator
  (`--lines`, `:492`).
- A parent command consumes the options it declares even when they follow a
  subcommand; `agent login` merges them with `{ ...optsWithGlobals(), ...options }`
  (`cli/src/commands/agent/login.ts:114`). Accumulating parsers receive
  `(value, previous)` with `previous` undefined on the first call
  (`integration/login.ts:48-51`, `app.ts:717-728`, `invoke.ts:1785-1798`).
- `Help.wrap` is a non-`u` JS regex over UTF-16 code units and uses a
  first-match string `replace('\r\n', '\n')` (`help.js:485-517`).
- clap cannot produce this text or these semantics byte for byte.

**Decision.** Port the used subset of commander 12.1.0 (`lib/command.js`,
`option.js`, `argument.js`, `help.js`, `suggestSimilar.js`, `error.js`) into
`crates/commander` as a translation. It must include parsing (short/long
options, `--opt=value`, combined short flags, `--`), parent options visible in
subcommands and consumed by the parent when they follow the subcommand,
`optsWithGlobals`, `opts`, variadic arguments, negatable options (`--no-x`,
default `true`, help term `--no-x`), custom option parsers called as
`(value, previous)` with `previous = None` on the first call, hidden options and
hidden commands, `addHelpText("after")`, `preAction` hooks, `version`,
`exitOverride`, `configureOutput` (separate out and err help widths), the
automatic `help` subcommand, suggestions, the help formatter (its `wrap` built
on `js::JsRegex` in non-`u` mode and `js::string_replace_first`), read access to
`commands`, `name()`, `alias()` (returns `None`; no command sets one),
`create_help().visible_commands()`, and `CommanderError { exit_code, code,
message }`.

The crate never exits the process. Where commander calls `process.exit(code)`,
the crate writes the same output and returns `Err(CommanderError)`;
`raft-computer`'s `main` returns that exit code (guide §4.4). The ambient
`process.exitCode` that `help()` reads is a field of the program state, set
where the source sets `process.exitCode`.

Features neither CLI uses (`choices`, `env`, `conflicts`, `implies`,
`passThroughOptions`, `enablePositionalOptions`, `allowUnknownOption`,
`.usage()`, the `.alias(name)` setter, `on("option:…")` events, whose one use
D10 removes) are not ported. Before translation, this list is checked against a
grep of every commander API used in `cli/src` and `computer/src`, including
tests to be ported; a missing API is added here. The grep (run when phase 1
started) added: read access to `command.options` with each `Option`'s `long`
and `flags` (`agent/login.test.ts:212-263`, `agent/bridge.test.ts:56-60`,
`integration/app.test.ts:318`); the registered arguments with `name()`
(`computer/src/cliServerArgContract.test.ts:50` reads `_args`); `new Option(…)`
with `hideHelp()` and `addOption` (`cli/src/core/command.ts:42-51`);
`outputHelp()` (`knowledge/get.test.ts:69`, `knowledge/search.test.ts:64`,
`integration/invoke.test.ts:1075`); and `helpInformation()`
(`computer/src/cliHelpCopy.test.ts`, `cliServerArgContract.test.ts:92`). Action handlers are async
closures returning `Result`. Proof: the 116 help goldens plus the parse-error
matrix from the oracle for both CLIs, including `raft-computer logs --lines`
with no value and with `x`, `5.9`, and `1e25`, `--no-start`, unknown commands,
and help output with hidden commands.

---

## D2. Contracts: a zod-compatible schema runtime

**Governs:** every translated zod schema (`agentApiContract.ts`,
`agentApiMessageContract.ts`, `daemonApiContract.ts`, `actionCards.ts`,
`externalAgentIntegration.ts`, `attachmentUploadContract.ts`,
`runtimeAccountUsage.ts`, `agentMigration.ts` if kept). A schema is in the
closure when an in-scope module imports it, directly or through the `index.ts`
barrel; a re-export alone does not count (`featureFlagRolloutGuardrail.ts` and
`slackBridgeProvisioning.ts` are in only if imported). The daemon's non-test
source imports no zod.

**Facts** (`research/shared.md` §2).
- The agent-API client sends the **parsed** params, query, and body. `.trim()`
  (100+ uses), `.default()`, and `z.coerce.number()` change the bytes on the
  wire.
- Responses are parsed too. `z.object` strips unknown keys, `passthrough` (155+
  objects) keeps them, and `strict` rejects them. Output key order follows the
  V8 object model (D3): array-index keys ascending first, then the other keys in
  insertion order, which for zod output is schema keys in schema order followed
  by unknown keys in input order. Verified: `{z:1, a:2, "5":0, b:" hi "}`
  through `object({b, a, d: default("x")}).passthrough()` gives
  `{"5":0,"b":"hi","a":2,"d":"x","z":1}`. Formatters and `--json` output print
  the parsed result.
- `z.coerce.number()` is `Number(x)`: `"0x10"` → 16, `" 12 "` → 12, `true` → 1,
  `null` → 0, and `"Infinity"` fails with `received: "Infinity"`. `task_number`
  (`agentApiContract.ts:557`) goes into the URL path, so `raft task claim 0x10`
  hits task 16.
- A refinement is not run when an earlier issue aborted the value (verified with
  `object({a: string()}).refine(fn)` on `{a: 1}`).
- Issue objects have a fixed key order per code, for example
  `{"expected","code","path","message"}` for `invalid_type` (with `received`
  after `code` when present) and
  `{"origin","code","minimum","inclusive","path","message"}` for `too_small`.
  `ZodError.message` is `JSON.stringify(issues, null, 2)` in that order.
- zod v4.3.6's default issue messages reach agents verbatim (`raft action
  prepare`), and the daemon-API client derives `cause/path/expected_kind/
  actual_kind` from zod issue codes.

**Decision.** Build `raft_shared::schema`: a small runtime that operates on
`js::Value` (D3) and mirrors the zod v4 combinators the closure uses. The list
is generated mechanically from the governed files and includes at least
`object`/`passthrough`/`strict`/`strictObject`, `string` with `trim`, `min`,
`max`, `regex`, `startsWith`, `uuid`, `datetime`, `url`, top-level `z.uuid()`,
`z.url()`, and `z.iso.datetime()` (with their own rules, not the `z.string()`
methods' rules), `number`/`int`/`z.int()`/`positive`/`nonnegative`/`finite`/
`safe`, `coerce.number` (through `js::to_number`), `boolean`, `literal`, `enum`,
`array`, `record`, `union`, `discriminatedUnion`, `optional`, `nullable`,
`nullish`, `default`, `undefined`, `null`, `transform` (identity branding only),
`refine`/`superRefine` as closures (including `z.ZodIssueCode` custom issues),
`meta` (no runtime effect), and `unknown`. Refinements follow zod's abort rule.
`parse(&js::Value) -> Result<js::Value, ZodError>` produces the same output
value, key order included, and the same `issues[]` (code, path, message, the
codes' extra fields, and each issue's key order) as zod 4.3.6. Each TS schema is
translated line for line into builder calls. Typed structs are deserialized
from the parsed value where code needs fields.

Proof: a golden corpus generated by the oracle. For every schema in the closure
and a set of valid and invalid inputs, record zod's `safeParse` result
(`tools/zod-golden/` script); the Rust runtime must reproduce each record,
including issue key order. The corpus covers coercion, refinements after an
aborting issue, and array-index keys under `passthrough`. This replaces
hand-guessing zod messages.

Rejected alternatives: plain serde structs (cannot reproduce stripping plus
passthrough ordering plus issue text without per-schema hand code), and
JSON-Schema generation (loses transforms and messages).

---

## D3. JavaScript semantics helpers and the JS value model

**Governs:** `raft_shared::js`; every translated expression listed in
`mapping-guide.md` §3 and §6.

**Facts** (`research/shared.md` §6, `research/cli.md` §3.1, §7.5, §10; each
item verified with Node 24.15.0).
- Agent-visible text depends on UTF-16 lengths and slices (limits of 40, 80,
  240, 500, and 4000; knowledge context 12–500). Slicing can split a surrogate
  pair, which a Rust `String` cannot hold: `"a😀b".slice(0,2)` stringifies as
  `"a\ud83d"` but is written to a stream as `a` + U+FFFD. Slice sites reach both
  sinks (`actionCards.ts:284` on the wire, `cli/src/proxy.ts:95`,
  `manifestV1.ts:907`, `actionV1.ts:180`). V8's `JSON.parse` accepts
  `"\ud800"`; serde_json rejects it.
- V8 orders array-index keys (canonical decimal integers up to 2^32−2)
  ascending before all other keys on every object: `{b:1,"2":2,a:3,"1":4,
  "01":5,"4294967295":6,"4294967294":7}` iterates as `1, 2, 4294967294, b, a,
  01, 4294967295`. This shows in `Object.entries` (`actionCards.ts:267-285`
  picks 24 presentation items that way; `agentApiRawClient.ts:172` orders query
  parameters that way), zod output, and `JSON.stringify`.
- `{...a, ...b}` with `b.k === undefined` overwrites `a.k` in place, and
  `JSON.stringify` then drops it (`agent/login.ts:114`, the
  `...(x ? {k: v} : {})` idiom in `client.ts`).
- `JSON.parse` yields doubles (`12345678901234567890` → `12345678901234567000`,
  `1e400` → `Infinity`, stringified `null`, `-0` → `0`) and has no depth limit.
  V8's error text reaches agents from stdin as well as from profile files
  (`action/prepare.ts:94-100`): `Unexpected token 'o', "nope" is not valid JSON`
  and `Expected double-quoted property name in JSON at position 7 (line 1
  column 8)`.
- Numbers are doubles: `Number.parseInt("5.9", 10)` → 5, `"1e25"` → 1, a
  24-digit string → `1e23` (`computer logs --lines`, `cli.ts:492` with
  `lib/api.ts:533-540`); `parseInt("  -0x1F")` → −31; `Math.round(-2.5)` → −2
  (`agent/_format.ts:239`); `(-0).toFixed(2)` → `"0.00"`.
- `s.split(sep, limit)` splits on every separator and then truncates
  (`"a:b:c".split(":", 2)` → `["a","b"]`; NO_PROXY in all three `proxy.ts`,
  `agentInboxStateMachine.ts:586,599`). A string-pattern `replace` replaces the
  first match only and still expands `$&`, `$1`, `$$`. Callback replacers build
  agent-API and manifest paths (`agentApiRawClient.ts:144`,
  `integration/invoke.ts:401`, `manifestV1.ts:295`).
- JS regexes without `u` step by UTF-16 code units (commander's wrap pattern);
  `regress` over `&str` steps by code points and reports byte offsets, and
  parses only the `imsuv` flags.
- Locale and time: `localeCompare` (ICU collation, default locale from the
  environment) in observable sorts (`agentInboxProjection.ts:104,137`,
  `agentAppInbox.ts:333`, `agentProcessManager.ts:8108`,
  `computer/src/setup.ts:529`); `Intl.DateTimeFormat().resolvedOptions().timeZone`
  sent as `body.tz` (`reminder/update.ts:53`, `schedule.ts:36`); `--tz`
  validated by ICU, case-insensitive and with aliases (`utc`, `EST5EDT`,
  `GMT+0`), the raw string sent unchanged (`schedule.ts:96`); local-time
  rendering (`message/_format.ts:66-76`, `message/search.ts:87-114`);
  `toLocaleDateString("en-US", {month:"short", day:"numeric", timeZone:"UTC"})`
  (`computer/src/setup.ts:357`); `normalize("NFKC")` with
  `toLocaleLowerCase("en-US")` (`externalProjection.ts:145-148`) and
  `normalize("NFKD")` (`managedMcp.ts:134`).
- `String(err)` and `${err}` render `"<name>: <message>"` (103 non-test sites).
- The hand-rolled SHA-256 in `apmHeldFreshness.ts:293-340` relies on `>>> 0`
  wraparound.

**Decision.** Implement `raft_shared::js` once, with oracle-generated golden
tests (Node 24.15.0, ICU 78.2) for each function.

Value model:
- `js::JsString` holds UTF-16 code units. `utf16_slice` and the other code-unit
  operations return it. Converting it to UTF-8 for a stream, file, header, or
  request body replaces lone surrogates with U+FFFD, which is what Node does;
  `json_stringify` escapes them as `\udxxx`. Text that is never sliced by code
  units stays `String`.
- `js::Value` is `Undefined | Null | Bool | Number(f64) | String(JsString) |
  Array(Vec<Value>) | Object(js::Object)`. `js::Object` keeps V8 order: array-index
  keys ascending, then insertion order, and an overwritten key keeps its
  position. `js::Value` is the type of `json_parse` results, schema input and
  output (D2), object spread, unvalidated JSON that the source only casts, and
  everything passed to `json_stringify`. Translated code does not use
  `serde_json::Value`. `js::from_value` (serde `Deserializer`) reads typed
  structs after a schema parse, and `js::to_value` (serde `Serializer`, fields
  in declaration order) turns typed values into `js::Value` for output.
- `is_nullish`, `is_truthy`, and `to_display_string` (`String(x)` on any value:
  `[object Object]`, `null`, `5`, …) give `??`, `||`, and template literals
  their JS meaning on `js::Value`.
- `json_stringify` omits `Undefined` object members and writes `null` for
  `Undefined` array elements, as V8 does.

Helpers:
- strings: `utf16_len`, `utf16_slice`, `pad_start`/`pad_end`, `trim` (JS
  whitespace set, including U+FEFF), `split(s, sep, limit)`,
  `string_replace_first(s, pat, repl)` and `string_replace_all(s, pat, repl)`
  with `$` expansion,
  `to_locale_lower_case_en_us`, `normalize_nfkc`/`normalize_nfkd`,
  `locale_compare` (ICU4X collator with the locale Node resolves from the
  environment), `error_to_string(&err)` = `"{js_name}: {message}"`;
- `JsRegex` over `regress` built with its `utf16` feature and run on UTF-16
  buffers: `find_from_ucs2` for patterns without `u`/`v`, `find_from_utf16` with
  them, match indices and `lastIndex` in code units, and the `g`, `y`, `d` flags
  handled by `JsRegex`. Methods: `test`, `exec`, `match_all`, `replace`,
  `replace_all`, `replace_with(|m: &JsMatch| -> JsString)` (captures, named
  groups, offset), and `split`. A golden checks `\p{L}`, `\p{N}`, and `iu` case
  folding against Node 24.15.0; any difference is listed in this record before
  a translated pattern depends on it;
- JSON: `json_stringify`, `json_stringify_pretty`, and `json_parse`, which is
  the only translation of `JSON.parse` and `res.json()`: f64 numbers, lone
  surrogates allowed, no depth limit, and V8's error messages for every error
  kind, including the context-snippet and position forms and the snippet
  truncation rule. `response_text(bytes)` is `res.text()`: UTF-8 lossy, a leading
  BOM stripped, the `content-type` charset ignored; `res.json()` is
  `json_parse(response_text(bytes))`;
- numbers: `number_to_string`, `to_fixed`, `to_number`,
  `parse_int(s: &JsString, radix: Option<f64>) -> f64` (JS radix inference; the radix is the JS argument, converted with ToInt32),
  `to_integer_or_infinity`, `is_integer`, `is_safe_integer`, `math_round`,
  `math_floor_div`;
- dates and zones: `to_iso_string`, `date_parse` (V8 legacy parser subset),
  `resolved_time_zone(&Env)` (honours `TZ`, returns ICU's canonical id),
  `is_valid_time_zone` (case-insensitive IANA ids plus ICU aliases),
  `local_offset_minutes(ms, &Env)` and the local-field getters,
  `to_locale_date_string_en_us_utc`, and a serde adapter `iso_ms` for
  `DateTime<Utc>` fields;
- `encode_uri_component`, `Utf8StreamDecoder`, `ReadlineSplitter` (Node
  readline: splits on `\r\n`, `\n`, and a lone `\r` with readline's `crlfDelay`
  handling, over `Utf8StreamDecoder`), `sort_default`, and `object_assign` (over
  `js::Object`; `Undefined` overwrites).

Golden inputs include astral characters, lone surrogates, array-index keys,
`undefined` members, and edge numbers.

Settled while implementing the core helpers (phase 1):
- Errors implement `js::JsErrorName` (`js_name()`; the message is `Display`);
  `error_to_string(&err)` renders `"<name>: <message>"`, `error_to_string_parts`
  is the raw form. A `JsError` message is a Rust `String`, so a lone surrogate
  quoted in a `JSON.parse` error shows as U+FFFD (Node prints it the same way to
  a stream).
- Nesting: `json_parse` has no depth limit; drop, clone, equality,
  `to_display_string`, and `json_stringify` are iterative, so they succeed where
  V8 throws `RangeError` (beyond about 6,161 levels for stringify, about 2,816 for
  `String()`). The serde bridge stops at 128 levels with `RangeError: Maximum
  call stack size exceeded`.
- `pad_start`/`pad_end` return `Result` and throw `RangeError: Invalid string
  length` above V8's 2^29−24 code units. U+0085 is not JS whitespace: `trim`,
  `Number()`, `parseInt`, and `\s` all leave it (verified on Node 24.15.0).
- `parseInt` with a radix other than 10 or a power of two accumulates with a
  fused multiply-add: V8 contracts `result * multiplier + part` on arm64, the
  oracle machine. An x64 Node may differ in the last bit.
- A `JsString` given to a non-`js` serializer is written as UTF-8 with U+FFFD
  for lone surrogates; `from_value` into `String` is lossy the same way.
- Deferred to their own phase-1 units: `JsRegex`, locale collation and
  normalization, dates and time zones, `Utf8StreamDecoder`/`ReadlineSplitter`,
  and `split`/`to_locale_*` helpers not yet listed in `js`. Public names added
  beyond the list above: `JsErrorName`, `error_to_string_parts`,
  `json_parse_js`, `MAX_STRING_LENGTH`, `collapse_js_whitespace`, `less_than`,
  `is_finite`, `to_number_value`, `trim_start`, `trim_end`, and `js::convert`.
- `Value` implements `Drop` (iterative), so contents are moved out with
  `into_*` accessors or `std::mem::take`, not by destructuring.
- Float/integer conversions for translated code (guide §4.6) are the checked
  helpers in `js::convert` (`f64_to_i64_exact`, `to_int32`, `to_uint32`,
  `usize_to_f64`, …); they are the only place such casts live.

---

## D4. Errors that carry Node text

**Governs:** `raft_shared::node_compat`; any output path that prints a Node or
undici error message.

**Facts.** Visible examples: `ENOENT: no such file or directory, open '<p>'`
(profile errors), `EACCES: permission denied, …`, `Unexpected error: fetch
failed` for a raw fetch failure, and
`fetch failed for <url>: <class>/<CODE>` from the canonical proxy wrapper
(`research/cli.md` §3.2, §4.1, §4.2). `classifyFetchTransportFailure`
(`cli/src/proxy.ts:95-148`) reads Node, undici, and OpenSSL `code` strings from
the cause chain and regex-matches `${err.name} ${err.message}`; reqwest, hyper,
and rustls errors carry neither. The same undici text goes into traces as
`original_message` (`cli/src/client.ts:290-291`).

**Decision.** `node_compat` maps `io::Error` + syscall + path to Node's message
format (`<CODE>: <description>, <syscall> '<path>'`, and
`<CODE>: <description>, <syscall> '<a>' -> '<b>'` for `rename`, `copyfile`,
`link`, and `symlink`) using libuv's code and description table for the codes
that can occur (ENOENT, EACCES, EPERM, EEXIST, ENOTDIR, EISDIR, ELOOP, EMFILE,
ENOSPC, EROFS, EBUSY, ENOTEMPTY, EXDEV, ETIMEDOUT, ECONNREFUSED, ECONNRESET,
EPIPE, EADDRINUSE, ENETUNREACH, EHOSTUNREACH, EAI_AGAIN/ENOTFOUND). On Windows
the code comes from the raw OS error through libuv's `uv_translate_sys_error`
table (for example `ERROR_SHARING_VIOLATION` → `EBUSY`), not from
`io::ErrorKind`.

Fetch failures go through a synthetic Node error layer:
`node_compat::fetch_error(&reqwest::Error) -> NodeErrorChain` yields
`TypeError: fetch failed` with a `cause` that carries Node's exact `code` and
`message` (`getaddrinfo ENOTFOUND <host>`, `connect ECONNREFUSED <ip>:<port>`,
`UND_ERR_CONNECT_TIMEOUT`, `UND_ERR_HEADERS_TIMEOUT`, `UND_ERR_BODY_TIMEOUT`,
and the OpenSSL codes Node reports, such as `UNABLE_TO_VERIFY_LEAF_SIGNATURE`,
`DEPTH_ZERO_SELF_SIGNED_CERT`, `CERT_HAS_EXPIRED`, and
`ERR_TLS_CERT_ALTNAME_INVALID`, mapped from rustls errors). The source's
`classifyFetchTransportFailure` and its regexes then run on that chain
unchanged, and traces record the same text. A raw fetch throw renders as
`fetch failed`. Each code, and the Windows table, is golden-tested against the
oracle.

---

## D5. HTTP stack and proxies

**Governs:** `raft_shared::http`; `raft-cli` `client.rs`, `proxy.rs`,
`agent_api_path.rs`, `daemon_api_path.rs`; daemon `proxy.rs`, `daemon_fetch.rs`,
`connection.rs`, `agent_credential_proxy.rs`; computer `proxy.rs`,
`api_client.rs`, `lib/api.rs`.

**Facts** (`research/cli.md` §4; `research/daemon.md` §3; undici under
`node_modules/.pnpm/undici@7.24.7`; request bytes verified against a local
server).
- Three undici builds are in play: npm 7.24.7 (cli, daemon), npm 7.24.8
  (computer), and Node 24.15.0's bundled 7.24.4 behind global `fetch`.
- Proxy selection has three copies of `proxy.ts` (cli, daemon, computer):
  scheme-based env precedence with `||`, NO_PROXY with host:port and suffix
  rules. Some paths bypass proxies entirely (invokeV1, presigned PUT, bridge
  activity, device auth). The daemon's WebSocket proxy precedence also covers
  `WSS_PROXY`/`WS_PROXY`.
- undici fetch adds each of these headers when the caller did not set it:
  `accept: */*`, `accept-language: *`, `sec-fetch-mode: cors`, `user-agent`, and
  `accept-encoding` (`br, gzip, deflate` for https, `gzip, deflate` for http).
  The HTTP/1 writer adds `host` and `connection: keep-alive`. Observed order:
  `host`, `connection`, caller headers, `accept`, `accept-language`,
  `sec-fetch-mode`, `user-agent`, `accept-encoding`, `content-length`.
  `user-agent` is `node` for global `fetch` (`cli/src/client.ts:272`,
  `cli/src/proxy.ts:215`) and `undici` for the npm package's `fetch`
  (`agentLogin/deviceAuthClient.ts:26`, `commands/agent/login.ts:16`,
  `agent/list.ts:30`, `computer/src/proxy.ts:7`, `computer/src/apiClient.ts:5`,
  `daemon/src/daemonFetch.ts:1`).
- Responses are decoded transparently (gzip, deflate, br, zstd);
  `agentCredentialProxy.ts:393-398` strips `content-encoding` for that reason.
- undici speaks HTTP/1.1 only; no call site sets `allowH2`.
- Every undici client has a 10 s connect timeout, a 300 s headers timeout, and a
  300 s body timeout (`core/connect.js:53`, `dispatcher/client.js:256-257`). The
  daemon's explicit lanes override some of them (`daemon/src/proxy.ts:202-206,
  245-258`). There are no retries.
- `ProxyAgent` tunnels with CONNECT for http and https targets (`proxyTunnel`
  defaults to true), sending `host`, `proxy-connection: keep-alive`, and
  `proxy-authorization: Basic …` from the URL's credentials; `socks:`/`socks5:`
  URLs are supported. The proxy URL is parsed with WHATWG `new URL`, so a
  scheme-less value fails.
- Default and `redirect: "follow"` fetches (`agentCredentialProxy.ts:619,1276`)
  follow up to 20 redirects with fetch-spec header stripping on cross-origin
  hops.
- `FormData` bodies (`cli/src/client.ts:357-377`) use the boundary
  `----formdata-undici-0` + 11 digits, write filenames as raw UTF-8 with only
  `"`, CR, and LF escaped (`%22`, `%0D`, `%0A`), and set `content-length`.
- `Headers` iteration yields lowercase names in sorted order, duplicates joined
  with `", "`, and `set-cookie` kept separate (`agentCredentialProxy.ts:392-399`).
- The WebSocket client is `ws` 8.20.0 with default options
  (`daemon/src/connection.ts:233-248`). It offers
  `permessage-deflate; client_max_window_bits` and accepts messages up to
  100 MiB. A proxied handshake goes through `https-proxy-agent` 7.0.6's CONNECT.
- Running under Node 24.15.0, the CLI and the computer honour
  `NODE_EXTRA_CA_CERTS`, `NODE_TLS_REJECT_UNAUTHORIZED=0` (with Node's warning on
  stderr), `NODE_USE_SYSTEM_CA=1`, and `NODE_USE_ENV_PROXY=1`, although the
  source never names them. The default trust store is Node's bundled Mozilla
  set.

**Decision.** `raft_shared::http` wraps reqwest built with
`default-features = false`, rustls, `http1_only()`, decoding for gzip, brotli,
deflate, and zstd, SOCKS support, `.no_proxy()`, and
`.default_headers(HeaderMap::new())` so reqwest adds no `accept` of its own.
- Every translated fetch call site names its `FetchFlavor::{NodeGlobal,
  NpmUndici}`, which sets `user-agent`. The wrapper inserts undici's default
  headers, in undici's order, only when the caller did not set them, with
  `accept-encoding` keyed on the URL scheme.
- Proxies: a translation of each package's `proxy.ts` (`getProxyUrlForTarget`,
  `shouldBypassProxy`) picks the proxy per request target, and one client per
  proxy URL is cached as `buildFetchDispatcher` does. The proxy URL is parsed
  with `url::Url` and rejected wherever undici rejects it, with the same error.
  Proxied targets, http included, go through a CONNECT tunnel connector that
  sends undici's CONNECT headers; `socks`/`socks5` URLs use SOCKS.
- Timeouts: `connect_timeout(10s)`, a 300 s deadline until response headers,
  and a 300 s idle deadline per body chunk, replaced exactly where the source
  passes other values. Expiry produces the matching `UND_ERR_*` code in D4's
  chain. No retries.
- Redirects: `Policy::custom` with undici's limit of 20 and fetch-spec header
  stripping; `redirect: "error"` and `"manual"` as in guide §10.
- Multipart: a hand-built body that reproduces undici's boundary format, part
  header order and casing, filename escaping, and `content-length`.
- Response headers are read through `http::fetch_headers_entries()` (WHATWG
  sort and combine).
- WebSocket: the client must offer and implement
  `permessage-deflate; client_max_window_bits`, which `tokio-tungstenite` does
  not, with a 100 MiB message limit and no smaller frame limit. A proxied
  handshake is a CONNECT with `https-proxy-agent`'s header set. The crate is
  chosen when `connection.ts` is translated and recorded in guide §13.
- TLS: rustls with the root set Node 24.15.0 bundles (`webpki-roots` at the
  matching Mozilla snapshot); `NODE_EXTRA_CA_CERTS` adds a PEM file, with Node's
  warning text when the file cannot be loaded; `NODE_USE_SYSTEM_CA=1` adds the
  OS store; `NODE_TLS_REJECT_UNAUTHORIZED=0` disables verification and prints
  Node's one-time warning on stderr. `NODE_USE_ENV_PROXY=1` installs a
  translation of undici's `EnvHttpProxyAgent` for call sites that pass no
  dispatcher (both flavors; they share undici's global dispatcher).
  `NODE_OPTIONS` is not interpreted.
- Golden tests capture raw request bytes from the oracle and from Rust against
  a local socket: plain requests per flavor, proxied CONNECT, multipart, the
  WebSocket handshake, and its proxied CONNECT.

---

## D6. DaemonCore concurrency model

**Governs:** `raft-daemon-core`: `core.rs`, `agent_process_manager.rs`,
`agent_start_coordinator.rs`, `connection.rs`, `agent_credential_proxy.rs`,
`managed_mcp_runtime_proxy.rs`, `drivers/runtime_session.rs`, `apps/*`.

**Facts** (`research/daemon.md` §10; paths under `daemon/src`).
- All state is mutated from one JS event loop, with invariants that rely on no
  await between check and set (start dedupe maps, computer-control operation
  dedupe, start-queue invariants). There are about 15 timer kinds per agent.
  Fire-and-forget promises complete later and mutate state.
  Connection-generation fencing ignores stale sockets and retries. The
  credential proxy's HTTP handlers wait on agent state (the freshness hold).
- Process events are fenced by object identity, not by launch id: listeners
  close over the `runtime` instance and check `current.runtime === runtime`
  (`agentProcessManager.ts:3262-3320`), and `rebindLaunch` gives a live process
  a new launch id (`:2519`). Each spawn has a `processInstanceId` (`:3130`).
- APM handlers read and write driver and session state synchronously:
  `runtimeProcessBindingFence.send` calls `driver.encodeStdinMessage`, writes
  stdin, and returns `{ok, acceptedAs}` in the same tick (`:2597`;
  `drivers/runtimeSession.ts:138-149`); `ap.driver.currentSessionId` and
  `runtime.currentRuntimeHomeDir` are read inline (`:7301`, `:4993`).
- Handlers interleave at real I/O awaits and re-check state afterwards:
  `startAgent` around `buildSpawnConfig` and `driver.spawn`, then
  `stopEpochChanged`; stop around `runtime.stop`, then `startEpochChanged`
  (`:3124-3226`, `:4088-4170`). An await on in-process promises yields only to
  microtasks, so no I/O event runs in between.
- `RuntimeSession.start` attaches its stdout listener after `await
  driver.spawn`, and the APM continuation after `start` (status `active`,
  "Starting…", startup timeout; `:3676-3681`) runs before any stdout event.
- Not every timer lives in the state union: `RuntimeSession.stop` arms an
  unref'd SIGKILL timer that checks that session's `closed`
  (`runtimeSession.ts:161-169`).
- The credential proxy's side-effect step reads coordinator state, awaits
  `loadRecentTargetMessages`, then calls `planAgentInboxSideEffect` with a
  synchronous `isMessageModelSeen` callback into APM state
  (`agentCredentialProxy.ts:1498-1544`).
- Both proxy servers are process-global, bound lazily once on `127.0.0.1:0`,
  `unref()`'d, and never closed (`agentCredentialProxy.ts:232-336`,
  `managedMcpRuntimeProxy.ts:437-448`).
- Host hooks (`core.ts:1007-1063`): `getComputerLifecycleReadyAcks` returns a
  Promise and may wait up to 60 s (`computer/src/machineServiceAttestation.ts:125-160`),
  and `emitReady` re-checks the connection generation after it
  (`core.ts:4204-4216`). `onComputerControl` runs detached
  (`void Promise.resolve().then`, `core.ts:4145`), and its emitters call
  `connection.send`.
- `connection.send` is synchronous: it writes when the socket is `OPEN`, and
  otherwise queues replayable messages and drops the rest
  (`connection.ts:196-222`).

**Decision.** One actor task, `CoreActor`, owns everything the TS `DaemonCore`
and `AgentProcessManager` instances own, including each agent's driver and
runtime-session state (D7). It receives a single `enum CoreMsg` from:
- the connection (inbound server frames, open/close with a generation);
- child processes (stdout chunks, stderr chunks, exit, close, error), each
  tagged with `(agent_id, process_instance_id)`; the actor accepts an event
  only where the TS identity check would, by comparing `process_instance_id`
  with the bound session's;
- timers;
- spawned futures (fire-and-forget completions and continuations);
- the proxy servers (one request per TS synchronous segment, answered through a
  `oneshot`).

Raw pipe I/O stays outside the actor: per process, a reader task that posts
chunks and an ordered writer task fed by an unbounded channel. `send()`,
`parse_line()`, and reads such as `current_session_id` run inside the actor and
return synchronously; stdin bytes go to the writer channel in call order. The
reader task of a new process starts only after the post-start continuation has
run, so no output precedes `active` and the startup timeout.

The actor handles each message to completion without awaiting other
components. An await on real I/O or a timer becomes a continuation message
posted to the back of the queue. Other messages may run first, as in Node, so
every re-check that follows the await in TS is translated. An await that only
chains in-process promises is not split; the following code runs in the same
handler. Each split point is listed in the file's review notes with the
re-check that follows it.

Timers are `tokio::time::sleep` tasks that post `CoreMsg::Timer`. The fencing
key follows where the TS stores the handle. A timer held in APM's state union
carries `(agent_id, generation)` and is dropped when the generation no longer
matches; this replaces the TS "timer lives in the state union" rule (the
`timerStateTypes` typeproof becomes this check). A timer created by a runtime
session or another per-process object carries `process_instance_id` and fires
against that process even after the agent has moved on (the stop SIGKILL). The
APM translation lists every timer kind with its key in its review notes.

Pure modules (`apmStateMachine`, `launchPhaseTransition`, `agentInbox*`, the
normalizers, `agentRuntimeInput`) stay plain functions and structs, with no
actor.

The credential proxy and the MCP proxy are process singletons: created lazily
on first registration, alive for the process lifetime, never closed, and never
keeping the runtime alive on their own. Their HTTP handlers run as independent
tasks. Registration is a direct update of a registry shared with the handlers
(`Arc<RwLock>`), never a request queued behind handlers. Each synchronous
segment of a handler that touches agent state is one actor request that carries
everything it needs; the side-effect step's segment after
`loadRecentTargetMessages` is a `PlanSideEffect` request that runs
`planAgentInboxSideEffect`, with `isMessageModelSeen`, inside the actor.

The connection's `send` runs in the actor with the TS open check and replay
queue. The socket's close reaches the actor as a `CoreMsg`, so the actor's open
state changes at the same point in message order as `readyState` does in TS. A
frame handed to the writer while open is not replayed if the socket then closes
before flushing, as with `ws.send`.

Host hooks (`CoreHost`):
- `on_connect`, `on_disconnect`, `on_handshake_rejected`: sync;
- `get_computer_lifecycle_acks`: sync;
- `get_computer_lifecycle_ready_acks`: async. `emit_ready` spawns it and posts
  `ReadyAcksResolved { generation, acks }`; `ready` is sent only if the
  generation still matches, as `emitReady` does;
- `on_computer_lifecycle_receipt`: async, spawned, failures logged;
- `on_computer_control`: async, spawned as TS does; its
  `emit_upgrade_progress`/`emit_upgrade_done` post messages to the actor, which
  sends the frames;
- `on_computer_restart_reconcile`: async, spawned;
- `computer_control_via_supervisor`: a bool.
`reconcileComputerLifecycleOrigin` and `onComputerUpgradeReconcile` serve only
the dropped K upgrade path; the Rust computer does not provide them and core's
branches for them are deleted (D13).

The two credential-mint copies stay as two translations (translation-shaped
port; unifying them is a redesign). Reviewers verify both copies match their
sources.

---

## D7. Runtime drivers with in-parser I/O

**Governs:** `drivers/*.rs`, `drivers/runtime_session.rs`.

**Facts** (`research/daemon.md` §5.1–5.2; paths under `daemon/src/drivers`).
- Codex (JSON-RPC app-server), grok (ACP), and kimi (wire) write to the child's
  stdin from inside `parseLine`, and parsers keep pending-request maps. Grok
  also writes from `encodeStdinMessage`.
- Codex (`codex.ts:979`) and grok (`grok.ts:454`) defer their first request
  through `queueMicrotask` inside `spawn`; codex's also sets
  `pendingThreadRequest`. Kimi writes twice synchronously inside `spawn`
  (`kimi.ts:178-202`).
- `runtimeSession` splits stdout itself: `StringDecoder` chunks, the raw
  `stdout` chunk event first, then a split on `"\n"` only (a `"\r"` stays in the
  line), whitespace-only lines skipped, and the trailing partial line never
  flushed. Each line's events are dispatched before the next line is parsed.
  stderr is per chunk `toString().trim()`, emitted when non-empty
  (`runtimeSession.ts:172-190`).
- Model detection (`detectCodexModelsFromAppServer`, `codex.ts:1501`;
  `detectGrokModelsFromAcp`, `grok.ts:749`) spawns its own child with its own
  line splitter (trimmed lines), writes, and timeout, outside `RuntimeSession`.

**Decision.** The Rust `RuntimeDriver` trait keeps the TS method set (`probe`,
`spawn`, `parse_line`, `encode_stdin_message`, `build_system_prompt`, optional
`create_session`). `spawn`, `parse_line`, and `encode_stdin_message` each take
`&mut DriverIo`, which queues stdin writes. The driver lives in the agent record
that `CoreActor` owns (D6). After each call the actor pushes the queued writes
to the process's writer channel, in order, before it dispatches the events the
call returned; for stdout this happens line by line, as in TS. This replaces
direct `child.stdin.write` without reordering writes relative to reads.

A microtask write in `spawn` (codex, grok) runs, with the state it sets, right
after `spawn` returns and before the start continuation, which is the
observable order of the TS code. Kimi's synchronous writes are queued during
`spawn`.

The stdout splitter is the translation of `runtimeSession`'s own
(`Utf8StreamDecoder`, raw chunk event, split on `\n`, skip whitespace-only
lines, no flush of the trailing partial on exit), and stderr is chunk-trimmed.
Neither uses `BufReader::lines()` or `js::ReadlineSplitter`.

Model-detection clients are standalone async functions that own their child
process, their splitter (with the `.trim()` they apply), their writes, and their
timeout. They never touch actor state and return a value.

---

## D8. CLI transport wrappers, host kind, and the in-process CLI

**Governs:** daemon `drivers/cli_transport.rs`, `drivers/node_host_launch.rs`;
computer `service.rs`, `index.rs`, `service_upgrade_start.rs`, `__cli`.

**Facts.**
- Wrappers are wire-visible and byte-exact (bash, `.cmd`, `.ps1`, the forwarding
  guard, the `slock-daemon-generated` marker). Under a SEA host the wrapper runs
  `exec <execPath> __cli "$@"`. Credentials appear only in the wrapper's exec
  prefix (`research/daemon.md` §5.3).
- `runBundledRaftCli` sets `process.argv = [execPath, "slock", ...args]` and
  imports the CLI entry (`daemon/src/core.ts:1255-1262`), which runs
  `enforceSupportedNodeRuntime` and may `spawnSync` the current wrapper with
  `stdio: "inherit"` through `forwardManagedTransportIfNeeded`
  (`cli/src/main.ts:400`).
- `detectNodeHostKind` returns electron, then sea, then unknown, then node.
  `resolveNodeHostLaunch` throws `NodeHostUnavailableError` for sea and unknown;
  there is no PATH lookup (`daemon/src/drivers/nodeHostLaunch.ts:98-174`).
- The computer branches on `isSeaBinary()`/`isSeaEntry()` on its own
  (`computer/src/service.ts`, `index.ts`). On a SEA host
  `ensureSeaRuntimePackageDir` writes `<home>/runtime-pkg/package.json` and sets
  `PI_PACKAGE_DIR`, which every agent inherits through
  `spawnEnv = {...process.env}` (`service.ts:361-374`,
  `cliTransport.ts:820-834`).
- The production TS computer is a SEA whose bundler sets `import.meta.url` to
  the executable's file URL (`computer/scripts/native/build.mjs:198-206`), so
  `resolveOpencliBinPath`'s `createRequire(…).resolve("@jackwener/opencli")`
  searches from the executable's directory (`cliTransport.ts:307-347`).

**Decision.** The Rust computer is the TS SEA host. Every host-kind branch takes
the SEA side, except where that side belongs to the dropped K upgrade
machinery.
- `isSeaBinary()` and `isSeaEntry()` are true:
  - `buildResidentSpawn`: command `node_exec_path()` (guide §7), args
    `[mode, serverId?]`, no execArgv;
  - `resolveResidentSlockCliPath`: `"__cli"`;
  - `ensureSeaRuntimePackageDir`: kept, because agents of kept runtimes see the
    file and the variable;
  - `buildSelfExecArgv`: `[node_exec_path()]`; `runEntryMainGuard` always runs;
  - `spawnDetachedService`/`spawnChild`: `resolveKResidentBinary` is dropped and
    the binary is `node_exec_path()` (D11);
  - the K recovery spawn in `reconcile`, `dispatchToKResident`, and
    `stripForwardedCarrierName`: dropped (D13);
  - `serviceUpgradeStart`: its SEA branch starts the dropped K coordinator, so
    only the `UPGRADE_START_REJECTED` / `K_COORDINATOR_SEA_ONLY` rejection
    remains, unconditionally (D13);
  - the `upgrade` command's SEA check: the command is not registered (D13).
- `detectNodeHostKind()` returns `"sea"`:
  - `cliTransport`: `slock` wrappers run `<node_exec_path()> __cli`, and the
    opencli wrapper's command is `node_exec_path()`, as TS does for non-node
    hosts;
  - `resolveNodeHostLaunch` always raises `NodeHostUnavailableError` with the
    TS sea message. Codex rejects only its `npm_global` candidate, with the TS
    diagnostic; gemini's Windows npm entry and opencode's non-`.exe` entries
    fail to launch with that error;
  - `codex.ts:957` traces `host_kind: "sea"`.
- `resolveOpencliBinPath` translates Node's CommonJS lookup of
  `@jackwener/opencli` from `node_exec_path()`'s directory: `node_modules`
  directories upward, then `NODE_PATH` entries, then `$HOME/.node_modules`,
  `$HOME/.node_libraries`, and `<exe>/../../lib/node`. Within the package it
  resolves the main entry as `require.resolve` does (`exports` with the
  `require`, `node`, and `default` conditions, else `main`, else `index.js`).
  The walk to `package.json` and the `bin` read are translated as written.

The `slock` wrappers are still written, because the daemon writes them for
agents and agents may call `slock`; dropping the CLI alias only means the Rust
package ships no `slock` binary. `upgradeExistingAgentWrappers` (pre-marker
wrappers) and `regenerateExistingOpencliWrappers` are kept: they rewrite files
agents use and are not migration of Raft state. The wrapper bodies are produced
by the same translated templates and golden-tested against the oracle's
`cliTransport` output.

`raft-computer __cli <args>` builds its own current-thread tokio runtime, as
`raft` does, and calls `raft_cli::run` with argv matching `runBundledRaftCli`
(`[exe, "slock", ...args]`, program name `raft`) and the real process stdio, so
a `forwardManagedTransportIfNeeded` child inherits the real file descriptors;
the exit code is the child's `status ?? 1`, as in the source.
`enforceSupportedNodeRuntime` is not translated: the TS SEA embeds Node
24.15.0, so the check can never fire. Its removal is listed in the review
notes, and its tests are waived as `packaging`.

---

## D9. Test porting and parity accounting

**Governs:** all `*_tests.rs`, `tools/test-parity`, `tests-waived.md`, and
`tests/golden/`.

**Decision.** As in `mapping-guide.md` §1.6 and §12. The parity tool takes the
upstream test list from the oracle, not from parsing source:
`node --test --test-reporter=tap` for node:test packages and `vitest list --json`
for vitest packages, run on each OS, so template-literal and loop-generated
titles are enumerated. A test is keyed by file + describe path + title. A test
that upstream skips on an OS (`{ skip: … }`, `skipIf`, `inSourceSnapshot`) is
ported with the same condition as
`#[cfg_attr(<cfg>, ignore = "upstream skip: <reason>")]` and counted as
`upstream-skipped` on that OS. A test belonging to a dropped feature is waived
with that feature's name. A test that checks TS source text or packaging is
waived as `source-scan` or `packaging`. Every other test is ported.

**Skip kinds (clarification).** vitest's JSON output carries no skip reason, so
each upstream skip in scope gets a kind in `docs/migration/scope/rules.json`
`upstreamSkips`, read from the upstream `skip`/`skipIf` expression:
- `platform` (a `process.platform`/OS condition): ported with
  `#[cfg_attr(<cfg>, ignore = "upstream skip: <reason>")]` as above.
- `opt-in` (an env var such as `RUN_CLAUDE_INTEGRATION_TESTS`,
  `RUN_CODEX_INTEGRATION_TESTS`, `RUN_GROK_INTEGRATION_TESTS`,
  `TASK695_FAILURE_PROBE`): no ignore attribute. The Rust test reads the same
  variable at runtime with the same test (`=== "1"` stays `== "1"`) and returns
  early when it does not hold, so setting the variable runs it on every OS.
  It is counted as ported, not as upstream-skipped.
- `always` (an unconditional `test.skip`): `#[ignore = "upstream skip: <reason>"]`.
`tools/test-parity` enforces the attribute or the runtime check per kind.

Golden capture scripts live in `tools/golden/` and are rerunnable against the
oracle. The orchestrator runs them (README "Work units and roles").

---

## D10. Versions and identity strings

**Governs:** every crate's `Cargo.toml` version and every place that reports a
version or client name.

**Decision.** Crate versions equal the upstream package versions (cli 0.0.24,
daemon 1.0.25, computer 1.0.28, oar 0.0.7). Baked version strings such as
`BUNDLED_DAEMON_VERSION`, `BUNDLED_CLI_VERSION`, `COMPUTER_VERSION`, and
`Raft CLI: 0.0.24` use those numbers. Protocol identity strings stay as the
source has them (`slock-daemon`/`1.0.0` in clientInfo, `X-Raft-Client: cli`,
`daemon-server-session-worker`, and so on).

`readCliVersion()` always returns the baked version, so `main.ts`'s
`cliVersion === "unknown"` branch (its `-V` option and `option:version`
listener) is unreachable; it is deleted and listed in the review notes.
`raft-computer __build-versions` (`computer/src/cli.ts:879-887`) is kept and
prints `{"computerVersion":"1.0.28","daemonVersion":"1.0.25","cliVersion":"0.0.24"}`
followed by a newline.

---

## D11. Computer service process model

**Governs:** `raft-computer` `service.rs`, `start_stop.rs`, `services/start.rs`,
`services/stop.rs`, `service_control.rs`, `macos_login_carrier.rs`, `cleanup.rs`,
`reset.rs`, `os_supervisor_lifecycle.rs`, `cli.rs` (`__service`).

**Facts** (`research/computer-service.md` §0, §2). Upstream 1.0.28 runs no OS
service manager. `__service` is a detached, self-spawned process tracked by
`computer/run/service.pid`. `stop` sends it SIGTERM (`TerminateProcess` on
Windows). Restart is an IPC self-handoff: the live service spawns a replacement,
waits until it attests the same managed set, then exits. macOS alone has a login
LaunchAgent, `build.raft.computer.login.<hash16>` (`RunAtLoad`, no
`KeepAlive`), with an owner marker that can hand the job to Raft Desktop. The
launchd, systemd, and scheduled-task definitions in `osSupervisor*.ts` exist
only to retire old installs; their entries still arrive as `__service
--os-supervised <kind> --slock-home <path>`, which `bootstrapThenRun` refuses
before anything else (`computer/src/index.ts:369-378`,
`osSupervisorLifecycle.ts:16-50`). The supervisor reconciles once at startup,
then on a 5 s interval and on independent `scheduleReconcile` timeouts
(`serviceReconcileLoop.ts`, `service.ts:1068-1070`).

**Decision.** Port the model that exists.
- Keep: the detached `__service`, `service.pid` semantics, the SIGTERM stop,
  the IPC self-restart handoff with its timeouts, and the macOS login carrier
  with an identical label, plist bytes, marker files, and the
  enable/disable readback. Drop only the K resume-deadline budget and
  `refreshCliLoginCarrierIfOwned`.
- Do not add systemd units, Windows tasks, or `KeepAlive`. Any of them would
  fight the self-restart handoff.
- Every binary location is `node_exec_path()` (guide §7): the service spawn,
  the runner `__run` spawn, and the `__cli` wrapper. The plist
  `ProgramArguments[0]` comes from `resolveStableDispatcherPath`, translated as
  written, including the `RAFT_COMPUTER_DISPATCHER_PATH` override and the
  refusal of a path under `<home>/computer/k/`.
- Legacy supervisor entries: `parseLegacyOsSupervisorInvocation` and its refusal
  line `raft-computer: retired_os_supervisor_entry_ignored kind=<kind>\n`
  (stderr, no service started) are translated exactly and kept despite the
  legacy-migration drop. Other shapes reach `__service`, which validates
  `--os-supervised` against `launchd-user`, `systemd-user`, and `windows-task`
  (`invalid OS supervisor kind: <kind>` otherwise), writes
  `RAFT_COMPUTER_OS_SUPERVISOR_KIND`, and applies `--slock-home`/`--raft-home`
  (`cli.ts:784-799`), all translated exactly. D14 reads the same kind.
- Spawns go through `raft_shared::process::spawn` (guide §9), which reproduces
  libuv's flags. The service (`detached: true, windowsHide: true`, stdio = the
  `service.log` fd): on Unix `setsid`; on Windows
  `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` with `STARTF_USESHOWWINDOW` +
  `SW_HIDE`, and no `CREATE_NO_WINDOW`, because libuv adds it only when no stdio
  slot is an inherited fd. The `__run` child (`windowsHide: true`, not detached,
  stdio = its log fd): `SW_HIDE` only.
- Windows keeps the upstream behavior. SIGTERM is a hard terminate, runners
  outlive the service and are adopted by the next one, and no Job Object is
  used.
- The supervisor is one task that owns every `RunnerRecord` (D6 style). A record
  is marked `starting` before any await in `spawn_child`, as in the source. The
  schedule is the TS one: one `reconcile` at startup, a 5 s interval, and
  independent `scheduleReconcile(ms)` timeouts, each posting a `Reconcile`
  message. Overlapping reconciles are allowed and guarded, as in TS, by
  `starting` and `canSpawn`. Awaits inside reconcile, the child-exit handler,
  and the restart handoff are continuations (D6), so the task keeps serving IPC
  during them.
- Installers (`install.sh`, `install.ps1`, SEA build, Hands release lookup) are
  not ported. Distribution is prebuilt binaries from this repo's releases.

---

## D12. Cross-implementation interop: IPC, locks, and state files

**Governs:** `internal/ipc_server.rs`, `internal/ipc_codec.rs`,
`lib/ipc_client.rs`, `service_ipc_seam.rs`, `concurrency.rs`,
`lifecycle_operations.rs`, `machine_operation_store.rs`, `paths.rs`, daemon
`machine_lock.rs`, and every state-file reader and writer.

**Facts** (`research/computer-core.md` §1 "Electron co-tenant", §2, §3, §7).
Raft Desktop embeds the TypeScript computer (`runService`/`runResident`) and
talks to the service over the same IPC. A Rust CLI can therefore meet a
TypeScript service on the same `RAFT_HOME`, and the reverse. The only guard is
the version-string comparison `SERVICE_VERSION_SKEW`.

**Decision.** Co-existence with Raft Desktop's TypeScript service is required
(D21). Keep full wire and disk interop with upstream 1.0.28.
- IPC is byte-compatible: a u32 big-endian length, UTF-8 JSON, a 1 MiB cap, the
  same hello/request/response/event/cancel frames and error codes, the same
  socket path and modes (`run/service.sock`, dir 0700, socket 0600), the stale
  probe, and the Windows pipe name `raft-computer-<sha256(computerDir)[:16]>`
  with the default ACL. Pipe instances use libuv's `CreateNamedPipeW` flags:
  `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance only and no
  `PIPE_REJECT_REMOTE_CLIENTS`, so tokio's `ServerOptions` sets
  `first_pipe_instance(true)` only for the first instance and
  `reject_remote_clients(false)`. Check these against libuv's `src/win/pipe.c`
  in Node 24.15.0 before implementing. The restart handoff (`releaseListener`
  in `internal/ipc-server.ts`) is golden-tested on the Windows leg.
- The mutation locks reimplement `proper-lockfile` 4.1.2 exactly. Sites and
  options:
  - `computer/.lock`: lock target `computerDir`, `lockfilePath`
    `<computerDir>/.lock`, stale 60 s unless the caller overrides it, update as
    given, retries 10 / min 200 ms / max 800 ms / factor 1.5
    (`concurrency.ts:80-116`);
  - `<server>/lifecycle-operations.json.mutation.lock`: stale 60 s, retries
    10 / 20 ms / 200 ms / 1.5 (`lifecycleOperations.ts:101-125`);
  - the machine operation store's `<file>.lock`: stale 30 s, update 10 s,
    retries 50 / 10 ms / 250 ms / 1.2 (`machineOperationStore.ts:209-215`);
  - the `cleanup.ts:365` reclaim: stale 60 s, retries 0, with `onCompromised`.
  Behaviors: an mkdir lock directory; `stale` clamped to at least 2000 ms and
  `update` to [1000, stale/2] (default stale/2); the mtime-precision probe
  (seconds vs milliseconds); an mtime refresh every `update` ms; compromise
  when the mtime is not ours or the refresh gets ENOENT; retries through `retry`
  0.12 on any error; no pid check; and the `signal-exit` handler that removes
  held lock directories on exit and on signals, so an interrupted command
  leaves no lock behind. Golden-test it by running a TS lock holder (oracle)
  against the Rust contender, and the reverse.
- The daemon machine lock (`daemon/src/machineLock.ts`) is a separate protocol:
  a `daemon.lock` directory holding `owner.json` (`schemaVersion: 2`, token,
  pid; release rewrites pid to 0), a 30 s incomplete-lock stale rule, owner
  death only on ESRCH, and the `Another Slock daemon is already running …`
  text. It is the only exclusion between a TS and a Rust runner on one home. It
  is translated exactly and golden-tested the same way.
- Every state file keeps its exact serializer: pretty or compact, trailing
  newline or not, key order (struct field order), and modes. That includes the
  `attach` write without `schemaVersion`. The attach file is written in place
  (`write` then `chmod 0600`), as `services/attach.ts` does. A temp file plus
  rename is not used (D21).
- `RAFT_COMPUTER_PARENT_MUTATION_LOCK_HELD` and `…_SOURCE_SERVICE_PID`
  propagation, and their removal in `runnerChildEnv`, are translated exactly.

---

## D13. Dropped update surface: what remains observable

**Governs:** `cli.rs`, `status.rs`, `doctor.rs`, `resident_lifecycle_bridge.rs`,
`lifecycle_operations.rs`, `service_ipc_seam.rs`, `service_upgrade_start.rs`,
`service_control.rs`, `service.rs` (the resident core factory), and
DaemonCore's `computer:upgrade` handling.

**Facts.** DaemonCore does not answer `computer:upgrade` itself. It dedupes by
durable lifecycle acks and `handledComputerControlOperationIds`, calls
`onComputerControl` detached, maps failures to `control_busy`,
`self_relaunch_unavailable`, or `computer_control_failed`, and replies only when
a `requestId` exists (`daemon/src/core.ts:4102-4173`). The computer's hook
enqueues a lifecycle operation with `shutdown` and `ready` phases, then relays
to the service's `upgrade-start` IPC (`computer/src/service.ts:469-494`). A
non-SEA service rejects that with `UPGRADE_START_REJECTED` and
`describeUpgradeStartRejection("K_COORDINATOR_SEA_ONLY")`
(`serviceUpgradeStart.ts:67-72`), and `requestServiceUpgradeViaIpc` turns the
rejection into `computer:upgrade:done {ok:false, error: <message>}`
(`serviceControl.ts:264-303`). An upgrade operation's `ready` phase binds only
to a K receipt (`residentLifecycleBridge.ts:101-137`).
`retireCompletedUpgradeShutdownsFromLog` removes only the `shutdown` phase of
upgrade operations whose `requestId` has an `ok` line in `upgrade.log`
(`lifecycleOperations.ts:273-291`).

**Decision.**
- The `upgrade`, `versions`, `operation`, `channel`, `__k-upgrade`,
  `__installer-converge`, `__legacy-supervisor-takeover`, and `__supervisor`
  commands are not registered. Invoking one gives commander's normal
  unknown-command error. They are not stubbed. `raft-computer --help` therefore
  lacks the `channel`, `operation`, and `upgrade` lines; README gate 3 compares
  against the oracle output with exactly those lines removed.
- DaemonCore's `computer:restart`/`computer:upgrade` handling and the
  computer's `onComputerControl` hook are straight translations. The IPC
  method `upgrade-start` stays registered, and its only remaining branch is the
  `K_COORDINATOR_SEA_ONLY` rejection, so the server receives the same
  `computer:upgrade:done` a non-SEA TS service sends. The enqueued operation's
  `shutdown` phase is reported and receipted as usual; its `ready` phase stays
  pending, as on a non-SEA TS service.
- `computer:supervisor-mutations-v1` is advertised under the same condition as
  TS (`computerControlViaSupervisor`, from `supervisorMutationsAttested`).
- `status` prints `Upgrade: none in flight`. The doctor's K receipt check is
  removed, and the list of doctor checks shrinks accordingly.
- `retireCompletedUpgradeShutdownsFromLog` is translated as is; only an earlier
  TypeScript install writes `upgrade.log`. There is no other retirement rule.
- The release channel file `computer/channel` is not read or written.
- K hooks in kept code:
  - `reconcileComputerLifecycleOrigin` (`adoptLegacyKUpgradeOrigin`): not
    provided, and core's branch is deleted. A K receipt left by an earlier TS
    SEA install is not adopted, and `ready` is not replayed for it.
  - `onComputerUpgradeReconcile` (`reconcileKUpgradeOnConnect`): not provided,
    and core's branch is deleted. No `computer:upgrade:done` is sent after
    connect; the Rust computer never swaps binaries.
  - `getReadyAcknowledgements`: `loadOperation` is replaced by "no K
    operation", so upgrade `ready` acks are omitted, as when TS finds no K
    state. Restart acks are unchanged.
  - `acknowledgeReceipt`: `acknowledgeKReadyReceipt` is dropped;
    `acknowledgeLifecycleReceipt` still runs.
  - `readKRunnerHold`: kept. A hold file written by a TS K upgrade on a shared
    home still holds runner spawns.
  - `resolveKResidentBinary`, the K recovery spawn in `reconcile`,
    `dispatchToKResident`, and `stripForwardedCarrierName`: dropped (D8, D11).
  - `resolveStableDispatcherPath` with `RAFT_COMPUTER_DISPATCHER_PATH`: kept
    (D11).

---

## D14. Entry sequence and shell environment capture

**Governs:** `raft-computer` `main.rs`, `shell_env_capture.rs`, `index.rs`.

**Facts** (`research/computer-core.md` §1 entry seam, §5;
`computer/src/index.ts`, `shellEnvCapture.ts`).
- `index.ts` checks `process.argv.includes("__print-env")` before anything
  else, so that word anywhere in argv selects print-env mode (exit 0 after the
  frame is written, 8 on a socket error).
- `bootstrapThenRun` runs, in order: the legacy-entry refusal (D11), K dispatch
  and carrier-name repair (dropped, D13), `bootstrapSupervisedServiceEnv`, then
  the CLI.
- `bootstrapSupervisedServiceEnv` captures only for an argv containing
  `__service` whose kind (the `--os-supervised` value, else
  `RAFT_COMPUTER_OS_SUPERVISOR_KIND`) is `launchd-user` or `systemd-user`. Every
  other invocation keeps the inherited environment. When it runs, it first sets
  `SLOCK_HOME` from `--slock-home`/`--raft-home` and sets the kind variable. On
  success it applies the capture replace-not-merge (keys absent from the capture
  are deleted, the capture is applied, protected keys are restored) and sets
  `RAFT_COMPUTER_SHELL_ENV_STATE=inherited`. On failure it sets
  `unavailable:<code>` and writes its fixed stderr message.
- Capture is POSIX-only, with the passwd shell restricted to zsh, bash, sh,
  dash, and ksh: a unix-socket server in a temp dir, `<shell> -i -l -c <cmd>` in
  a new process group with stdio ignored, a 10 s timeout, a 1 MiB cap, strict
  nonce framing, a nonzero exit that wins over a valid frame, and `killGroup`:
  SIGTERM to the group, then an unref'd SIGKILL 1 s later if the SIGTERM was
  delivered.

**Decision.** `main()` builds TS's `process.argv` (`[exe, exe, ...args]`, the SEA
shape the source indexes into), then:
1. if any element equals `__print-env`, runs print-env mode and exits;
2. runs the legacy-entry refusal (D11);
3. evaluates the capture gate exactly. When it passes, the capture runs
   synchronously on the main thread before the tokio runtime or any other
   thread exists: a single-threaded loop over `poll(2)` on the listening
   socket and the accepted connection, reaping the shell with `waitpid(WNOHANG)`,
   with the same timeout, cap, framing, and outcome rules. The group SIGTERM is
   sent inside the loop. Start-up does not wait for the SIGKILL grace: its
   deadline is handed to the runtime, which arms a task for it, as the unref'd
   TS timer fires later in the service's life;
4. applies the result to the process environment with `std::env::set_var` and
   `remove_var` (replace-not-merge, protected keys restored, `SLOCK_HOME`, the
   kind, and `RAFT_COMPUTER_SHELL_ENV_STATE` set as TS does) and writes the
   failure message. This is the only point where they are sound;
5. creates the runtime and builds `ProcessEnv` (guide §7) from the process
   environment.

Later `process.env` mutations in the source mutate `ProcessEnv` only.

---

## D15. JSON Schema validation: port the used subset of Ajv 2020

**Governs:** `raft-cli` `commands/integration/manifest_v1.rs`,
`commands/integration/action_v1.rs`; `raft_shared::json_schema`.

**Facts.** `manifestV1.ts:504-512` and `compileAgentManifestSchemaV1`
(`:894-900`) use `new Ajv2020({ allErrors: false, strict: false,
validateSchema: true, formats: {} })` from ajv 8.20.0 (the version pnpm resolves
for the cli). `actionV1.ts:177-196` prints `errors[0].instancePath` (through
`boundedSchemaErrorPath`) and `errors[0].message` sliced to 256.
`manifestV1.ts:513` prints the `ajv.compile` exception
(`schema is invalid: data/… must be …`). With `formats: {}` every `format`
keyword is ignored and Ajv logs its unknown-format warning. The `jsonschema`
crate validates formats, orders and words errors differently, and its `pattern`
is not ECMAScript.

**Decision.** Port the used subset of Ajv 8.20.0's 2020-12 validator into
`raft_shared::json_schema` as a translation: the keywords the bounded manifest
schemas can contain, Ajv's keyword evaluation order, `allErrors: false`
first-error semantics, its English message templates and `instancePath`
format, meta-schema validation with its compile-error message, the
`formats: {}` behavior including the warning, and `pattern` compiled with
`JsRegex` and the `u` flag. Proof: oracle goldens from `manifestV1.test.ts` and
`actionV1.test.ts`, plus a corpus of valid and invalid schemas and payloads. The
`jsonschema` crate is not used.

---

## D16. Agent migration is in scope

**Governs:** `raft migrate` (`export`, `import`, `status`, `ready`, `arrived`)
and `packages/daemon/src/agentMigration*.ts`.

`raft migrate` is a live server feature. Translate the command and the seven
daemon modules, including `machine:migration_transport:lease`. Help text matches
the oracle, including the `migrate` line on `raft --help`.

## D17. Antigravity driver is not registered

**Governs:** `drivers/index.ts`.

Do not port `drivers/antigravity.deprecated.ts`. The factory map omits
`antigravity` along with the already-dropped `builtin`, `kimi-sdk`, and `pi`.
`getDriver("antigravity")` throws
`Unknown runtime: antigravity. Available: claude, codex, grok, copilot, cursor, gemini, kimi, opencode`
(insertion order of the remaining factories). Existing antigravity agents cannot
be resumed.

## D18. Trace bundle upload is in scope

**Governs:** `packages/daemon/src/traceBundleUpload.ts`.

Translate it as upstream: fail-open, same destination, same env disable, same
`machines/<lockId>/trace-uploads/<file>.uploaded.json` records.

## D19. Computer legacy read fallbacks are not ported

**Governs:** `serverState.ts`, `paths.ts`, `serverUrl.ts`, and every caller of
the fallback readers.

Three behaviors are deleted. Everything else in those files stays.

- `readServerAttachment` reads only `runner.state.json`. It does not read
  `attachment.json`, does not precedence-merge, and does not migrate-on-read.
  A home whose only credential is `attachment.json` reads as absent.
- Readers use `runner.pid` and `runner.log` only. `serverRunnerPidReadFallback`
  and `serverRunnerLogReadFallback` are not ported, so a live `server-runner.pid`
  or `server-runner.log` is invisible to status, doctor, and reconcile.
- `canonicalizeServerUrl` does not rewrite `https://api.slock.ai` to
  `https://api.raft.build`, and the staging rewrite does not map
  `https://slock-server-staging.fly.dev` to the AWS staging URL. Stored and
  configured URLs are used as trimmed, with only the trailing-slash strip.
  `SLOCK_SERVER_URL` / `RAFT_SERVER_URL` resolution stays. Hostname
  classification in the daemon (`api.slock.ai` as its own target class) stays;
  this decision is about computer URL rewriting only.

## D20. Markerless Desktop guard stays

**Governs:** `assertNoMarkerlessLegacyDesktop`.

This is the exception to the dropped-legacy scope. While
`/Applications/Raft Computer.app` exists without a marker, login-carrier
convergence refuses, with upstream's message. The guard is what stops a second
service next to the old Desktop app.

## D21. Co-existence and the attach write

Raft Desktop's TypeScript service and this Rust port share one home. D12
stands: IPC, `proper-lockfile`, the daemon machine lock, and state files match
what that service reads and writes.

The attach state file is written in place, then `chmod 0600`, matching
`services/attach.ts`. Do not write a temp file and rename it.

## D22. Setup and doctor without legacy migration

**Governs:** `setup.rs`, `doctor.rs`, `doctor_cli.rs`, `cli.rs` (the `setup`
and `doctor` commands).

**Facts.** This records the observable effect of README "Dropped: Legacy
migration and adoption" on two kept commands. It adds no new drop.

- `setupCore` injects legacy detection, the legacy-machines roster, three
  pickers, adopt-by-fingerprint, adopt-by-daemon-id, and forced migration
  diagnostics (`computer/src/setup.ts:1238-1254`). `--machine <id>` adopts a
  server row by id (`setup.ts:1360-1390`, `SETUP_MACHINE_INVALID`) and is
  refused next to an existing attachment (`setup.ts:1337-1342`,
  `SETUP_MACHINE_ALREADY_ATTACHED`). Without `--machine`, setup runs discovery
  and the pickers (`setup.ts:1392-1656`), which print `Migration:` lines and
  honour `--fresh` and `--verbose`. When nothing is adopted, setup attaches
  fresh (`setup.ts:1659-1662`); this is the whole path when detection reports
  `no_local_evidence`. Forced scrubbed diagnostics run after an adoption
  (`setup.ts:1668-1720`). The unlinked-runner recovery archives state and
  loops back to the attachment decision (`setup.ts:1686-1694`).
- `doctor` adds an `identity <server>` check from legacy detection: the
  zero-match setup blocker and the regret switch
  (`computer/src/doctor.ts:246-263`, `setupBlockingIdentityDetail`,
  `regretSwitchDetail`). `doctor --migration-details` prints local legacy
  evidence instead of the report (`computer/src/cli.ts:466-475`,
  `doctorCli.ts:179-290`).

**Effect in the port.**
- Setup never runs legacy detection, fetches the legacy roster, prompts a
  picker, adopts, or forces migration diagnostics. With no attachment it goes
  straight to the fresh attach of `setup.ts:1659-1662`, then start, printing no
  `Migration:` line. `SETUP_MACHINE_INVALID` and
  `SETUP_MACHINE_ALREADY_ATTACHED` are never produced. The unlinked-runner
  recovery still archives the stale state and loops back, which now leads to a
  fresh attach.
- Doctor never adds an `identity <server>` check; the list of doctor checks
  shrinks accordingly (as for the K receipt check in D13).
- `services/diagnosticsPush.ts` stays: `lib/api.ts` exposes it (README keeps
  `lib/api.ts` and its closure). Only setup's migration callers go.
- `setup --machine <machineId>`, `setup --fresh`, `setup --verbose` and
  `doctor --migration-details` are not registered (user decision). They are
  not stubbed. `raft-computer setup --help` and `raft-computer doctor --help`
  therefore lack those option lines, the same way D13 removes the `channel`,
  `operation` and `upgrade` lines from `raft-computer --help`; golden help
  comparisons use the oracle output with exactly those lines removed. Passing
  one of them gives commander's `error: unknown option '--machine'` (with the
  option as typed) and exit code 1.
- Tests: the cases of `setup.test.ts` and `doctor.test.ts` that exercise the
  paths or options above are waived per case in `tests-waived.md` as
  `dropped`. Ported cases that injected the detection seam keep their other
  assertions; each left-out assertion has its own `:: assert` row there.
