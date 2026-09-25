# 试点 `raft task claim` 评审 A：行为一致性

评审对象：工作区中未提交的 Rust 试点代码（见 HANDOFF.md 阅读顺序）。
对照基准：`upstream/raft-source` @ `05f7d8fd`，zod 4.3.6。

方法：用 `node --import tsx` 直接驱动上游 `taskClaimCommand.handler`，并注入 client（与 `claim.test.ts` 的写法相同）。另在 scratchpad 里建了一个 crate，以 path 依赖引用 `raft-shared` / `raft-cli`，把同一批输入交给 Rust handler 和 schema，再逐字比较两边的输出。下面每条都是两边实际跑出来的结果。临时 `.ts` 文件已删除，仓库里没有改动任何源文件。

已确认一致、不再列出的部分：
- `--number` 的 `js::to_number`：共 80 个输入逐位比对（覆盖 hex/octal/binary、带符号 hex、指数、`.5`、`1.`、`+1`、`-0`、`Infinity`、`1e400`、`1_000`、全角数字、U+FEFF/U+2028/U+00A0/U+3000 空白、U+180E/U+0085/U+200B 非空白、超过 2^53 的 hex 舍入），与 V8 `Number()` 完全一致。
- body 键序、passthrough、trim、`--message-id "  "` 触发 INVALID_ARG、零成功退出策略（含 `results: []`）、"already claimed by you"、refusal summary、reviewer-isolation 各分支、HTTP 4xx/5xx、`error: ""` 与 `error: null` 的 `??` 语义、`agent_proxy_failed` 在 surface client 先抛出 PROXY_5XX、transport 错误映射（CHECK_FAILED / `agent_api_transport` / retryable=false）、stderr 错误信封与 JSON 错误 payload 的字段和顺序。

---

## F1 [阻塞] `z.number().int()` 缺少 safe-integer 范围检查，超大 `--number` 会被原样发出

- 位置：`crates/raft-shared/src/schema.rs:374-376`（`parse_number` 的 int 检查）。受影响的 schema：`agent_api_claim.rs:258`（body 的 `task_numbers`）、`:299`（响应 `taskNumber`）、`:167/171/190`（envelope）。
- 原因：zod 4 的 `.int()` 就是 `safeint` 格式，范围 `[MIN_SAFE_INTEGER, MAX_SAFE_INTEGER]`，超出时报 `too_big` / `too_small`。Rust 只检查 `fract() == 0`。
- 输入 1：`raft task claim --target '#x' --number 9007199254740993`
  - 上游：`INVALID_ARG`，消息为 `Agent API taskClaim body did not match the shared contract`，**不发请求**。cause 是 `{"code":"too_big","maximum":9007199254740991,"note":"Integers must be within the safe integer range.","origin":"int","inclusive":true,"path":["task_numbers",0],"message":"Too big: expected int to be <=9007199254740991"}`。
  - Rust：发出 `POST {"channel":"#x","task_numbers":[9007199254740992]}`，退出码 0。认领的任务号和用户输入的不同，这是一次上游会拒绝的写操作。
- 输入 2：`--number 1e21`
  - 上游：同上，INVALID_ARG，不发请求。
  - Rust：发出 `{"channel":"#x","task_numbers":[1000000000000000000000]}`。
- 输入 3（响应侧）：`{"results":[{"taskNumber":9007199254740993,"success":false,"reason":"nope"}]}`
  - 上游：`INVALID_JSON_RESPONSE`，`response did not match the shared contract`。
  - Rust：`CLAIM_FAILED`，`Claim refused — #9007199254740992 nope. …`。
- 修复建议：`NumberChecks.int` 通过后，再检查 `number > 9007199254740991.0` 和 `number < -9007199254740991.0`，分别生成 zod 形状的 `too_big` / `too_small` issue，字段顺序为 `code, maximum|minimum, note, origin:"int", inclusive:true, path, message`。在 `agent_api_claim_tests.rs` 加 `9007199254740993` 和 `1e21` 两条回归用例，在 `claim_tests.rs` 加一条"不发请求"的断言。

## F2 [应修] `string().datetime()` 与 zod 4.3.6 的正则不一致，有的该拒不拒，有的该收不收

- 位置：`crates/raft-shared/src/schema.rs:696-706`（`is_zod_datetime` / `has_time_and_offset`），它依赖 `js.rs:636-722`（`parse_iso_instant`）。
- zod 4.3.6 默认的 `datetime()`（`offset` 和 `local` 都为 false）等价于 `^<date>T(?:[01]\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:\.\d+)?)?Z$`：
  - 秒可以省略；
  - 只接受大写 `T` 和大写 `Z`；
  - 不接受 `±hh:mm` 偏移；
  - 年份必须恰好 4 位。
- 以下输入都放在 held 响应的 `heldMessages[0].timestamp`：

| 输入 | 上游 | Rust |
|---|---|---|
| `2026-01-01T10:59Z`（无秒） | 通过，渲染 `│ @a 10:59  hi`，退出码 0 | `INVALID_JSON_RESPONSE`，退出码 1 |
| `2026-01-01T10:59:00+05:00` | `INVALID_JSON_RESPONSE`（`Invalid ISO datetime`），退出码 1 | 通过，渲染 `@a 05:59`，退出码 0 |
| `2026-01-01t10:59:00z` | `INVALID_JSON_RESPONSE`，退出码 1 | 通过，渲染 `@a 10:59`，退出码 0 |

- 修复建议：不要再借 `date_parse` 判断格式，直接照 `regexes.js` 的 `dateSource` 加 `timeSource({precision: undefined})` 加 `Z` 手写一个匹配器（闰年规则也照搬 `dateSource`）。issue 字段按 zod 补齐，见 F9。

## F3 [应修] `js::date_parse` / `core_time::hhmm` 不等于 V8 的 `new Date(s)`：小数秒被四舍五入，另有几种合法格式解析失败

- 位置：`crates/raft-shared/src/js.rs:688-697`（第 4 位小数 ≥5 时进位），`:656`（要求长度 ≥19，也就是必须有秒），`:699-715`（没有偏移时直接判为不可解析）。调用方是 `crates/raft-cli/src/core_time.rs:13-18`，由 `freshness/format.rs:47-51` 调用。
- 输入 1：held 消息 `timestamp: "2026-01-01T10:59:59.9999Z"`（能通过 zod）
  - 上游：`│ @a 10:59  hi`（V8 截断到毫秒）。
  - Rust：`│ @a 11:00  hi`。
- 输入 2：`threadParentMessage.createdAt: "2026-01-01T09:30Z"`（parent 不经过 schema）
  - 上游：`│ @p 09:30  root`。
  - Rust：`│ @p  root`（时间变成空）。
- 输入 3：`createdAt: "2026-01-01T24:00:00Z"`
  - 上游：`@p 00:00`。
  - Rust：空。
- 输入 4：`createdAt: "2026-01-01T09:30:00"`（没有偏移，V8 按本地时区解析）
  - 上游在 `TZ=Asia/Shanghai` 下输出 `@p 01:30`。
  - Rust：空。
- 修复建议：
  - 超过 3 位的小数一律截断，不进位；
  - 允许省略 `:ss`；
  - 允许 `24:00:00(.000)`；
  - 没有偏移的日期时间按本地时区处理。本地时区可以先接 D3 约定的时间 helper；如果这一刀不做，就在 HANDOFF 的"已知偏差"里写明。
- 前两点是纯函数修正，建议这一刀就修。

## F4 [应修] `js::number_to_string` 不是 ECMAScript `Number::toString`

- 位置：`crates/raft-shared/src/js.rs:444-466`。
- 问题：
  - 绝对值大于 2^53、小于 1e21 的整数，被 `{:.0}` 打成精确的十进制展开；
  - 1e21 及以上、以及小于 1e-6 的数，用 Rust `Display` 输出，不会切到指数形式。
- 直接比对 `String(Number(s))`：

| 输入 | 上游 | Rust |
|---|---|---|
| `1e21` | `1e+21` | `1000000000000000000000` |
| `1.5e-7` | `1.5e-7` | `0.00000015` |
| `123456789012345678901` | `123456789012345680000` | `123456789012345683968` |
| `12345678901234567890` | `12345678901234567000` | `12345678901234567168` |
| `1.5e300` | `1.5e+300` | 301 位数字 |

- 从 handler 能观察到的例子：held 响应 `{"state":"held","newMessageCount":1e21,"omittedMessageCount":1e-7,"firstShownSeq":3,"heldMessages":[]}`。
  - 上游：`Held — 1e+21 unread messages in #x.`，`├ ⋯ 1e-7 earlier messages skipped …`。
  - Rust：`Held — 1000000000000000000000 unread messages …`，`├ ⋯ 0.0000001 earlier messages …`。
- 这个函数是 D3 的公共原语。json_stringify、模板插值、zod 字面量都依赖它，后续命令会大量使用。
- 修复建议：用 `format!("{:e}", x)` 取最短往返的有效数字串 `s`（k 位）和指数，令 n = exp + 1，再按 ES 规范 6.1.6.1.20 分四档输出：
  - k ≤ n ≤ 21：数字串后补 0；
  - 0 < n ≤ 21：在第 n 位后插小数点；
  - −6 < n ≤ 0：写成 `0.` + 若干 0 + 数字串；
  - 其余：指数形式，指数写成 `e+N` 或 `e-N`。
- 把上面的表格加成单测。

## F5 [应修] `string().uuid()` 没有检查版本位和变体位

- 位置：`crates/raft-shared/src/schema.rs:708-733`。
- zod 4.3.6 的 `uuid()` 要求第 3 组首位在 `[1-8]`、第 4 组首位在 `[89abAB]`，或者是全 0 / 全 f。Rust 只检查"8-4-4-4-12 的十六进制"。
- 输入：held 消息带 `external_message`（其余字段都合法，`senderType:"third_party_app"`、`mentioned:false`），`projection_id: "00000000-0000-0000-0000-000000000001"`。
  - 上游：`INVALID_JSON_RESPONSE`，issue 为 `{"origin":"string","code":"invalid_format","format":"uuid",…,"message":"Invalid UUID"}`，退出码 1。
  - Rust：通过，渲染 hold，退出码 0。
- 修复建议：照 `regexes.js` 的 `uuid()` 正则实现，并加 nil / max 两个特例。

## F6 [可选] `data: Some(Value::Null)` 不会走 `empty_response`

- 位置：`crates/raft-shared/src/agent_api_claim.rs:449`。
- 上游用 `response.data === null` 判断空响应，所以 null 报 `returned an empty response body`；undefined 会进入 schema，报 `response did not match the shared contract`。
- Rust 只把 `None` 当成空；`Some(Value::Null)` 会进入 schema，报 `… did not match the shared contract`。错误码两边都是 `INVALID_JSON_RESPONSE`，只有消息不同。
- 以后 live HTTP 客户端把 JSON 字面量 `null` 解析成 `Some(Value::Null)` 时，就会触发这个差异。
- 修复建议：把 `None | Some(Value::Null)` 都当成 `EmptyResponse`，并在注释里写明 `None` 对应 JS 的 `null`。

## F7 [可选] `api_failure_error` 会吞掉未知的 `errorCode`

- 位置：`crates/raft-cli/src/core/api_failure.rs:54-66`，以及 `:138-157` 的白名单。
- 上游写法是 `(response.errorCode as CliErrorCode) ?? fallback`，任何非空字符串都原样当作 CLI code。
  - 上游：`apiFailureError({status:502, errorCode:"rate_limited"}, "CLAIM_FAILED").code` 是 `rate_limited`；`{status:409, errorCode:"task_locked"}` 得到 `task_locked`。
  - Rust：分别得到 `SERVER_5XX` 和 `CLAIM_FAILED`。
- 在 claim 这一刀里，这个函数只会以 `agent_proxy_failed` 调用，所以**这条路径到不了**。但后续命令用到它时就是应修。
- 修复建议：`CliErrorCode` 增加一个 `Other(String)`（或者 code 直接用字符串类型），把未知 code 原样透传。

## F8 [可选] freshness formatter 会把 passthrough 原值先转成数字

- 位置：`crates/raft-cli/src/commands/freshness/format.rs:149-162`、`:185`。
- 上游把 `data.firstShownSeq` 原值直接插进 `historyCursorText`，并用 JS 的 `<` 比较 `parent.seq`（两边都是字符串时按字典序比较）。Rust 先 `to_number` 再格式化。
- 输入：`{"state":"held","omittedMessageCount":2,"firstShownSeq":"0x10","heldMessages":[]}`
  - 上游：`… --before 0x10. ⋯`
  - Rust：`… --before 16. ⋯`
- 这两个字段不在 held schema 里，服务端正常只会发数字，所以风险很低。
- 修复建议：`first_shown_seq` 保留原始 `Value`，插值时用 `to_display_string`，比较时按 JS 抽象关系比较实现（两边都是字符串时比较字符串）。

## F9 [可选] ZodError 的 issue 形状与 zod 4.3.6 不一致（只影响 `cause`，不输出给用户）

- 位置：`crates/raft-shared/src/schema.rs:544-559`、`:586-601`、`:306-315`、`:374-376`。
- 同一输入的对照：
  - `invalid_type`：zod 4.3.6 **没有** `received` 字段，Rust 多出 `"received": "…"`。
  - int 的 issue：zod 带 `"format":"safeint"`，Rust 没有。
  - `invalid_format`：zod 的字段是 `origin:"string", code, format, pattern, path, message`，message 为 `Invalid ISO datetime` / `Invalid UUID`。Rust 缺 `origin` 和 `pattern`，message 为 `Invalid datetime` / `Invalid uuid`。
  - union：只有一个分支没有中止时（例如 held 分支只有格式类 issue），zod 直接返回该分支的 issues，不包 `invalid_union`。例如 `timestamp` 带偏移时，上游 cause 是单个 `invalid_format`（path 为 `heldMessages.0.timestamp`），Rust 包成了 `invalid_union`。
- `renderError` / `formatErrorEnvelope` 不渲染 cause，所以目前在 stdout/stderr 上看不出差异。
- 修复建议：以后要把 ZodError 当作数据暴露出去（例如 `--json` 或 trace）之前修齐。这一刀可以只在 HANDOFF 里登记。

## 已知偏差复核（不计数）

HANDOFF 已登记：`threadParentMessage.createdAt` 不可解析时，上游抛 RangeError（INTERNAL_BUG），Rust 输出空时间。复核后属实。另外，F3 说明除了"不可解析"，还有几种**可解析**的格式 Rust 也当成空，这一点应当并入 F3 一起处理。

---

结论：F1（safe-int）会让 Rust 发出上游会拒绝的写请求，修掉 F1 并补齐 F2 到 F5 的 zod/JS 原语之后，试点就可以通过行为一致性检阅。
