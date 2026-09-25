# 试点 `raft task claim` 评审 B：Rust 设计、契约符合度、测试质量

范围：工作区里未提交的 `crates/raft-cli/src/**`、`crates/raft-shared/src/**`、`crates/raft-cli/tests/fixtures/claim_format_oracle.json`、`crates/raft-cli/Cargo.toml`。字节级对照上游由评审 A 负责，这里只在 Rust 语义能直接证明出错时才提输出差异。

已复核：`cargo test -p raft-shared -p raft-cli --offline --lib`（26 + 2 通过）、`cargo clippy … --all-targets`（零警告）、`cargo fmt --check`（通过）。文中的数值结果来自 scratchpad 里一个依赖 `raft-shared` 的探针 crate，zod 行为是用 `upstream/raft-source/packages/shared/node_modules/zod`（4.3.6）在 Node 下跑出来的。

严重度：**阻塞**（接受这个试点当模板之前必须处理，处理方式可以是修代码，也可以是在 decisions.md 里明确改记录）；**应修**（下一刀之前修）；**可选**。

---

## 阻塞

### B1. handler、`ApiHttpClient` 和 `request_task_claim` 都是同步的，guide §8 要求 async
- 位置：`crates/raft-cli/src/core/command.rs:27,32`（`handler: fn(&CommandContext, A) -> Result<(), CliError>`），`crates/raft-cli/src/core/context.rs:104-111`（`trait ApiHttpClient { fn request(..) -> Result<..> }`），`crates/raft-shared/src/agent_api_claim.rs:397-400`（`transport: impl FnOnce(..) -> Result<..>`），`crates/raft-cli/src/agent_api_path.rs:32`。
- 违反的规则：mapping-guide §8 写的是 “`async function` → `async fn`”。上游 handler 是 `async (ctx, opts) => …`，`CliAgentApiHttpClient.request` 返回 `Promise<ApiResponse>`，`AgentApiRawTransport.request` 返回 `Promise`（`agentApiRawClient.ts:41-43`）。
- 为什么现在就要处理：下一刀要接 live HTTP（reqwest 只有 async 接口）。到那时同步签名得改三层：`DefinedCommand`、`ApiHttpClient`，还有 raft-shared 里的 raw client。每个照这个模板写出来的命令也都得改。现在改只涉及一个命令、一个 stub。
- 建议的最小改法：不引入新依赖，直接用 `std::pin::Pin<Box<dyn Future<Output = …> + 'a>>`（或者 §13 已经允许的 `futures::future::LocalBoxFuture`）：
  - `ApiHttpClient::request(&self, …) -> LocalBoxFuture<'_, Result<ApiResponse, RequestError>>`；
  - `DefinedCommand::handler: for<'a> fn(&'a CommandContext, A) -> LocalBoxFuture<'a, Result<(), CliError>>`；
  - `request_task_claim` 改成 `async fn`，transport 用 `AsyncFnOnce` 或返回 future 的闭包；
  - 测试里用一个很小的 `block_on`（`futures::executor::block_on`，或者 tokio current-thread）。
- 顺带记一条决策：D8 和 §8 定的是 CLI 用 current-thread runtime，所以 `CommandContext`、`CliIo` 不是 `Send` 不算问题（D6/D7 的 CoreActor 不承载 CLI handler，`__cli` 自己起 runtime），不需要为了 `Send` 去改 `Box<dyn FnMut>`。在 decisions.md 写一句 “CLI 的 future 是 `!Send`；需要派生任务时用 `spawn_local`/`LocalSet`”，免得下一刀有人为此去加 `Arc<Mutex>`。

### B2. `js::Value::String(String)` 和 `utf16_slice -> String`（有损）偏离 D3 的值模型
- 位置：`crates/raft-shared/src/js.rs:98`（`String(String)`），`js.rs:265-276`（`utf16_slice(value: &str, start: usize, end: Option<usize>) -> String`，注释自己写着 “Lone surrogates become U+FFFD because this unit stores text as Unicode scalars”）。
- 违反的规则：D3 写的是 “`js::Value` is `Undefined | Null | Bool | Number(f64) | String(JsString) | …`”，“`utf16_slice` and the other code-unit operations return it [JsString]”，“`json_stringify` escapes them as `\udxxx`”。§6.1 要求支持 “JS clamping and negative index rules”，现在的签名是 `usize`，表达不了负数下标。
- 为什么现在就要处理：`Value::String(text)` 已经在 schema、format、claim、测试里被模式匹配了几十处。下一刀的 `actionCards.ts:284` 这类切片会直接上线（D3 列了四个会进 sink 的切片点）。拖得越久，换类型要改的地方越多。
- 建议：二选一，而且必须明确选。(a) 现在就引入 `JsString(Vec<u16>)`，给 `Value::String` 用，`utf16_slice(&JsString, i64, Option<i64>) -> JsString`，写入 sink 时再 lossy 转换；(b) 在 decisions.md 里改 D3：`Value::String` 先用 `String`，列出因此不能正确还原的切片点，并定下什么时候切换。不能让代码和 D3 静默不一致。

---

## 应修

### S1. `CliErrorCode` 是封闭枚举，服务器透传的 errorCode 会被悄悄换掉
- 位置：`crates/raft-cli/src/core/errors.rs:7-108`，`crates/raft-cli/src/core/api_failure.rs:57,62,139-157`（`parse_known_code` 只认 14 个码，其余返回 `None`，然后走 fallback），`crates/raft-cli/src/core/context.rs:218-235`（`bootstrap_code` 不认识的码一律变成 `InvalidArg`）。
- 依据：上游 `errors.ts:1-2` 写明 “Server-passthrough codes keep their wire casing so callers can branch on the exact server contract”。`apiFailureError` 用的是 `(response.errorCode as CliErrorCode) ?? fallbackCode`，`cliError(code: string, …)` 和 `new CliError({code: err.code as CliErrorCode})` 也都接受任意字符串。mapping-guide §3 里 “string-literal union → enum” 的前提是闭集，而这里在运行时是开集。
- 失败场景：任何 4xx 响应带 `errorCode: "task_not_found"`（或者 `UNCLAIM_FAILED` 这类已经在枚举里、却不在 `parse_known_code` 里的码），Rust 输出 `Code: CLAIM_FAILED`，上游输出 `Code: task_not_found`。本刀只有 proxy-5xx 这一条路径会调 `api_failure_error`，所以还没触发；但下一刀几乎每个命令都会走到这里。
- 建议：加 `CliErrorCode::Wire(String)`（去掉 `Copy`，`as_str(&self) -> &str`），或者直接把 `code` 改成 `Cow<'static, str>`、已知码做成常量。`parse_known_code` 和 `bootstrap_code` 都删掉，原样透传。

### S2. `CliError` 不是 `Sync`，装不进 `anyhow::Error`；“caught unknown” 这一层缺失
- 位置：`errors.rs:337,366`（`cause: Option<Box<dyn std::error::Error + Send>>`），`errors.rs:471-505`（`InternalBugError::from_error(cause: CliError)`、`to_cli_error(error: CliError) -> CliError` 是恒等函数），`context.rs:87-91`（`RequestError::{Cli, Message(String)}`）。
- 依据：§4.2 写的是 “A caught `unknown` → `anyhow::Error`”，“`err instanceof X` → `err.downcast_ref::<X>()`”。`anyhow::Error::new` 要求 `Send + Sync + 'static`，而 `Box<dyn Error + Send>` 不是 `Sync`，所以 `anyhow::Error::from(cli_error)` 编译不过。上游 `toCliError(err: unknown)` 的意思是 “是 CliError 就原样返回，否则包成 InternalBugError”。Rust 版的输入类型已经是 `CliError`，这个分支根本表达不出来。`InternalBugError` 包的应该是非 CliError 的东西，现在签名反过来了。
- 建议：`cause: Option<anyhow::Error>`（或者 `Box<dyn Error + Send + Sync>`）；`to_cli_error(err: anyhow::Error) -> CliError`，里面先 `downcast::<CliError>()`，失败再走 `InternalBugError`；`RequestError` 直接换成 `anyhow::Error`，`cli_error_from_transport_failure` 里用 `downcast` 替代 `if let RequestError::Cli`。§13 已经列了 `anyhow`。

### S3. `std::env::vars()` 遇到非 UTF-8 环境变量会 panic
- 位置：`crates/raft-cli/src/core/context.rs:190-192`。
- 依据：`std::env::vars()` 的文档写明 “panics if any key or value in the environment is not valid unicode”。§7 要求 “On Unix, keys and values are decoded from the OS lossily as UTF-8, as Node does”。
- 失败场景：用户 shell 里只要有一个 Latin-1 的变量值，`raft task claim` 就直接 panic，而 Node 会正常运行。
- 建议：用 `std::env::vars_os()` 加 `to_string_lossy()`。另外 `HashMap<String, String>` 和 §7 的 `raft_shared::env::Env`（有序，Windows 下大小写不敏感）不一致，至少在 context.rs 加一条 `review:` 注释，说明等 `env` 模块落地后替换。`reviewer_isolation_enabled(&HashMap)` 同理。

### S4. auth/env.ts 的半截实现和占位客户端带着自编文案进了非测试代码
- 位置：`context.rs:254-377`（`default_load_agent_context`），特别是 `:304` 的 `"… credential file was not read; filesystem auth is not part of this unit"`、`:344` 的 `"SLOCK_AGENT_PROXY_TOKEN_FILE was not read; filesystem auth is not part of this unit"`、`:296` 的 `eprintln!`；还有 `context.rs:175-188` 的 `UnconfiguredApiClient`（`"HTTP client is not part of this unit"`）。
- 违反的规则：§1.5 “Never leave … placeholders. If something in scope cannot be translated yet, stop and report it”；§11 “All user- or agent-visible strings are copied character for character from the source”；§9 “stdout/stderr writes through the injected Io”（`eprintln!` 绕过了 `ctx.io`，测试也捕获不到）；§1.1（这是 `auth/env.ts` 的内容，却写在 `core/context.rs` 里）；§6（`:260` 用的是 Rust `str::trim`，不是 `js::trim`）。
- 说明：HANDOFF 明确说 auth/env.ts 不在本刀范围，所以缺失不算问题。问题在于代码里放了一个会给出错误文案的假实现。
- 建议：本刀删掉这两个默认值，让 `CommandRuntimeOptions` 里的 `load_agent_context` / `create_api_client` 在试点期间必填（或者 `create_command_context` 返回 `Result`，调用方必须注入）。下一刀在 `crates/raft-cli/src/auth/env.rs` 里整份移植。

### S5. `register_cli_command`、`Command`、`RegisteredCommand` 是 stub
- 位置：`crates/raft-cli/src/core/command.rs:57-96`（`let _ctx = create_command_context(..); … let _handler = command.handler;`）。
- 违反的规则：§1.5 “Dropped code is deleted, never stubbed … empty functions”。而且它在注册时建了一次 context 就扔掉了，上游是在每次 `.action` 回调里新建（`command.ts:57-58`）。照这个样子接 commander 是错的。
- 另外一个下一刀会碰到的问题：`create_command_context(options: CommandRuntimeOptions)` 按值消费 `Box<dyn Fn>`，而上游每次 action 都从同一份 `runtimeOptions` 建新的 ctx。接上 commander 之后 options 必须能复用：改成 `Rc<dyn Fn>`，`CommandRuntimeOptions: Clone`，或者 `create_command_context(&CommandRuntimeOptions)`。
- 建议：删掉 `Command`、`RegisteredCommand`、`register_cli_command`，保留 `review:` 注释。把可复用的问题写进 HANDOFF 的 “下一刀”。`command_action_result` 没有调用者，也没有测试（见 T5），要么补测试，要么一起删掉。

### S6. 模块布局不符合 “一个 TS 模块对应一个 Rust 模块、路径相同”
- 位置和依据（§1.1 “One TS module → one Rust module, same relative path”；§2 “`raft_shared::…` using the same module path as the TS source file”；§1.3 头注释）：
  - `crates/raft-shared/src/agent_api_claim.rs:1` 的头注释自己写着它合并了 `agentApiContract.ts`、`agentApiMessageContract.ts`、`agentApiPaths.ts`、`agentApiRawClient.ts` 四个文件。上游没有 `agentApiClaim.ts`，路径应该是 `raft_shared::agent_api_contract`、`agent_api_raw_client`、`agent_api_paths` 等。`request_task_claim` 是把通用的 `requestAgentApiRawRoute<K>` 手工单态化了，上游的 `routeKey`、`missing_route`、`missing_path_param`、`response` 字段在类型上都不见了。
  - `crates/raft-cli/src/core_time.rs` 在上游没有对应文件（上游是 `freshness/_format.ts:40` 里内联的 `new Date(ts).toISOString().slice(11, 16)`）。
  - `crates/raft-cli/src/agent_api_path.rs` 在 `agentApiPath.ts` 和 shared 的 `agentApiClient.ts`（`apiResponseFromClientResult` / `cliErrorFromClientFailure`）之间自己压平了一层，这一点跟 D2 “translated line for line” 也有出入。
- 为什么是应修：下一刀要往 `agentApiContract` 里加路由。如果照这个样子写，会变成每个路由一个 `agent_api_<route>.rs`，和 guide 越走越远。
- 建议：把 schema 挪到 `raft_shared::agent_api_contract`（只放本刀用到的 schema，顺序和上游一致），把 raw client 挪到 `raft_shared::agent_api_raw_client`（先只做 `request_agent_api_raw_route`，按 route key 泛化）。`core_time::hhmm` 内联回 `freshness/format.rs`，删掉 `core_time.rs`。上游 `_format.ts`、`_target.ts`、`_apiFailure.ts` 去掉前导下划线的命名方式，请在 mapping-guide §1.1 里补一句规则。

### S7. `number_to_string` 不符合 V8 规则，会改变 claim 请求体的字节
- 位置：`crates/raft-shared/src/js.rs:444-466`。
- 依据：D3 和 §6.3 要求 “V8 number formatting … exponent at ≥1e21”。探针结果（右边是 Node 24 的输出）：
  - `1e21` → `1000000000000000000000`（JS：`1e+21`）
  - `1e-7` → `0.0000001`（JS：`1e-7`）
  - `123456789012345680000` → `123456789012345683968`（JS：`123456789012345680000`；`{:.0}` 打印的是精确十进制展开，不是最短往返表示）
- 失败场景：`raft task claim --target '#x' --number 1e21` 能通过 `is_integer && > 0`，Rust 发出的是 `{"channel":"#x","task_numbers":[1000000000000000000000]}`，上游发的是 `[1e+21]`。任何大于 2^53 的整数都会打出多余的数字。
- 建议：实现 ECMAScript Number::toString，用 `ryu` 或 `{:e}` 取最短位数，再按 k、n 的规则决定用定点还是指数。补上面三个值的单元测试。

### S8. `schema::string().datetime()` 比 zod 4.3.6 宽松
- 位置：`crates/raft-shared/src/schema.rs:696-706`。
- 依据：在 zod 4.3.6 里，`z.string().datetime()` 对 `"2026-01-01T00:00:00+08:00"` 和 `"2026-01-01t00:00:00Z"` 的 `safeParse` 都是 `false`（默认不允许 offset，而且只接受大写 `T`），Rust 两个都接受。这是契约行为，不是纯输出格式：带 offset 的 `heldMessages[].timestamp` 在上游会得到 `INVALID_JSON_RESPONSE`，在 Rust 会正常渲染。
- 建议：按 zod 的 datetime 正则来实现（`Z` 必须有，除非 `offset: true`；`T` 必须大写；秒以下精度任意）。这一条和评审 A 的范围重叠，请 A 顺手把 zod golden 覆盖到。

### S9. 测试函数名不符合 §1.6 的转写规则（17 个里有 9 个）
- 规则：“function name is the title transliterated to ASCII, lowercased, with every run of other characters replaced by `_` … truncated to 80 chars”。
- 不符合的函数：
  - `commands/task/format_tests.rs` 全部 8 个：规则得到的是 `formatclaimresults_mixed_success_and_failure`，实际写成了 `format_claim_results_…`（驼峰被拆开了）。其中 `format_claim_conflict_copy_is_projected_not_hardcoded` 和 `format_claim_conflict_placement_is_one_contiguous_block` 还删改了标题里的词。
  - `commands/task/claim_tests.rs` 里三个超过 80 字符却没有截断的：`reviewer_isolation_task_claim_uses_withheld_mode_and_reprojects_poisoned_legacy_hold_output`、`zero_success_claim_with_a_structured_conflict_throws_typed_claim_conflict_naming_the_holder`、`zero_success_claim_without_a_conflict_throws_typed_claim_failed_carrying_the_reason`。
  - `// test:` 那一行和上游标题逐字一致，已核对。
- 建议：按规则机械改名。`tools/test-parity` 目前按 `// test:` 行匹配，所以这不影响 parity。但这条规则就是为了让函数名可以机械生成，试点应该先守住。

### S10. 测试没有钉住 HANDOFF 要求保住的几项行为（claim_tests.rs）
- **path 和 method 没有断言**：`StubClient::request` 忽略了 `_method` 和 `_path`（`claim_tests.rs:71-86`），没有任何测试证明请求发的是 `POST /internal/agent-api/tasks/claim`。建议把 `(method, path, body)` 一起记录下来并断言。
- **四个键的顺序没有在 handler 层测过**：HANDOFF 要求 body 键顺序是 `channel, task_numbers, message_ids, freshnessContextMode`，但 claim_tests 里没有任何一个用例传 `message_id`。四键顺序只在 `agent_api_claim_tests.rs:27-36` 里对 schema 测过，而且那个用例没有 `task_numbers`。建议加一例：`--number 87 --message-id " m1 " --reviewer-isolation`，断言 `json_stringify` 的结果等于 `{"channel":"#x","task_numbers":[87],"message_ids":["m1"],"freshnessContextMode":"withheld"}`。这一例同时覆盖 trim、`--reviewer-isolation` 旗标（目前 handler 层只测了环境变量那条路径）和整数 `87`。
- **断言查错了对象**：`input_guards_run_before_any_request`（`claim_tests.rs:455-462`）用 `RAFT_REVIEWER_ISOLATION=maybe` 新建了一个 Harness 然后丢掉，最后断言的 `h.requests().is_empty()` 查的是另一个 `h`。环境变量守卫在发请求之前生效这件事其实没有被验证。
- **`--number` 大数没有测**：见 S7。建议在 `number_flag_uses_js_number_coercion` 里加 `"1e21"` 和 `"123456789012345680000"`，对 `json_stringify` 做断言。

---

## 可选

### O1. `lib.rs` 在整个 crate 放行了两个 clippy lint
- 位置：`crates/raft-cli/src/lib.rs:3`。
- 分析：`large_enum_variant` 只由 `RequestError::Cli(CliError)` 和 `CommandThrown::Error(CliError)` 触发。在这两个变体里包一层 `Box<CliError>`（S2 做完后 `RequestError` 会直接消失），这个 allow 就可以删掉。`result_large_err` 可以留着，但请在 decisions.md 里记一条（CliError 和上游逐字段对应、只走失败路径），不要只靠 lib.rs 里的注释。crate 级的 allow 会把以后真正该报的 lint 也吞掉。

### O2. 死代码和没人用的 pub 项
- 以下各项在 crate 内没有调用者（已 grep 核对）：
  - raft-shared 的 `utc_timestamp.rs` 整个文件（`format_utc_timestamp`）
  - `renderer.rs:17-30,54,229-239` 的 `AxSurfaceExample`、`ax_surface`、`NL`、`FORMAT_ERROR_ENVELOPE_DESCRIPTION`、`write_diagnostic`、`write_json`
  - `errors.rs:474-486,503-505` 的 `InternalBugError::from_error`、`to_cli_error`
  - `js.rs` 的 `is_truthy_str`、`trim_end`、`math_floor`、`sort_default`、`string_replace_all`
  - `agent_api_claim.rs:6` 的 `AGENT_API_BASE_PATH`
  - `core_time::is_parseable`
  - `commands/api_failure.rs` 的 re-export
- 需要特别指出的是 `string_replace_all`（`js.rs:280-285`），它有 bug 却占着 D3 规定的 helper 名字：`$&` 不展开（探针：`("a.b", ".", "$&$&")` 得到 `"a$&$&b"`，JS 是 `"a..b"`），空 pattern 直接返回原串（JS 的 `"ab".replaceAll("", "-")` 是 `"-a-b-"`）。§6.11 也禁止用 `str::replace` 来实现它。
- 建议：删掉这些。等真正需要时，再按 D3 带着 golden 一起补。

### O3. `schema.rs` 的小问题
- `schema.rs:519-527` `number_to_index` 用 `f64` 逐次减 1 的循环来算下标。输入 ≥ 2^53 时 `remaining -= 1.0` 不再改变值，会死循环；很大的输入就算能结束也是 O(n)。目前 `custom_issue` 只产生字符串路径，这个分支到不了。建议 `Issue` 内部直接存 `Vec<Seg>`，只在 `to_value` 时转成 JS 值，这样可以删掉这段往返转换。
- `schema.rs:181-207`：`.trim()`、`.int()` 这类方法调用在不匹配的类型上时（比如写成 `string().optional().trim()`）会静默什么都不做。转写的时候如果把顺序写错，不会有任何报错。建议改成 `panic!`（schema 是编译期常量级别的东西），或者让 `StringSchema` 成为单独的类型。
- `super_refine(fn(&Object) -> Vec<Issue>)` 用的是函数指针，D2 写的是 “`refine`/`superRefine` as closures”。等需要捕获变量的 refine 出现时再改也可以，但请在注释里说明。
- 每次调用 `agent_api_task_claim_body_schema()` 都要重建整棵 `Arc` 树，可以改用 `LazyLock<Schema>`。

### O4. `TaskClaimTransportResponse.data` / `ApiResponse.data: Option<Value>` 把 null 和 undefined 混成了一种
- 位置：`agent_api_claim.rs:361,449`，`api_failure.rs:26`。
- 上游只在 `response.data === null` 时判定为 `empty_response`（`agentApiRawClient.ts:283`）。现在 Rust 里 `None` 和 `Some(Value::Null)` 是两种表示，`Some(Value::Undefined)` 又是第三种。建议字段直接用 `Value`，按 JS 的比较规则判断。

### O5. `freshness/format.rs` 里的小偏差
- `:24-34` 在本地又实现了一遍 `Number(value)`，而且 `Array` 一律返回 `NaN`（JS 里 `Number([])` 是 0，`Number([5])` 是 5）。§6 要求 “Use the helpers in `raft_shared::js`. Do not re-implement them inline”。建议挪到 `js::to_number_value(&Value)`。
- `:124` `items.len() as f64` 是整数到浮点的裸 `as` 转换，违反 §4.6。可以用 `f64::from(u32::try_from(len)?)`，或者在 js 里加一个 helper。

### O6. `task/format.rs:34-39` `canonical_task_target`
- 已核对：`target.get(..8)` 返回 `Some` 就说明第 8 字节是字符边界，所以 `&target[8..]` **不会** panic。另外 `/^dm:user:/i` 是非 `u` 正则，按规范 Canonicalize 不会把非 ASCII 字符映射到 ASCII，所以 `eq_ignore_ascii_case` 的语义和它一致。
- 但 §6.2 要求翻译过来的正则用 `js::JsRegex`。在 JsRegex 落地之前，请在这里加一条 `review:` 注释，写明等价性和理由。

### O7. `CliReplyText(pub String)` 让品牌类型失去了约束
- 位置：`renderer.rs:9`。上游的 `CliReplyText` 只能由 `axSurface` 和 `adoptCliReplyText` 构造。字段公开以后，任何代码都能直接 `CliReplyText(x)`，print-seam 的约束就没有了。建议把字段改成私有，加 `as_str`，只通过 `adopt_cli_reply_text` 构造。

### O8. `CliIo` 是一组闭包，不是 §9 说的 `Io` trait
- 位置：`core/io.rs:7-13`。§9 写的是 “through the injected `Io` trait … `Io` can hand a child the real fds (D8)”。`forwardManagedTransportIfNeeded` 需要把真实 fd 交给子进程继承，闭包做不到这件事。另外写入的是 `&str`，Node 的 `stream.write` 接受的是字节。本刀不需要改，但请在 HANDOFF 的 “下一刀” 里记下来：`CliIo` 要改成 trait，提供 `write_stdout(&[u8])` 和 `inheritable_fds()`。

### O9. `proxy_upstream_status: Option<u16>`
- 位置：`api_failure.rs:13`，`errors.rs:349,378`。这个值来自服务器 JSON 里的 proxy 诊断字段，没有经过 int 校验。按 §3，这类数字应该用 `f64`，否则接 live HTTP 时需要一次有损转换。

### O10. 测试的小问题
- `reviewer_isolation_tests.rs:39` 的 `let _typed: CliError = err;` 是类型层面的恒真断言（上游对应的是 `instanceof CliError`），删掉就行。
- `agent_api_claim_tests.rs:49-64` 只用 `contains` 检查 issue 的 message 子串。D2 要求用 `tools/zod-golden/` 的语料，对 issues 的完整内容和键顺序做逐字比较。建议至少对 `ZodError.message()` 整体做 `assert_eq!`，期望值从 oracle 取。
- oracle fixture 放在 `crates/raft-cli/tests/fixtures/claim_format_oracle.json`，旁边没有 `.cmd` 文件。§12 要求 “Golden fixtures captured from the oracle live in `tests/golden/<crate>/…` with the exact command that produced them in a sibling `.cmd` file”。建议挪到 `tests/golden/raft-cli/claim_format_oracle.json`，并补上生成命令。
- `render_error` 和 `command_action_result` 完全没有测试。零成功时 stderr 上的 `Error:/Code: CLAIM_CONFLICT/Next action:` 信封，以及最终退出码 1，都没有被验证（`task_claim_exit_status…` 只检查了 `err.exit_code`）。建议加一例：handler 返回 Err 后经过 `command_action_result`，断言 stderr 的完整内容和 `CliExit(1)`。
- `test_support::js` 的转换逻辑是可靠的：`preserve_order` 保留了 `json!` 字面量里的插入顺序，`Object::from_pairs` 再按 V8 规则把数组下标键提到前面，这和 JS 对象字面量的语义一致；没有开 `arbitrary_precision` 时 `as_f64()` 不会返回 `None`。唯一的问题在依赖声明：`crates/raft-cli/Cargo.toml:17` 直接写了 `serde_json = { version = "1", features = ["preserve_order"] }`，没有走 `[workspace.dependencies]`，缺了 §13 要求的 `float_roundtrip`，而且 `preserve_order` 这个 feature 在 decisions.md 里没有记录（§13：“Adding any other crate or feature needs a note in `decisions.md`”）。

---

## 结论

**不通过（需要修改）**：共 2 个阻塞项（B1 handler 和 client 同步，与 §8 冲突；B2 `Value::String` 偏离 D3 的 `JsString`，需要修代码或者改 D3 记录）和 10 个应修项，下一刀之前需要处理完。handler 的逻辑本身、JS `Number()` 的强制转换、零成功退出策略、reviewer isolation 的隐去逻辑都没发现问题。
