# Round 1 review — reviewer A (mapping guide; D1–D5, D9, D10)

Paths: `UP` = `upstream/raft-source/packages`, `UND` =
`upstream/raft-source/node_modules/.pnpm/undici@7.24.7/node_modules/undici/lib`,
`CMD` = `UP/cli/node_modules/commander/lib`. Line numbers are from the pinned
sources. "Verified" means the behavior was reproduced with the oracle or with
Node v24.15.0 / zod 4.3.6 from the upstream `node_modules`.

---

## 1. The HTTP default-header rule is backwards; undici adds headers the Rust port must add too
**Severity:** blocker
**Artifact:** mapping-guide §10, bullet "Headers" (lines 251–254); D10 (identity strings)

**Failing behavior.** The guide says "Do not add `User-Agent` or other defaults
the source did not send … `reqwest` adds none by default." Both halves are
wrong:
- The source does send defaults, because undici's fetch adds them to every
  request that doesn't set them: `accept: */*` (`UND/web/fetch/index.js:478-496`),
  `accept-language: *` (`:501-503`), `sec-fetch-mode: cors`
  (`UND/web/fetch/util.js:246`), `user-agent` (`index.js:1465-1466`), and
  `accept-encoding: gzip, deflate` on http or `br, gzip, deflate` on https
  (`:1520-1525`). The HTTP/1 writer also emits `host:` and `connection: keep-alive`
  (`UND/dispatcher/client-h1.js:1124-1132`).
- The User-Agent value depends on the call site (`index.js:76`). Node's global
  `fetch` sends `node`. The npm `undici` `fetch` sends `undici`. Both kinds are
  used:
  - Global `fetch` (`node`): `UP/cli/src/client.ts:272`,
    `UP/cli/src/proxy.ts:215`.
  - npm `undici` (`undici`): `UP/cli/src/agentLogin/deviceAuthClient.ts:26`,
    `UP/cli/src/commands/agent/login.ts:16`, `agent/list.ts:30`,
    `UP/computer/src/proxy.ts:7`, `UP/computer/src/apiClient.ts:5`,
    `UP/daemon/src/daemonFetch.ts:1`.
- reqwest's `ClientBuilder` adds `accept: */*` by default. Its
  `accept-encoding` and decompression behavior depend on the enabled features.
  §13 enables no gzip/brotli/deflate feature.
- undici transparently decodes gzip/br/deflate (and zstd,
  `UND/web/fetch/index.js:2228-2247`). `UP/daemon/src/agentCredentialProxy.ts:393-398`
  relies on this: it strips `content-encoding` because "Node fetch transparently
  decodes gzip/br responses". A reqwest client that sends `accept-encoding`
  without decoding forwards compressed bytes with the encoding header removed,
  which corrupts every proxied response. A client that doesn't send it produces
  different request bytes.

**Correction.** Replace the rule with this: every translated fetch call sends
exactly what undici would send. Add a `raft_shared::http::FetchFlavor::{NodeGlobal, NpmUndici}`
per call site, which decides the `user-agent` value. Default-insert `accept`,
`accept-language`, `sec-fetch-mode`, and `accept-encoding` (value keyed on the
URL scheme) only when the caller didn't set them. Enable reqwest `gzip`, `brotli`,
`deflate` (and `zstd`) so decoding matches. Call `.default_headers(HeaderMap::new())`
and remove reqwest's own `accept`. Add a golden test that captures raw request
bytes from the oracle and from Rust against a local socket, for each call-site
flavor.

## 2. reqwest negotiates HTTP/2, and undici fetch never does
**Severity:** blocker
**Artifact:** mapping-guide §10 bullet "HTTP client"; §13 (reqwest features)

**Failing behavior.** reqwest 0.12's default features include `http2`, and with
rustls it offers `h2` in ALPN. undici 7 fetch uses HTTP/1.1 unless `allowH2` is
set. No call site in `UP/{cli,daemon,computer}/src` sets it. Against raft.build
(or any CDN that supports h2), every Rust request would use a different
protocol, framing, and header encoding (HPACK, no `connection`/`host` headers).

**Correction.** Mandate `ClientBuilder::http1_only()` and build reqwest with
`default-features = false` plus an explicit feature list, so `http2`,
`default-tls` (native-tls, which contradicts "rustls"), and
`macos-system-configuration` are off.

## 3. Rust `String` cannot represent JS strings, so the UTF-16 helpers can't have `&str` → `String` signatures
**Severity:** blocker
**Artifact:** mapping-guide §6.1, §6.3 ("lone surrogates as `\udxxx`"); §3 row `string` → `String`; D3

**Failing behavior.** `utf16_slice` cuts in code units, and cutting inside a
surrogate pair yields a lone surrogate, which a Rust `String` cannot hold.
Verified: `"a😀b".slice(0,2)` → `JSON.stringify` gives `"a\ud83d"`, while
`process.stdout.write` emits `a` + `EF BF BD` (U+FFFD). The same value produces
different bytes on the two sinks. Real truncation sites that hit this:
- `UP/shared/src/actionCards.ts:284` (`rendered.slice(0, 497)` in action-card
  presentation items, which go on the wire through `JSON.stringify`);
- `UP/cli/src/proxy.ts:95` (`.slice(0, 80)` on error codes);
- `UP/cli/src/commands/integration/manifestV1.ts:907`, `actionV1.ts:180`
  (`slice(0, 253)` / `slice(0, 256)`);
- the 40/80/240/500/4000 limits D3 lists.

§6.3 promises that `json_stringify` escapes lone surrogates, but a
`serde_json::Value` built from `String` can never contain one, so the promise
cannot be kept. JSON input can also carry `"\ud800"` escapes: V8 accepts them,
and `serde_json` rejects them with "lone leading surrogate".

**Correction.** Pick a representation and state it in D3. Either:
- `js::JsString` (`Vec<u16>` or WTF-8) for any value that is sliced by UTF-16
  index and later serialized, with `json_stringify` taking a JS-value model
  (`js::Value`) that has `JsString` leaves; or
- a documented rule that each slice site is analyzed and emits U+FFFD for stdout
  sinks and `\udxxx` for `JSON.stringify` sinks, with `utf16_slice` returning an
  enum that carries a dangling-surrogate marker.

Add golden cases for both sinks. Also require a JSON parser that accepts lone
surrogate escapes (see finding 8).

## 4. Gate 3 asks for `raft-computer --help` to match the oracle, but D13 removes visible commands
**Severity:** major
**Artifact:** README "Gates" item 3 vs decisions D13

**Failing behavior.** Oracle `tools/oracle.sh computer --help` lists `channel`,
`operation`, and `upgrade` (with descriptions) in "Commands:". D13 deregisters
them, so the Rust help text differs from the oracle by design, and the column
width may change too. Commander pads to the longest term
(`CMD/help.js:371,377`). The longest term, `restart [options] [serverSlug]`,
survives, so padding happens to stay the same. The lines themselves still
differ, and gate 3 as written can never pass.

**Correction.** Change gate 3 to "matches the oracle except the lines D13
removes", and store the expected Rust golden with a diff against the oracle
golden that is reviewed once. Alternatively, keep the three commands registered
as visible and have them fail with a fixed error. D13 currently forbids that
("not stubbed"), so pick one.

## 5. D1's commander subset omits APIs and behaviors both CLIs depend on
**Severity:** major
**Artifact:** decisions D1 ("It must include … Features neither CLI uses … `.alias()` … are not ported")

**Violated source facts** (all in non-test source):
- **Negatable options.** `--no-start` (`UP/computer/src/cli.ts:333,360`) implies
  a default of `start: true` and help term `--no-start`. D1 doesn't list
  negatable options.
- **Hidden commands.** `.command("__service", { hidden: true })` and similar at
  `computer/src/cli.ts:784,801,807,815,850,862,864`. D1 lists only hidden
  *options*.
- **`.alias()` getter.** D1 says `.alias()` isn't ported, but
  `UP/cli/src/main.ts:134,146` call `candidate.alias()` to build the
  `Next action:` target. It must exist as a getter returning `None`.
- **Introspection.** `cursor.commands` (`main.ts:134,146`),
  `cursor.createHelp().visibleCommands(cursor)` (`main.ts:150-154`),
  `program.opts()` (`main.ts:238`), `CommanderError.code` / `.exitCode`
  (`main.ts:162,381-382`), and `computer/src/cliServerArgContract.test.ts:40`
  (`current.commands.find(c => c.name() === part)`) all need public Rust
  equivalents.
- **Option event.** `program.on("option:version", …)` (`main.ts:208`, on the
  unknown-version branch). The branch is either ported or explicitly declared
  dead under D10's baked version. The guide's §1.5 "dropped code" rule doesn't
  cover code that is unreachable but not part of a dropped feature.
- **Help width per stream.** `helpInformation` uses stdout columns for `--help`
  and stderr columns for error help (`CMD/command.js:61-67,2257-2261`). D1 says
  only "terminal width on a TTY". The bare-group help that raft prints goes to
  stderr (verified: `raft message` writes help to stderr, then
  `Error: (outputHelp)`, exit 1).
- **`help()` exit code.** `help()` exits with `process.exitCode || 0` and forces
  1 only for error help (`CMD/command.js:2395-2405`). The port needs an
  equivalent of the ambient exit code.
- **Parent-option consumption.** A parent command consumes flags it declares
  even when they appear after the subcommand (`UP/cli/src/commands/agent/login.ts:101-117`),
  and `{ ...optsWithGlobals(), ...options }` relies on spread key order. D1
  mentions `optsWithGlobals` but not this parsing rule, which changes which
  object holds the value.
- **Custom parsers.** A custom parser receives `(value, previous)`, where
  `previous` is `undefined` on the first call. The mutating `previous.push`
  parsers (`UP/cli/src/commands/integration/login.ts:48-51`, `app.ts:717-728`,
  `invoke.ts:1785-1798`) need that.
- **No-radix `parseInt` parser.** `computer logs --lines` uses
  `(v) => Number.parseInt(v, 10)` (`computer/src/cli.ts:492`). See finding 20.
- **Wrap regex.** `Help.wrap` is a JS regex over UTF-16 code units
  (`CMD/help.js:485-517`), with non-`u` `.` and `\s` including U+FEFF. See
  finding 18.
- **`process.exit` inside commander.** `_exit` calls `process.exit`
  (`CMD/command.js:507-512`). raft-computer takes this path, which conflicts
  with mapping-guide §4.4 ("only `main` calls `std::process::exit`").

**Correction.** Replace D1's feature list with one generated from a grep of
every commander API used in `UP/cli/src` and `UP/computer/src`, including
tests that are to be ported. Add negatable options, hidden commands, the
`alias()` getter, `commands` / `createHelp().visibleCommands` / `opts`, per-stream
help width, the ambient exit code, and the parent-consumption parse rule. State
that the crate returns `CommanderError` and never exits. Add the computer parse
matrix (`--no-start`, `--lines x`, hidden commands in help) to the goldens.

## 6. D2's combinator list is incomplete, and two of its facts are wrong
**Severity:** major
**Artifact:** decisions D2 ("Governs", "Facts", "Decision" combinator list)

**Violated source facts.**
- **Combinators missing from the list but used in the governed files:**
  `.positive()` (37), `.nonnegative()` (25), `.finite()` (7), `.safe()`
  (`agentApiContract.ts:1586,1591-1592`), `.meta()` (12), `.startsWith()`,
  `z.undefined()`, `z.null()`, `z.int()`, `z.iso.datetime()`
  (`attachmentUploadContract.ts:37,62`), top-level `z.uuid()` / `z.url()`
  (`attachmentUploadContract.ts:7-8,31`, whose rules differ from
  `z.string().uuid()`), and `z.ZodIssueCode` custom issues in `superRefine`.
  `featureFlagRolloutGuardrail.ts` and `slackBridgeProvisioning.ts` also use
  `.refine(Number.isSafeInteger)` and `.url().refine(new URL…)`. Both are
  re-exported from `UP/shared/src/index.ts:214`, so the closure must be decided.
- **False fact.** "the two local ones in `drivers/codex*.ts`": neither
  `UP/daemon/src/drivers/codex.ts` nor `codexInstructionShape.ts` imports zod
  (`grep` for `zod` / `z.` finds nothing). The daemon's non-test source has no
  zod import at all.
- **Wrong ordering fact.** "`passthrough` keeps unknown keys after the known
  ones." Verified with zod 4.3.6: parsing `{z:1, a:2, "5":0, b:" hi "}` with
  `object({b,a,d:default}).passthrough()` yields
  `{"5":0,"b":"hi","a":2,"d":"x","z":1}`. Array-index keys come first
  (V8 object semantics), so the output isn't simply known-then-unknown. See
  finding 19.
- **Coercion.** `z.coerce.number()` (`agentApiContract.ts:557`, `task_number`)
  is `Number(x)`. Verified: `"0x10"`→16, `"0b11"`→3, `" 12 "`→12, `"1e3"`→1000,
  `true`→1, `null`→0 (then fails `positive`), and `"Infinity"` fails with
  `received:"Infinity"` and message "expected number, received number". The
  parsed number goes into the URL path, so the wrong coercion changes wire
  bytes (`raft task claim 0x10` hits task 16).
- **Refinement skipping.** Refinements are skipped when an earlier issue
  aborted. Verified: `object({a:string}).refine(fn)` doesn't call `fn` when `a`
  is invalid. D2's "`refine`/`superRefine` as closures" doesn't specify this.
- **Issue key order.** Verified: issue objects serialize as
  `{"expected","code","path","message"}` for `invalid_type` and
  `{"origin","code","minimum","inclusive","path","message"}` for `too_small`.
  `ZodError.message` is `JSON.stringify(issues, null, 2)` in that order. D2 says
  only "(code, path, message, plus the codes' extra fields)".

**Correction.** Generate the combinator list mechanically from the governed
files and include every item above. Delete the codex claim. Rewrite the
passthrough fact using the V8 key-order model from finding 19. Specify coerce
as `js::to_number` (hex, binary, and octal literals, whitespace trim, and
`true`/`null`), and the abort/`when` rule for refinements. Require the golden
corpus to record issue key order and to include cases that exercise coercion,
refinements after aborts, and integer-like passthrough keys.

## 7. D5's "no timeouts" is false: undici imposes default timeouts on every fetch
**Severity:** major
**Artifact:** decisions D5 Facts ("The main CLI client has no retries and no timeouts") and Decision ("No retries, timeouts, or redirects beyond what the source does"); mapping-guide §10 "Timeouts"

**Failing behavior.** Every undici `Client` defaults to a 10 s connect timeout
(`UND/core/connect.js:53`), a 300 s `headersTimeout`, and a 300 s `bodyTimeout`
(`UND/dispatcher/client.js:256-257`). These apply to global fetch and to
`ProxyAgent`s. A hung server therefore fails in the TS CLI after 300 s with
`UND_ERR_HEADERS_TIMEOUT`, which `proxy.ts:117-121` classifies as `timeout`.
A blackholed host fails after 10 s with `UND_ERR_CONNECT_TIMEOUT`. A reqwest
client with no timeouts hangs forever. The daemon's explicit lanes
(`UP/daemon/src/proxy.ts:202-206,245-258`) override only some of these values.

**Correction.** State the undici defaults in D5. Translate them as reqwest
`connect_timeout(10s)` plus a per-request headers deadline (300 s) and a
per-chunk body idle deadline (300 s). Emit errors carrying the matching
`UND_ERR_*` code so D4 classification yields the same `causeClass/causeCode`.

## 8. §6.4 "`JSON.parse` → `serde_json::from_str::<Value>`" changes values and errors
**Severity:** major
**Artifact:** mapping-guide §6.4; §2 note "`serde_json` is built with `preserve_order`"; §13; D3 ("error kinds reachable from a file read")

**Failing behavior.** Verified with Node 24.15.0:
- `JSON.parse("[12345678901234567890]")` round-trips as `12345678901234567000`.
  serde_json keeps the exact u64.
- `1e400` → `Infinity` → stringified as `null`. serde_json errors with "number
  out of range".
- `-0` → stringified as `0`.
- serde_json without `float_roundtrip` may mis-round long decimals, so the
  re-stringified text differs.
- serde_json's recursion limit of 128 rejects deeply nested input that V8
  accepts.
- serde_json rejects lone-surrogate escapes (see finding 3).
- `res.json()` / `res.text()` decode UTF-8 lossily and strip a leading BOM.
  `serde_json::from_slice` on raw bytes rejects both.

V8's error text also reaches agents from **stdin**, not just from profile files:
`UP/cli/src/commands/action/prepare.ts:94-100` prints
`Action JSON failed to parse: ${err.message}`. So every V8 message shape is
reachable, including the context-snippet form
(`Unexpected token 'o', "nope" is not valid JSON`) and the positional form
(`Expected double-quoted property name in JSON at position 7 (line 1 column 8)`),
both verified.

**Correction.** Make `js::json_parse` the only `JSON.parse` / `res.json()`
translation. It returns a JS value model (f64 numbers, lone surrogates allowed,
no depth limit, BOM handling identical to the source's decode path) and V8
messages, including the snippet-truncation algorithm, pinned to Node 24.15.0.
Enable `float_roundtrip` and forbid `arbitrary_precision`. Update D3's
reachability statement. Make `json_stringify` convert every number through f64.

## 9. Missing: `localeCompare`, `Intl`, local-time formatting, `normalize`
**Severity:** major
**Artifact:** mapping-guide §6 (no rule); §6.9 ("With a comparator, use `sort_by`"); §13 (no crates); D3 helper list

**Failing behavior.**
- **`localeCompare`** is ICU collation using the default locale from the
  environment. It is not code-unit order. It is used in sort comparators whose
  order is observable: `UP/daemon/src/agentInboxProjection.ts:104,137`,
  `agentAppInbox.ts:333,339`, `agentProcessManager.ts:8108`,
  `UP/computer/src/setup.ts:529`. A translator following §6.9 will write
  `a.cmp(b)` and get different order for case and punctuation (for example,
  ICU sorts `"a" < "B"`).
- **`Intl.DateTimeFormat().resolvedOptions().timeZone`** is sent on the wire as
  `body.tz` (`UP/cli/src/commands/reminder/update.ts:53`, `schedule.ts:36`). It
  honors `TZ` and ICU canonicalization.
- **`--tz` validation** uses ICU (`schedule.ts:96`). Verified: `america/new_york`,
  `utc`, `EST5EDT`, and `GMT+0` are accepted, and the raw user string is sent
  unchanged. `chrono-tz` `FromStr` is case-sensitive.
- **Local-time rendering** to stdout: `getTimezoneOffset` / `getHours`
  (`UP/cli/src/commands/message/_format.ts:66-76`, `message/search.ts:87-114`)
  need the TZ database with Node's `TZ` handling, which on Windows differs
  from chrono `Local`.
- **`toLocaleDateString("en-US", {month:"short", day:"numeric", timeZone:"UTC"})`**
  at `computer/src/setup.ts:357`.
- **Unicode normalization.** `.normalize("NFKC")` and `toLocaleLowerCase("en-US")`
  in `shared/src/externalProjection.ts:145-148`, and `.normalize("NFKD")` in
  `shared/src/managedMcp.ts:134`.
- **Date serialization.** §3 maps `Date` → `chrono::DateTime<Utc>`. With a serde
  derive, that serializes as RFC 3339 with variable fractional precision, not
  `toISOString()`'s fixed milliseconds.

**Correction.** Add these to `raft_shared::js` with oracle goldens:
`locale_compare` (ICU4X collator, root locale plus Node's env-locale rule),
`resolved_time_zone(&Env)`, `is_valid_time_zone` (case-insensitive plus ICU
aliases), `local_offset_minutes(ms, &Env)`, `to_locale_date_string_en_us_utc`,
`normalize_nfkc/nfkd`, and a serde adapter `iso_ms` for `DateTime`. Add the
crates (`icu_collator`, `chrono-tz` or `jiff`, `iana-time-zone`,
`unicode-normalization`) to §13 and D3.

## 10. Ajv 2020 → the `jsonschema` crate won't reproduce the first error, its messages, or the format behavior
**Severity:** major
**Artifact:** mapping-guide §13 (`jsonschema` (draft 2020-12)); intro ("ajv 8.18.0"); no D-record

**Failing behavior.** `UP/cli/src/commands/integration/manifestV1.ts:504-512,894-900`
compiles schemas with `new Ajv2020({ allErrors:false, strict:false, validateSchema:true, formats:{} })`.
- `actionV1.ts:177-181,192-196` prints `errors[0].instancePath` and
  `errors[0].message` (Ajv's English, for example
  `must have required property 'x'`).
- `manifestV1.ts:513` prints `ajv.compile`'s exception message
  (`schema is invalid: data/… must be …`).
- With `formats: {}`, every `format` keyword is ignored, and Ajv's logger warns
  on stderr about unknown formats. The `jsonschema` crate validates formats by
  default, uses a different error order and wording, and its `pattern` uses a
  non-ECMAScript regex engine.

**Correction.** Add a D-record: port the used subset of Ajv 2020 (keyword
evaluation order, `allErrors:false` first-error semantics, message templates,
`instancePath` format, meta-schema validation messages, `formats:{}` behavior
and its warning), with `pattern` compiled by `JsRegex` with the `u` flag. Use
oracle goldens from `manifestV1.test.ts` / `actionV1.test.ts` plus a corpus.

## 11. D4 classification on the reqwest error chain produces different `causeCode` text
**Severity:** major
**Artifact:** decisions D4 ("the classified form uses the source's `classifyFetchTransportFailure` rules on the reqwest error chain"); mapping-guide §4.3

**Failing behavior.** `classifyFetchTransportFailure` (`UP/cli/src/proxy.ts:106-148`)
reads Node/undici `code` strings from the cause chain (`ENOTFOUND`,
`ECONNREFUSED`, `UND_ERR_CONNECT_TIMEOUT`, OpenSSL codes such as
`UNABLE_TO_VERIFY_LEAF_SIGNATURE` and `DEPTH_ZERO_SELF_SIGNED_CERT`). It also
regex-matches `${err.name} ${err.message}` text (`:95-101`), and the message is
printed as `fetch failed for <url>: <class>/<CODE>` (`:30-31`). reqwest, hyper,
and rustls errors carry none of those codes. Their messages ("error trying to
connect: dns error: …", rustls `InvalidCertificate(UnknownIssuer)`) change both
the class, via the `\bconnect\b` / `\bdns\b` regexes, and the missing
`/<CODE>`. The same undici message text is written to on-disk traces as
`original_message` (`UP/cli/src/client.ts:290-291`).

D4 also doesn't cover:
- two-path syscalls (`rename '<a>' -> '<b>'`, `copyfile`, `symlink`);
- Windows, where libuv maps Win32 errors through `uv_translate_sys_error`
  (for example `ERROR_SHARING_VIOLATION`→`EBUSY`, `ERROR_ACCESS_DENIED`→`EPERM`
  for some ops), which `io::ErrorKind` doesn't reproduce.

**Correction.** Specify a synthetic Node-error layer. `node_compat::fetch_error(&reqwest::Error) -> NodeErrorChain`
yields `TypeError: fetch failed` with a cause `{code, message}` using Node's
exact codes and messages (`getaddrinfo ENOTFOUND host`,
`connect ECONNREFUSED ip:port`, `UND_ERR_CONNECT_TIMEOUT`, the rustls→OpenSSL
code table). Classification then runs on that chain unchanged. Add the
two-path format and the Windows `uv_translate_sys_error` table, and golden-test
each code against the oracle.

## 12. Node-level env behaviors the Rust binary silently drops (TLS trust, env proxy)
**Severity:** major
**Artifact:** mapping-guide §10 ("reqwest (rustls)", "Paths that used global `fetch` without a dispatcher use a no-proxy client"); D5

**Failing behavior.** The TS CLI and daemon run under Node, which honors
`NODE_EXTRA_CA_CERTS`, `NODE_TLS_REJECT_UNAUTHORIZED=0`,
`NODE_USE_SYSTEM_CA` / `--use-system-ca`, and (in recent Node 24 releases, including the oracle's 24.15.0; confirm the exact version) `NODE_USE_ENV_PROXY`
for global fetch. None appears in the source (grep finds no hit), yet all
affect behavior. Corporate MITM setups rely on `NODE_EXTRA_CA_CERTS`, and the
daemon passes the env through to wrappers that run `raft`. rustls with the
default roots ignores all of them. The guide also doesn't say whether roots
come from `webpki-roots` (like Node's bundled store) or `rustls-native-certs`.

**Correction.** Add a D5 sub-decision: the root store is Node's bundled
Mozilla set (`webpki-roots`) plus `NODE_EXTRA_CA_CERTS` (PEM file, same error
text on a bad file), `NODE_TLS_REJECT_UNAUTHORIZED=0` → dangerous verifier,
system CA when `NODE_USE_SYSTEM_CA=1`, and `NODE_USE_ENV_PROXY` handling for
global-fetch call sites. Otherwise, record each as an intentional divergence.

## 13. Proxy transport: undici tunnels plain-http targets and supports socks5; the proxy URL parse differs
**Severity:** major
**Artifact:** mapping-guide §10 ("apply proxies explicitly"); D5 Decision

**Failing behavior.**
- **Tunneling.** `ProxyAgent` defaults to `proxyTunnel = true`
  (`UND/dispatcher/proxy-agent.js:106`). It uses CONNECT for http targets too,
  and only uses absolute-form forwarding when tunneling is off (`:148-154`).
  The CONNECT request carries `host` and `proxy-connection: keep-alive`, plus
  `proxy-authorization: Basic …` decoded from the URL credentials
  (`:123-127,183-192`). reqwest sends plain-http targets as absolute-form
  requests through an http proxy, which puts different bytes on the proxy
  connection.
- **SOCKS.** `socks5:` / `socks:` proxy URLs are supported (`:138-146`). They
  need reqwest's `socks` feature, which §13 doesn't enable.
- **URL parsing.** undici parses the proxy URL with WHATWG `new URL`. A
  scheme-less `HTTPS_PROXY=proxy.corp:8080` becomes the scheme `proxy.corp:`
  and fails, while reqwest's `Proxy::all` silently assumes `http://`.
- **Versions.** The computer resolves undici 7.24.8 and cli/daemon resolve
  7.24.7. Mapping-guide §10 names only the cli and daemon `proxy.ts`, but
  `UP/computer/src/proxy.ts` is a third copy used by `computerFetch`.

**Correction.** Implement a CONNECT-tunnel connector for all proxied targets,
matching undici's CONNECT headers. Enable `socks`. Parse proxy URLs with
`url::Url` and reject what undici rejects, with the same error. List all three
`proxy.ts` copies in §10 and D5, and pin the undici version per package.

## 14. Missing mapping for WHATWG `Headers`, the Node HTTP server, `URL`, and `URLSearchParams` semantics
**Severity:** major
**Artifact:** mapping-guide §10, §6.10; §13

**Failing behavior.**
- **`Headers` iteration.** `for (const [name, value] of upstream.headers.entries())`
  (`UP/daemon/src/agentCredentialProxy.ts:392-399`) yields lowercase names
  sorted alphabetically, with duplicate values joined by `", "` and
  `set-cookie` kept separate. These become the agent-facing response headers
  through Node's `http` server. `reqwest::HeaderMap` iteration order and
  duplicate handling differ.
- **Server defaults.** Node's `http.ServerResponse` adds `Date`,
  `Connection: keep-alive`, `Keep-Alive: timeout=5`, and
  `Transfer-Encoding: chunked` (title-cased), and preserves the case of
  user-set names. hyper adds only `date`, lowercases everything, and omits
  `keep-alive`. §10 maps local servers to hyper/axum with no header rule.
- **`URL` / `URLSearchParams`.** `new URL(...)`, `url.searchParams.set`
  (which re-serializes the whole query), `url.port` (empty when the port is the
  default), and bracketed IPv6 `hostname` are used for the proxy bypass and for
  diagnostics (`UP/cli/src/proxy.ts:155-162,164-184`). §6.10 covers only
  `URLSearchParams` → `form_urlencoded`, and the `url` crate isn't in §13.
- **Query stringification.** The agent-API client doesn't send the parsed query
  as JSON. It stringifies it with `String(value)`, skips `null`/`undefined`,
  repeats keys for arrays, and uses `set` for scalars
  (`UP/shared/src/agentApiRawClient.ts:172-180`). §5 says only "anything sent
  on the wire is the schema's parsed output".

**Correction.** Add `raft_shared::http::fetch_headers_entries()` (WHATWG sort
and combine) and a `NodeHttpResponse` writer that reproduces Node's default
response headers and name casing, with hyper's `preserve_header_case` /
title-case enabled. Map `URL` to `url::Url` with a table for `href`, `host`,
`hostname`, `port`, `origin`, `search`, and `searchParams`, and add `url` to
§13. Add the query-stringification rule to §5.

## 15. Multipart: undici's FormData encoding differs from `reqwest::multipart`
**Severity:** major
**Artifact:** mapping-guide §10 ("`FormData` multipart → `reqwest::multipart::Form` with the same field names, file names, and content types")

**Failing behavior.** `UP/cli/src/client.ts:357-377` sends `FormData` bodies
with the content type left to fetch. undici's boundary is
`----formdata-undici-0<11 digits>`. It writes filenames raw in UTF-8, escaping
only `"`, CR, and LF (`%22`, `%0D`, `%0A`), and sets `content-length`. reqwest
uses a random hex boundary and percent-encodes non-ASCII filenames by default
(`PercentEncoding::PathSegment`). An upload of `报告.png` would reach the
server with a different filename.

**Correction.** Require a hand-built multipart body (or
`Form::percent_encode_noop()` plus a custom boundary) that reproduces undici's
boundary format, part-header order and casing, filename escaping, and known
`content-length`. Golden-test it against the oracle.

## 16. WebSocket: `ws` offers permessage-deflate and has different frame limits than tungstenite
**Severity:** major
**Artifact:** mapping-guide §10 ("WebSocket client → `tokio-tungstenite`"); D5

**Failing behavior.** `UP/daemon/src/connection.ts:235,248` uses `ws` 8.20.0
with default options. Those defaults send
`Sec-WebSocket-Extensions: permessage-deflate; client_max_window_bits` and
accept messages up to 100 MiB (`maxPayload`). tokio-tungstenite doesn't
implement permessage-deflate, so the handshake differs and a server that
compresses frames can't be read. Its default limits (`max_message_size` 64 MiB,
`max_frame_size` 16 MiB) reject frames that `ws` accepts. The proxied handshake
goes through `https-proxy-agent` (`UP/daemon/src/proxy.ts:158`), which issues
its own CONNECT header set.

**Correction.** Pick a client that supports permessage-deflate (or add it),
set limits equal to `ws` defaults, and golden-test the handshake request bytes
and the proxy CONNECT bytes against the oracle.

## 17. The JS string API list omits `split(sep, limit)` and replacement semantics, with real counterexamples
**Severity:** major
**Artifact:** mapping-guide §6 (helper list), §6.2; D3

**Failing behavior.**
- **`split` with a limit.** `entry.split(":", 2)` splits on every separator and
  keeps the first two pieces, so `"a:b:c"` → `["a","b"]`. The natural Rust
  `splitn(2, ':')` gives `["a","b:c"]`. Sites:
  - NO_PROXY port matching in `UP/cli/src/proxy.ts:178`,
    `UP/daemon/src/proxy.ts:143`, `UP/computer/src/proxy.ts:58` (IPv6 and
    `host:port:x` entries);
  - inbox target parsing in `UP/daemon/src/agentInboxStateMachine.ts:586,599`
    (a thread id containing `:` is silently truncated in TS).
- **Callback replacers.** `.replace(/…/g, (match, key) => …)` builds agent-API
  paths (`UP/shared/src/agentApiRawClient.ts:144`) and manifest paths
  (`UP/cli/src/commands/integration/invoke.ts:401`, `manifestV1.ts:295`). §6.2
  lists only `$`-syntax `replace`.
- **String-pattern `replace`.** `String.prototype.replace` with a *string*
  pattern replaces only the first match and still expands `$&`, `$1`, and `$$`
  in the replacement. Commander's `wrap` uses `.replace('\r\n', '\n')`
  (`CMD/help.js:497`), and translators will reach for `str::replace`, which
  replaces every match.

**Correction.** Add `js::split(s, sep, limit)`, `js::split_regex`,
`JsRegex::replace_with(|m: &JsMatch| -> String)` (with capture groups, offset,
and named groups), and `js::string_replace_first(s, pat, repl)` with `$`
expansion. Forbid `splitn`, `str::replace`, and `replacen` on translated
string-API calls.

## 18. `regress` over `&str` doesn't give JS semantics for non-`u` patterns
**Severity:** major
**Artifact:** mapping-guide §6.2; D3 (`JsRegex` over `regress`)

**Failing behavior.**
- **Code units vs code points.** regress 0.11.1's `find` / `find_from` run on
  UTF-8 `&str` and step by code points. JS non-`u` regexes step by UTF-16 code
  units. Code-unit semantics need the `utf16` feature with `find_from_ucs2`
  (`~/.cargo/registry/src/*/regress-0.11.1/src/api.rs:502-518`,
  `Cargo.toml [features] utf16`). Commander's help wrap uses the non-`u`
  pattern `.{1,${columnWidth-1}}` (`CMD/help.js:503-505`). With astral
  characters in a description, the `&str` engine counts one where V8 counts
  two, and wraps at a different column.
- **Byte offsets.** Match offsets from `find` are UTF-8 byte offsets, while JS
  `index` and `lastIndex` are UTF-16.
- **Flags.** regress parses only `imsuv` (`api.rs:65-85`). `g`, `y`, and `d`
  must be handled by `JsRegex`.
- **Unicode tables.** regress's Unicode version for `\p{L}`, `\p{N}`
  (`UP/shared/src/raftRefs.ts:1,14,29`) and for `iu` case folding may differ
  from V8/ICU in Node 24.15.

**Correction.** Specify that `JsRegex` runs on UTF-16 buffers (the
`regress/utf16` feature): `find_from_ucs2` for non-`u` patterns and
`find_from_utf16` for `u`/`v` patterns, with UTF-16 indices throughout. Add a
golden check that regress's Unicode version matches Node 24.15's ICU for
`\p{L}`, `\p{N}`, and case folding. Otherwise, document the delta.

## 19. Integer-key hoisting belongs to the object model, not only to `json_stringify`
**Severity:** major
**Artifact:** mapping-guide §6.3 ("integer-like key hoisting"), §3 (`Record` → `IndexMap`), §6.8; D2

**Failing behavior.** V8 orders array-index keys (canonical decimal ≤ 2^32−2)
ascending, before string keys, on every object. The ordering shows up in
`Object.entries` / `Object.keys` and zod output, not only in `JSON.stringify`.
Verified: `{b:1,"2":2,a:3,"1":4,"01":5,"4294967295":6,"4294967294":7}` →
`{"1":4,"2":2,"4294967294":7,"b":1,"a":3,"01":5,"4294967295":6}`.

Iteration sites whose output is observable:
- `UP/shared/src/actionCards.ts:267-285`: `Object.entries(action)` and
  `.slice(0, 24)` choose which 24 presentation items reach the server, and the
  nested `safePresentationValue` prints `key: value`;
- `UP/shared/src/appConfigTransport.ts:95`;
- `agentApiRawClient.ts:172`, where query parameter order goes on the wire.

An `IndexMap` in insertion order gets all of these wrong. The guide's rule
fixes only the final stringify.

The guide also says "integer-like" but never defines it: `"01"` and
`"4294967295"` are not hoisted.

**Correction.** Define `js::Object` (or require `IndexMap` insertion through
`js::object_insert`) that maintains V8 ordering (array indices ascending, then
insertion order) and use it for every JS object value, including zod output and
the `Value`s `json_parse` returns. Define array index exactly.

## 20. Numbers: defaulting to `i64` and Rust arithmetic diverge from JS doubles
**Severity:** major
**Artifact:** mapping-guide §3 (row "number that is always an integer in practice → `i64`"), §6.6; D3

**Failing behavior.**
- **`--lines` parsing.** `raft-computer logs --lines` is parsed by
  `Number.parseInt(v, 10)` (`UP/computer/src/cli.ts:492`) and accepted when
  `Number.isInteger(x) && x > 0` (`UP/computer/src/lib/api.ts:533-540`).
  Verified:
  - `--lines 100000000000000000000000` → `1e23`, which passes and prints the
    whole file. An `i64` overflows.
  - `--lines 5.9` → 5. `"5.9".parse::<i64>()` fails, so the default of 200
    lines is used.
  - `--lines 1e25` → 1.
- **`parseInt` without a radix** accepts hex: `parseInt("  -0x1F")` → -31. The
  helper needs a radix parameter and JS radix inference.
- **Floor vs integer division.** `Math.floor(a/b)` differs from Rust `i64 / i64`
  for negatives.
- **Rounding.** `Math.round(-2.5)` is -2, while `f64::round` gives -3. Used in
  `UP/cli/src/commands/agent/_format.ts:239`.
- **Negative zero.** `(-0.001).toFixed(2)` is `"-0.00"` and `(-0).toFixed(2)`
  is `"0.00"`, but Rust `format!("{:.2}", -0.0)` gives `"-0.00"`.
- **Casts.** `f64 as i64` saturates and maps NaN to 0, where JS keeps
  NaN/Infinity.
- **Typed fields.** `serde` deserialization of a JSON number with a fraction
  (`5.0`) into `i64` fails, where TS accepts it.

**Correction.** Change §3: keep `f64` wherever a value comes from user input,
`parseInt`/`Number`, or unvalidated JSON, and convert to integers only through
`js::to_integer_or_infinity` / `js::is_integer` at the source's own check.
Require `js::parse_int(s, radix: Option<u32>) -> f64`, `js::math_round`, and
`js::math_floor_div`. Forbid bare `as` casts between floats and integers, and
forbid `/` on integers where the source used `Math.floor`. Add all of these to
D3's goldens.

## 21. Build-mode-dependent arithmetic isn't addressed
**Severity:** major
**Artifact:** mapping-guide §4.5 (covers only asserts); workspace profile (unspecified)

**Failing behavior.** Integer overflow panics in debug and test builds and
wraps in release. The hand-rolled SHA-256 in `UP/shared/src/apmHeldFreshness.ts:303-340`
relies on `>>> 0` wraparound. A literal `u32` `+` translation panics under
`cargo test` (debug) and silently wraps in `cargo build --release`, so the
gates in README 4–5 (debug) and 2 (release) exercise different code. The same
holds for any `i64` counter arithmetic under §3.

**Correction.** Set `overflow-checks = true` in `[profile.release]` so both
modes behave the same. Require `wrapping_*` wherever the source uses
`>>> 0` / `| 0`, and `checked_*` with the source's error elsewhere. Also forbid
`cfg!(debug_assertions)` behavior differences in translated code.

## 22. §5's "typed structs deserialized from the parsed value" breaks on unvalidated JSON the source only casts
**Severity:** major
**Artifact:** mapping-guide §5 bullet 2; §3

**Failing behavior.** Much response handling isn't zod-parsed; it's cast. For
example, `UP/cli/src/client.ts:221-247` casts the error body and does
`error = body?.error ?? \`HTTP ${res.status}\``, and
`errorCode = body?.errorCode ?? body?.code ?? null`. If the server sends
`"error": {…}` or `"error": 5`, TS carries the non-string forward: template
literals render `[object Object]` or `5`, and `??` keeps `0` or `""`. A Rust
`struct { error: Option<String> }` fails to deserialize and takes a different
branch. The same pattern appears in `parseProxyDiagnostics(body?.proxy)` and
throughout the formatters.

**Correction.** Add a rule: `as T` casts on unvalidated JSON are translated
over `js::Value` with explicit JS coercions (`js::to_display_string`, the
`??`/`||` helpers). Typed deserialization is allowed only immediately after a
zod-equivalent parse that guarantees the shape.

## 23. Windows process and env semantics are missing
**Severity:** major
**Artifact:** mapping-guide §7 (`Env` = "ordered map of `OsString` keys", `env_clear()` + explicit env, `home_dir` = "`HOME`/`USERPROFILE`, then the OS lookup"), §9 (`child_process.spawn`)

**Failing behavior.**
- **Case-insensitive env.** On Windows, `process.env` lookups are
  case-insensitive (`env.Path` = `env.PATH`). An `OsString`-keyed map is
  case-sensitive.
- **libuv required vars.** When spawning with an explicit env on Windows, libuv
  adds missing `SYSTEMROOT`, `SYSTEMDRIVE`, `TEMP`, `PATH`, `USERPROFILE`, and
  others from the parent (`make_program_env` required_vars). Rust
  `env_clear().envs(..)` doesn't, and children then fail (Winsock and crypto
  need `SYSTEMROOT`).
- **Home directory.** `os.homedir()` on Windows reads `USERPROFILE` only,
  never `HOME`. On Unix it reads `HOME`, and an empty `HOME` returns `""`. Git
  Bash sets `HOME` on Windows, so "HOME/USERPROFILE" picks a different
  directory.
- **Shell spawn for `.cmd`.** `shell: requiresWindowsShell(command)` spawns
  `.cmd`/`.bat` through `cmd.exe /d /s /c "<joined args>"` with verbatim
  arguments (`UP/daemon/src/drivers/codex.ts:185,401`, `grok.ts:170`,
  `probe.ts:259-264`). Rust `Command` applies its own batch escaping.
- **Executable search.** Rust's Windows PATH search order (child PATH,
  application directory, System32) differs from libuv's (PATH plus
  `.com`/`.exe`).

**Correction.** Make `Env` case-insensitive on Windows with Node's
first-occurrence rule. Implement `raft_shared::process::spawn` that reproduces
libuv's required-vars injection, Node's `shell:true` command-line construction,
and libuv's search order. Specify `home_dir` per platform exactly as libuv
does. Golden-test these on the Windows CI leg.

## 24. §8 maps `Promise.all` / `Promise.race` to combinators that cancel the losers
**Severity:** major
**Artifact:** mapping-guide §8 bullet 2

**Failing behavior.** `Promise.all` rejects on the first failure, but the other
promises keep running to completion. `try_join_all` drops them mid-flight.
`UP/daemon/src/agentProcessManager.ts:4869` stops all agents in parallel with
`Promise.all(ids.map(id => this.stopAgent(id, {wait:true})))`. In TS, one
rejection doesn't abort the other stops. In Rust, the remaining stops are
cancelled at their next `.await`, which leaves processes running and state
half-updated. `Promise.race` behaves the same way: `UP/cli/src/commands/agent/bridge.ts:488`
leaves `reader.read()` pending after the timeout, and `select!` drops it.

**Correction.** Map `Promise.all` to spawn-all, then join each handle in order,
returning the first error only after its position (the others continue
detached). Map `Promise.race` to `select!` over spawned handles (or
cancel-safe futures), with the losers left running when the source had side
effects. Require a review note per site.

## 25. Test-parity rules can't handle the actual upstream titles and skips
**Severity:** major
**Artifact:** mapping-guide §1.6, §12; D9; README gate 6 ("test counts match the parity report (no silent skips)")

**Failing behavior.**
- **Dynamic titles.** 6 titles are template literals, several generated in
  loops (`UP/cli/src/commands/message/_privateStateFile.test.ts:9,32`,
  `UP/cli/src/commands/version.test.ts:164`, which embeds
  `JSON.stringify(metadata)`). D9's "reads upstream test titles directly from
  the TS files" can't enumerate them statically.
- **Invalid identifiers.** 8 titles start with a digit and 21 contain non-ASCII
  characters, so "snake_case of the title" yields invalid or colliding Rust
  identifiers.
- **Nesting.** node:test files use `describe` (6), and title-only matching
  loses the describe path, so duplicate titles are ambiguous.
- **Conditional skips.** Upstream skips tests conditionally
  (`UP/cli/src/auth/managedTransport.test.ts:87` `{ skip: process.platform === "win32" }`,
  and several `inSourceSnapshot` skips in `axManifest.test.ts:18,29`). Gate 6's
  "no silent skips" and equal counts on all three OSes contradict upstream's
  own per-platform counts.

**Correction.** Enumerate titles by running the oracle
(`node --test --test-reporter=tap` / `vitest list --json`) on each OS, not by
parsing source. Key tests by `file + describe path + title`. Define the
identifier rule (prefix `t_` for digit-leading titles, transliterate non-ASCII
characters, dedupe). Port upstream skip conditions as `#[cfg_attr(windows, ignore = "…")]`,
counted as "upstream-skipped" in the parity report.

## 26. §9's readline rule is wrong: Node splits on a lone `\r` and decodes lossily
**Severity:** minor
**Artifact:** mapping-guide §9 row "`readline` line splitting → `tokio::io::BufReader::lines()`"

**Failing behavior.** Verified with Node 24.15: readline over `"a\rb\r\nc\n"`
plus the bytes `64 FF 0A` plus `"tail"` emits `"a"`, `"b"`, `"c"`, `"d\uFFFD"`,
and `"tail"`. `BufReader::lines()` doesn't split on a lone `\r`, and it returns
`Err(InvalidData)` on invalid UTF-8, which ends the stream. The in-scope site is
`UP/cli/src/commands/agent/login.ts:182`. The rule is written generically, so
translators will reuse it for child output too.

**Correction.** Add `js::ReadlineSplitter` (`/\r?\n|\r(?!\n)/`, including
readline's `crlfDelay` handling, over `Utf8StreamDecoder`) and point the table
row at it.

## 27. Redirect "follow" and response-decoding defaults are not specified
**Severity:** minor
**Artifact:** mapping-guide §10 ("`redirect: \"error\"` … `\"manual\"` …")

**Failing behavior.**
- **Redirects.** Default fetch and explicit `redirect: "follow"`
  (`UP/daemon/src/agentCredentialProxy.ts:619,1276`) follow up to 20
  redirects, with fetch-spec header stripping on cross-origin hops. reqwest's
  default policy stops at 10 and strips a different header set
  (`authorization`, `cookie`, `proxy-authorization`, `www-authenticate`).
- **Body decoding.** `res.text()` decodes UTF-8 lossily and strips a BOM.
  reqwest's `.text()` sniffs the charset from `content-type` and can decode
  non-UTF-8.

**Correction.** Specify `Policy::custom` with a limit of 20 and fetch-spec
header stripping, plus `js::response_text(bytes)` (UTF-8 lossy, BOM strip,
ignoring the charset), for every `res.text()` / `res.json()`.

## 28. `undefined` has no representation in the mapping, so spread and optional-key semantics are lost
**Severity:** minor
**Artifact:** mapping-guide §3 (`T | undefined` → `Option<T>` skip), §6.3 ("`undefined` omission"), §6.8 (`object_assign` on `Map<String, Value>`)

**Failing behavior.** `{...a, ...b}` where `b.k === undefined` *overwrites*
`a.k` (the key keeps its position, the value becomes undefined, and
`JSON.stringify` drops it). `Map<String, Value>` can't express this. A
translator either skips the key (keeping `a.k`, which is wrong) or writes
`null` (which serializes as `null`, also wrong). Pattern sites:
`{ ...command.optsWithGlobals(), ...options }` (`UP/cli/src/commands/agent/login.ts:114`)
and the `...(x ? {k:v} : {})` idioms throughout `client.ts` and `transportTrace`.

**Correction.** Give the JS value model an `Undefined` variant (or
`Option<Value>` entries in `js::Object`). Define `object_assign` over it and
make `json_stringify` omit `Undefined` in objects and emit `null` in arrays, as
V8 does.

## 29. §4.2 "`Display` equals `.message`" conflicts with `String(err)` sites
**Severity:** minor
**Artifact:** mapping-guide §4.2

**Failing behavior.** `String(err)` and `${err}` render `"<name>: <message>"`,
for example `Error: x` or `TypeError: fetch failed`. There are 105 such
expressions in non-test source, for example
`UP/daemon/src/chatBridgeRequest.ts:39` `return String(err)`. With `Display`
equal to `.message`, a literal translation drops the `Error: ` prefix.
`RAFT_COMPUTER_DEBUG_STACK` prints `err.stack` (`UP/computer/src/cli.ts:922-925`),
which can't be reproduced and has no decision.

**Correction.** Require every error type to expose `js_name()` and add
`js::error_to_string(&err)` = `"{name}: {message}"`. Record `err.stack` output
as an accepted divergence, or decide it.

## 30. The crate list in §13 is incomplete, and some listed choices are infeasible as specified
**Severity:** minor
**Artifact:** mapping-guide §13; the "Adding any other crate needs a note" rule

**Failing behavior.**
- **Missing crates** that findings 9, 13, 14, and 18 require: `url`,
  `unicode-normalization`, `icu_collator`, `chrono-tz`/`jiff`,
  `iana-time-zone`, `webpki-roots`.
- **Missing features:** reqwest `gzip`/`brotli`/`deflate`/`zstd`/`socks`,
  `http1_only`, `default-features = false`; regress `utf16`; serde_json
  `float_roundtrip`.
- **Features that should be off:** reqwest's default `http2`, `default-tls`,
  and `macos-system-configuration`.
- **Unfit choices:** `jsonschema` can't replace Ajv (finding 10), and
  `tokio-tungstenite` lacks permessage-deflate (finding 16).
- **Implicit defaults.** §3's `Map` → `HashMap` default ("IndexMap if iteration
  order is observable") leaves the call to the translator, who often can't see
  the downstream consumer. JS `Map`/`Set` iteration is always insertion order.

**Correction.** Rewrite §13 as a pinned `[workspace.dependencies]` block with
features. Default `Map`/`Set` to `IndexMap`/`IndexSet`, and allow `HashMap` only
with a review note.

## 31. Conflicts and gaps across README, decisions, and the guide
**Severity:** minor
**Artifact:** README, decisions.md, mapping-guide (cross-document)

1. **OS service.** README "Finish line" says `raft-computer` "runs as an OS
   service". D11 says upstream "runs no OS service manager" and forbids adding
   one.
2. **`raft migrate`.** README scope includes "every command registered in
   `packages/cli/src/main.ts`", which covers `raft migrate`
   (`UP/cli/src/main.ts:357-358`). O1 leaves migration open, and D2 says
   "`agentMigration.ts` if kept". The translation of `main.ts` and the
   `migrate` help golden (in the 116) are blocked until O1 is decided.
3. **Unassigned hidden modes.** The raft-computer in-scope list omits
   `__print-env` (D14) and says nothing about `__build-versions`
   (`UP/computer/src/cli.ts:879-887`), which prints three baked versions.
4. **Who runs the oracle.** README says only the orchestrator runs builds and
   tests and workers never run git. D2 and D9 require oracle-generated corpora
   (`tools/zod-golden/`, `tools/golden/`), and §5 requires the runtime to
   "reproduce each record". Nothing assigns who runs the oracle or when.
5. **Undici version.** The guide header pins "undici 7", but three undici
   builds are in play: npm 7.24.7 (cli, daemon), npm 7.24.8 (computer), and
   Node 24.15's bundled undici for global fetch. Node itself is not pinned
   (D3 says "Node 24", while the oracle runs v24.15.0 and V8 message text
   varies by version).
6. **`exit` in commander.** §4.4 says only `main` calls `std::process::exit`,
   but commander's default path calls `process.exit` from inside parsing
   (finding 5). D1 must say that the crate returns an exit code instead.

**Correction.** Fix each wording:
- README: "runs as a detached background service (D11)".
- README scope: mark `migrate` as blocked on O1.
- README scope: list `__print-env` and decide `__build-versions`.
- README roles: add a "golden capture" step owned by the orchestrator.
- Guide header: pin Node 24.15.0 and the per-package undici versions.
- D1: add the no-exit rule.
