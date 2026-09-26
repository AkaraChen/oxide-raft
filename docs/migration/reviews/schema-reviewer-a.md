# raft_shared::schema — 对抗评审 A

结论：**有条件通过**。没有阻塞项，有 2 个应修项和 3 个可选项。

- 语义层面（输出值、V8 键序、issue 的字段、键序、路径和英文消息，以及 `error.message` 的 JSON 格式）很扎实：定向探针和随机差分模糊测试都没有发现差异。
- 问题集中在两处：
  - url.rs 与 Node 24 的 IDNA 规则不一致，其中一条让新 golden 失败。
  - 每次解析都会整份深拷贝输入，每尝试一个 union 分支又拷贝一次。在大响应上这比 zod 慢 5–30 倍。

Oracle 与探针：
- Oracle 是 Node 24.15.0（x64）和 zod 4.3.6（通过 upstream/raft-source/packages/shared 的 createRequire 加载）。
- 探针代码在 scratchpad/zodReviewA/。那是 crates/raft-shared/src 的一份副本，只去掉了 `json_schema` 模块，因为另一个单元正在修改它，当前无法编译。schema 和 js 的代码没有改动。

## 新 golden（311 条）的核对结果

我在 scratch 副本的 golden_tests.rs 里做了三处改动：
- 补上 `noNull`、`never`、`issueNoCode`、`issueNested` 四个具名 refinement。
- 支持 `"$json:"` 输入（用 `js::json_parse` 解析）。
- 去掉 `cases.len() == 264` 的断言。

改完后，现有实现对 311 条中的 310 条输出完全一致，包括 data、issues 的键序和 `error.message`。唯一的差异是下面的发现 1（`z.url.trim` 的 `"http://xn--a-/"`）。

以上三处 harness 改动属于修复者的工作。另外，url.rs 自带的单元测试把 `"http://xn--a-/"` 列为合法（url.rs:482 起的 `ok` 列表），与 Node 24 相反，也需要一并翻转。url.rs 单元测试中的其余 56 个断言与 Node 24 一致。

## 已验证无差异的范围（不在 golden 集中）

**定向差分**：约 280 个用例，与 zod 逐字节比较 `JSON.stringify(data | issues)` 和 `error.message`，全部一致。覆盖：
- 消息里的数字格式：`max(1e21)` 显示为 `<=1e+21`；还有 `min(1e-7)`、`1.2345678901234568e+21`、`-0` 的输出与消息、`z.int()` 对 1e300 和 ±2^53 的 too_big/too_small（origin 为 `int`，含 note）。
- `z.coerce.number()`：48 种输入，包括 ` `、`﻿`、` `（剥掉）和 `\u0085`、`᠎`、`​`（结果为 NaN），`0b`/`0o`/`0X`、`-0x10`、`+0x10`、`1_0`、`""`、`"  "`、`±Infinity`、`1e1000`、`[[]]`、`[["7"]]`、`[1,2]`、`{}`、`null`、`true`、`undefined`；`received: "Infinity"`/`"NaN"` 的位置也一致。
- trim：完整的 JS 空白集合，含 U+FEFF，不含 U+0085/U+180E/U+200B；也覆盖了两端带孤立代理项的情况。
- UTF-16 长度：emoji、孤立代理项、组合字符。
- `toLowerCase`：对全部 1,112,064 个码点逐一比较（按 500 个一批，带上下文），**0 差异**，包括 final sigma、İ、Unicode 16/17 新增的大小写对，以及孤立代理项两侧的 Σ。
- `datetime({offset:true})` 的 11 个边界；`uuid` 的版本位和大写形式。
- `literal(true/1)` 以及 `-0` 按 SameValueZero 命中；enum 的数字键序（`4294967295` 与 `4294967294`）。
- passthrough 与 strict 的键序；`__proto__`；record 的数字重试和 enum 键。
- discriminatedUnion：数字、false、null 作判别值，`-0` 命中 `1`/`0`，数组输入。
- union 的 nonaborted 单分支规则；union 嵌套 union 的 errors 结构。
- refine/superRefine：空消息 `""`（refine 保留 `""`，superRefine 回落为 `"Invalid input"`）、嵌套数组路径、在 abort 后跳过。
- default、optional、nullish 的组合；contracts 里的 app-config union（两个 strict object 组成的 union）和 patch body。
- 错误 JSON 中的孤立代理项、U+2028、控制字符、引号和反斜杠。

**随机差分模糊**：scratchpad/zodReviewA/fuzz.mjs 复用 combinators.mjs 的 spec 语法和 build 函数，随机生成 schema（深度不超过 3，覆盖全部已支持的 kind、check 和 wrap）以及与之相匹配或扰动过的输入。9 个种子共 **143,160 条用例**，其中 104,647 条为失败用例，issue 码分布为：invalid_type 7 万、invalid_format 5.5 万、invalid_value 1.4 万、unrecognized_keys 9k、custom 9k、invalid_union 8k、too_big/too_small 1.3 万、invalid_key 2k。结果 **0 差异**。harness 的有效性已验证：篡改一条记录的消息后，测试能报出该差异。

**contract 覆盖**：我逐一核对了 8 个受管文件用到的组合子和选项。除发现 5 外，全部都有对应实现，包括：
- `.extend` 的 `{...base, ...incoming}` 顺序，以及对带 check 的对象覆盖键时 panic；
- `.meta`/`.describe` 无副作用；
- `z.ZodIssueCode.custom`、superRefine 路径、refine 字符串参数；
- discriminatedUnion 的 enum 判别值和 optional 判别值；
- 值为 `z.object({}).passthrough()` 的 `record`、`.default({})`/`.default([])`；
- `z.int().positive().max(N)`、`.int().safe()`、`.finite()` 无操作；
- `datetime({ offset: true })`、带 `u` 标志的 `.regex(/…/u)`。

daemonApiRawClient 从 issue 中只读取 `code`、`path`、`expected`、`received`，Rust 的 `ZodIssue::get` 都能提供。

**不可信输入与 panic**：
- 我检查了 parse.rs、issue.rs 和 url.rs 中所有的下标、`unwrap` 和算术。可能 panic 的点只在 builder 阶段：`z::union([])`、discriminatedUnion 选项不合法或判别值重复、`with_message` 前面没有 check、`IssueSpec::code` 传入非 custom 码。
- url.rs 中 punycode 的 `adapt`、`checked_*`、端口的饱和运算、IPv4 和 IPv6 的计算都不会溢出：6 万条 ASCII 随机 URL 与 `URL.canParse` 结果完全一致，也没有发生 panic。
- `Value` 的 clone、drop 和 display 都是迭代实现，schema 递归深度只受 schema 本身嵌套深度限制。深嵌套输入经 `z.unknown()` 或 `record(string, unknown)` 不会栈溢出。

**Send + Sync**：`Schema`、`ZodError` 都是 Send + Sync。例外情况见发现 4。

## 发现

### 1. [应修] url.rs 接受 `xn--` 标签解码后全为 ASCII 的主机，Node 24 拒绝（新 golden 失败）
- 位置：url.rs `domain_to_ascii` 中处理 `xn--` 的分支。它只拒绝解码结果为空、含 ASCII 大写或含 C1 控制字符的情况。url.rs 的模块文档称"ASCII host 精确"，这一说法因此不成立。
- 输入：`z.url().safeParse("http://xn--a-/")`（golden 中的 `z.url.trim` 组）。
  - zod（Node 24.15.0）：`{"code":"invalid_format","format":"url","path":[],"message":"Invalid URL"}`
  - Rust：`Ok("http://xn--a-/")`
- 同类输入：`http://xn--ab-/`、`http://a.xn--a-/`、`ws:xn--8080a-`、`file://xn--a-/`，还有 `ws:%2exn--xn--ab-`（百分号解码之后才出现 xn-- 标签）。这些都来自 6 万条 ASCII 模糊测试，是其中仅有的差异类别。
- 原因：Node 24 的 ada 实现了较新的 UTS #46：以 `xn--` 开头的标签，如果 Punycode 解码结果全是 ASCII，就判为错误。Node 22 不做这项检查，所以旧的单元测试把它当作合法。
- 影响：golden 失败，D2 的证明不成立。实际触达面是 attachmentUploadContract 中的 `thumbnailUrl` 和 `url`（服务端响应）。
- 建议：
  - `punycode_decode` 成功后，如果 `decoded.iter().all(char::is_ascii)` 就返回 `None`。
  - 把 url.rs 单元测试中的 `"http://xn--a-/"` 从 ok 列表移到 bad 列表。
  - 修正模块文档。

### 2. [应修] 每次解析都深拷贝整个输入，union 每尝试一个分支再深拷贝一次；大响应上比 zod 慢 5–30 倍
- 位置：
  - parse.rs:34 `parse()` 调用 `Payload::new(value.clone())`；
  - parse.rs:292 union 分支中的 `run(option, Payload::new(p.value.clone()))`。
- 输入：release 构建，同一台机器。数据是 50,000 条任务，每条形如 `{taskNumber, title, body: "x"*200, tags:[…], meta:{…}}`，按 agentApiTaskClaimResponseSchema 的结构测量，即 `union([passthrough{results: array(passthrough{taskNumber})}, passthrough{ok?, state: literal("held")}])`：

  | schema | zod | Rust |
  |---|---|---|
  | 直接用对象 schema | 19 ms | 91 ms |
  | `z.unknown()` | ≈0 | 91 ms（几乎全是这次深拷贝） |
  | union，成功分支在前 | 16 ms | 208–223 ms |
  | union，held 分支在前 | 11 ms | 357–370 ms |

  单独深拷贝加释放一次这个值就要 110 ms。极端情况：`z.union([z.string(), z.number(), z.boolean(), z.unknown()])` 解析一个有 40 万个键的对象，zod 0.2 ms，Rust 697 ms。
- 影响：结果没有错。但 events、history、task 这类大响应每次都要付出上面的代价，外层包 union 的响应（`agentApiTaskClaimResponseSchema`、`…UpdateStatusResponseSchema`、`…AmendResponseSchema`、`agentApiSendResponseSchema` 是判别联合，不受影响）按尝试过的分支数成倍增加。
- 建议：
  - 解析时借用输入（`&Value`），输出时按需构造，就像 zod 那样新建 `payload.value = {}` 而不改输入；
  - 或者至少提供 `parse_owned(Value)` 跳过顶层拷贝，并让 union 在分支失败时不保留、不复制整值（例如只在第一个分支之前拷贝一次，或者分支借用输入）。

### 3. [可选] 非 ASCII 主机的 IDNA 映射和有效性只是近似实现，Rust 会接受许多 Node 拒绝的 URL
- 位置：url.rs `domain_to_ascii` 的非 ASCII 分支。任何非 ASCII 标签都被当作 `xn--a` 放行。模块文档已声明这是近似实现。
- 输入：以下 URL 在 zod（Node 24）中都得到 `Invalid URL`，Rust 都接受：
  - `http:// a/`、`http://a /`、`http://　/`（空白类字符映射成空格，而空格是禁止的）；
  - `http://a‌b/`（ZWNJ 上下文规则）；
  - `http://̀a/`（以组合字符开头）；
  - `http://אa/`（bidi 规则）；
  - `http://⒈/`、`http://․/`、`http://℀/`（disallowed）；
  - `http://͸/`（未分配码点）；
  - `http://xn--a-ecp/`（解码为 `a⒈`）。
  - 另外，这些主机换成 `file://` 前缀时也有同样差异。
- 方向：差异都是单向的，只有 Rust 更宽松。代表性样本中没有发现 Rust 拒绝而 Node 接受的情况。
- 影响：只影响两个响应字段（`thumbnailUrl`、presigned `url`），而且只在服务端返回畸形主机时才会出现。
- 建议：引入 UTS #46 映射表和有效性检查（例如 `idna` crate，把版本固定到与 Node 24 的 ada/ICU 相同的 Unicode 版本）。如果不引入，就在 D2 中把它登记为已知差异。

### 4. [可选] 共享的 `/g`、`/y` 正则在并发解析时会误判
- 位置：parse.rs:467-468。它先 `pattern.set_last_index(0)`，再 `pattern.test(s)`。`lastIndex` 是 `JsRegex` 里的一个共享 `AtomicUsize`，两步之间不是原子的。
- 输入：8 个线程共用一个 `static LazyLock<Schema> = z::string().regex(JsRegex::new("a","g"))`，各对 `"a"` 解析 20 万次：1,600,000 次中出现 **1 次** 失败，报 `invalid_format`。zod 是单线程的，结果永远是成功。
- 影响：受管的 contracts 中所有 `.regex()` 都没有 g/y 标志，目前无法触发。但 `schemas_can_live_in_statics` 测试明确承诺 schema 可以作为 static 共享。
- 建议：对 g/y 正则，在检查中使用局部 lastIndex，即以 0 为起点做一次不改共享状态的 exec；或者在 builder 中克隆一个私有 `JsRegex`，并在 `CheckKind::Regex` 中用 `test_at(s, 0)` 这类无状态的 API。

### 5. [可选] 缺少 `.shape` 访问器，有一处 contract 无法逐行翻译
- 位置：agentApiContract.ts:987，`action: agentApiIntegrationAppManageBodySchema.shape.action`。Rust `Schema` 没有读取对象 shape 字段的公开 API（`ObjectDef` 是 `pub(crate)`）。
- 影响：翻译者只能把内层 `z.enum([...])` 提成单独变量，偏离 D2 的"line for line"要求，行为上不受影响。
- 建议：增加 `pub fn shape_field(&self, key: &str) -> Option<Schema>`，或者在 mapping-guide 的 §5 中注明允许这样提取。

## 统计
- 阻塞：0
- 应修：2（xn-- 解码全 ASCII；解析和 union 的深拷贝性能）
- 可选：3（非 ASCII IDNA 近似；g/y 正则并发 lastIndex；`.shape` 访问器）
