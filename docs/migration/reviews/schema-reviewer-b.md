# raft_shared::schema：对抗审查 B（差分模糊测试、issue 形状、翻译接口）

**结论：不通过。** 共 1 个阻塞项、4 个应修项、4 个可选项。

- 阻塞项：新 golden（311 条）里有 1 条 Rust 复现不了。`z.url()` 会接受 `http://xn--a-/`。
- 应修项：
  - `url.rs` 把非 ASCII 标签整体替换成 `xn--a`，因此禁用字符和 UTS #46 有效性都被跳过（S1）。
  - 共享的 `JsRegex` 在多线程下会竞争 `lastIndex`（S2）。
  - `.finite()` 配合 `.with_message()` 时，消息会挂到前一个检查上，或者直接 panic（S3）。
  - 没有 `.shape` 访问器（S4）。

除此之外，差分模糊测试共跑了约 15 万条随机 schema × 输入，在契约实际用到的组合子范围内没有找到其他输出或 issue 差异。

审查范围：`crates/raft-shared/src/schema/*.rs`（快照时间是文件 mtime 03:03–03:06）。

探针全部放在 `…/scratchpad/zodReviewB/`：
- `gen.mjs` 随机生成 schema 和输入，`common.mjs` 是 JS 端的构建器，`src/main.rs` 是 Rust 端的差分 harness。
- `examples/{url,lower,coerce,spot,race,mini,deep,perf}.rs` 是专项探针。

对照环境是 Node v24.15.0 + zod 4.3.6（`createRequire` 取自 `packages/shared`）。

环境说明：仓库里的 `crates/raft-shared/src/json_schema/mod.rs` 引用了尚不存在的 `compile.rs` 等文件（另一个单元还在写），所以现在 `raft-shared` 整体编译不过。探针 crate 用的是 `js/` 和 `schema/` 的快照副本，`lib.rs` 去掉了 `json_schema` 模块，依赖版本和仓库的 `Cargo.lock` 保持一致。

---

## 阻塞

### B1. 新 golden 有 1 条复现不了：`z.url()` 接受 `http://xn--a-/`
- 位置：`url.rs:232-245`（`domain_to_ascii` 里处理 `xn--` 的分支）。
- 违反的规则：D2 "the Rust runtime must reproduce each record"。
- 复现方法：
  - 我在自己的 harness 里补上了 golden 新增的 4 个具名 refinement（`noNull`、`never`、`issueNoCode`、`issueNested`），其余用仓库 `golden_tests.rs` 的构建逻辑，然后跑 `tests/golden/raft-shared/zod-combinators.json`。
  - 结果是 **310/311 一致**。唯一的差异是：

    ```
    z.url.trim input="http://xn--a-/"
      rust ok "http://xn--a-/" ; zod issues [{"code":"invalid_format","format":"url","path":[],"message":"Invalid URL"}]
    ```
  - 同一类的还有 `http://xn--abc-/`、`http://xn--9ca-/`、`http://xn--a-ecp.ru/`：Node 都拒绝，Rust 都接受。
- 原因：
  - `punycode_decode("a-")` 得到 `['a']`，现有代码只检查"非空、没有大写、没有 C1"。
  - UTS #46 的 Processing 要求：`xn--` 标签解码后如果为空或全是 ASCII，就记为错误；解码后的标签还要通过有效性检查（NFC、映射后不变等）。
- 仓库测试的现状：`golden_tests.rs:309` 仍然断言 `cases.len() == 264`，`apply_refinement` 也没有那 4 个 refinement（会 panic `unknown refinement noNull`）。这一点符合任务说明，由修复者补接线。补完之后仍然会停在这一条上。
- 修复建议：
  1. 解码结果全是 ASCII 时返回 `None`。
  2. 更彻底的做法是把 S1 一起解决。

---

## 应修

### S1. `url.rs` 把非 ASCII 标签整体换成 `xn--a`，禁用字符和 UTS #46 有效性都被跳过
- 位置：`url.rs:210-258`。非 ASCII 标签会被 `labels.push("xn--a")` 顶替，之后才做 `is_forbidden_domain` 检查。
- 违反的规则：D2 要求结果与 `new URL()` 相同。这个检查的契约使用点是 `attachmentUploadContract.ts:31`（`thumbnailUrl: z.url().nullable()`）和 `:65`（`url: z.url()`），都用在服务端响应上。
- 模糊测试：30,000 条随机 URL（`genurl.mjs`，组合了 scheme、分隔符、userinfo、host、port 和尾部，再随机插入字符），**有 489 条不一致，全部是 Rust 接受而 Node 拒绝**，没有反方向的情况。最小复现如下（左边是 Node 的结果，Rust 全部接受）：

  | 输入 | Node | 类别 |
  |---|---|---|
  | `http://é x/` | ERR | 禁用字符（空格）被同一标签里的非 ASCII 字符掩盖 |
  | `http://é<x.com/`, `http://é^x.com/`, `http://é\|x/` | ERR | 同上 |
  | `http://é%zz/`, `http://é%3Cx/` | ERR | `%` 或解码后的 `<` 被掩盖 |
  | `file://é@x/` | ERR | file host 里的 `@` 被掩盖 |
  | `https://attacker.com .example/`, `http://a b/` | ERR | NBSP 在 UTS #46 里映射成空格，应当拒绝 |
  | `http://a‍b/`, `http://a‌b/` | ERR | CONTEXTJ |
  | `http://ab/`, `http://a\u{e0001}b/`, `http://a\u{10ffff}b/`, `http://⒈.com/` | ERR | disallowed 码位 |
  | `http://́a/` | ERR | 标签以组合字符开头（V5） |
- 影响：服务端给出的这类 URL，TS 客户端会判定响应无效，Rust 会放行，之后在别的解析器（reqwest/`url` crate）里以另一种错误出现，agent 看到的文本也就不同了。
- 修复建议：
  1. 最低限度：在替换之前，对原始标签（percent-decode 之后）做 `is_forbidden_domain` 检查，并且先把 U+00A0 这一类 `disallowed_STD3_mapped` 映射成对应的 ASCII，再做检查。
  2. 要完全一致，就得实现 UTS #46 的映射表和有效性规则（Node 24.15 用的是 ICU 78 / Unicode 17 的数据）。如果要引入 `idna` crate，需要按 mapping-guide §13 在 decisions.md 里登记，并且用上面的表格和 `url.json` 语料做 golden。
  3. 在补齐之前，模块文档里"approximately"的说法应当登记为 D2 的已知差异，而不是只写在源码注释里。

### S2. 共享 `JsRegex` 的 `lastIndex` 在多线程下竞争，`g`/`y` 正则会随机失败
- 位置：`parse.rs:467-468`（`pattern.set_last_index(0); if pattern.test(s)`），`JsRegex` 的 `last_index: AtomicUsize`（`js/regex.rs:156`）。
- 违反的规则：`schemas_can_live_in_statics` 测试和模块设计都假定 `Schema` 能放进 static，在 tokio 的多个 worker 之间共享。zod 在单线程里先置 0 再 `test`，结果是确定的。
- 复现（`examples/race.rs`）：

  ```rust
  static S: LazyLock<Schema> = LazyLock::new(|| z::string().regex(JsRegex::new("a", "g").unwrap()));
  // 8 个线程各执行 200,000 次 S.safe_parse(&"xxx…xa".into())
  ```

  结果：**1,600,000 次里有 34 次误报失败**（zod 为 0）。原因是线程 A 置 0 之后，线程 B 的 `test` 把 `lastIndex` 推到了 47。
- 现状：契约里的正则都不带 `g`/`y`，问题暂时是潜伏的。但 API 允许这样写，而且没有任何提示。
- 修复建议：
  - 检查路径不要依赖对象上的 `lastIndex`。可以给 `JsRegex` 加一个无状态的 `test_at(s, 0)`，或者 `exec_from(s, start) -> (match, next_index)`，由调用方按 zod 的语义（从 0 开始匹配；对 `y` 是锚定在 0）调用。
  - 另一种做法是在 `.regex()` 构造时拒绝 `g`/`y`，并写进文档。

### S3. `.finite()` 之后的 `.with_message()` 会把消息挂到前一个检查上，或者直接 panic
- 位置：`node.rs:381-384`（`finite()` 返回 `self.clone()`，不加入任何 check），`node.rs:221-230`（`with_message` 修改最后一个 check）。
- 违反的规则：D2 "Each TS schema is translated line for line"。按字面翻译 `z.number().int().finite({ message: "x" })`，得到的是 `.int().finite().with_message("x")`。
- 复现（`examples/mini.rs` 与 `p3.mjs`）：
  - `z.number().int().finite({message:"x"})` 解析 `1.5`：
    - zod：`"message":"Invalid input: expected int, received number"`，因为 classic 的 `inst.finite = () => inst` 会忽略参数。
    - Rust：`"message":"x"`，int 检查的消息被改写了。
  - `z.number().finite({message:"x"})`：zod 能正常构造；Rust 的 `.finite().with_message("x")` 会 **panic**（"needs a preceding check"）。如果它在 `LazyLock` 里，整个 static 就会被毒化。
  - `.optional().with_message(..)`、`.nullable().with_message(..)` 这类放错位置的调用同样会在构造时 panic。
- 现状：契约里的 7 处 `.finite()` 都没有带参数，暂时不会触发。但 `with_message` 是"作用于上一次调用"的隐式协议，而 `finite` 是这个协议里唯一的例外，翻译者没有办法察觉。
- 修复建议：让 `finite()` 压入一个 `CheckKind::Noop` 标记，使后面的 `with_message` 落在它上面、实际不产生效果；或者提供 `finite_with(params)`。同时在模块文档里说明 `with_message` 只能紧跟在检查方法之后。

### S4. 没有 `.shape` 访问器，`agentApiContract.ts:987` 无法逐行翻译
- 位置：`node.rs` 的公开方法里没有读取 shape 字段的方法（完整列表见 `grep "pub fn" node.rs mod.rs`）。
- 违反的规则：D2 "translated line for line into builder calls"。
- 源码：`action: agentApiIntegrationAppManageBodySchema.shape.action`。在 spot 翻译（下一节）里我只能手工再写一遍那 7 个值的 `z::enum_([...])`，这会让两处定义日后悄悄分叉。
- 修复建议：增加 `pub fn shape_field(&self, key: &str) -> Schema`，key 不存在时 panic，与 TS 在编译期就报错的行为对应；或者返回 `Option<Schema>`。

---

## 可选

- **O1. 数字字面量作 record 键时，path 元素的类型不同。**
  - `z.record(z.union([z.literal(-1), z.literal(1.5)]), z.string())` 解析 `{}`：zod 的 path 是 `[-1]`、`[1.5]`（数字），Rust 是 `["-1"]`、`["1.5"]`（字符串）。
  - 原因是 `PathSeg::from_number` 只能表示 usize，`PathSeg` 没有 f64 变体。模糊测试里这一类共 421 条。
  - 契约里的 record 键都是 `z.string()`，不受影响。修复方法是给 `PathSeg` 增加 `Number(f64)` 变体，或者在文档里写明不支持。
- **O2. shape 键名是 `__proto__` 或 `Object.prototype` 上的成员时行为不同。**
  - zod 的 `input[key]`、`key in input` 会沿原型链查找：
    - `z.object({constructor: z.string().optional()})` 解析 `{}`：zod 失败，消息是 `received function`；Rust 成功。
    - `z.object({__proto__: z.string()})` 解析 `{}`：zod 报 `received Object`；Rust 报 `received undefined`。
    - 输入里有自有的 `__proto__` 键时，zod 输出会丢掉这个键（因为赋值走的是 setter）；Rust 保留。
  - 我用 grep 查过，契约里没有这类键名。建议在构造时对这些键 panic，或者在文档里写明。
- **O3. `.refine(fn)` 不写消息的形式没法表达，`.refine(fn, { path })` 也不支持。**
  - zod 不写消息时的默认消息是 `"Invalid input"`。Rust 的 `refine` 必须传消息，传 `""` 会得到 `""`，这一点与 zod 的 `{message:""}` 一致。
  - 契约里的 6 处 refine 都带了消息，都没有 path，暂时不受影响。
- **O4. 构造期报错的时机不同，只在病态 schema 上出现。**
  - 判别值重复（例如两个选项的判别字面量都 `.nullable()`）：Rust 在构造时 panic；zod 要等第一个对象输入到达时才抛错，非对象输入照常返回 `invalid_type`。
  - `z.string().min(5).max(1)`：zod 在构造时抛 `SyntaxError`（内部拼出了 `/^[\s\S]{5,1}$/`）；Rust 能正常构造。

---

## 差分模糊测试与已确认没有问题的部分

- **随机 schema × 输入，约 15.1 万条**（`gen.mjs`，seed 11–18、21–50，每批 400 个 schema × 10 个输入）。
  - 覆盖的组合子：全部支持的类型、字符串和数字检查（其中 20% 带 `{message}`）、数组长度检查、strip/passthrough/strict 三种对象模式、`strictObject`、`extend`、嵌套的 `union`/`discriminatedUnion`（包括可选和可空的判别字段）、七种 record 键类型、`optional`/`nullable`/`nullish`/`default`/`transform`/`meta`、9 种 refine/superRefine（带数字 path、不带 code、调换 message/code 顺序、一次加两条 issue）。
  - 输入值：`undefined`、`NaN`、`±0`、`±Infinity`、`1e21`、`2^53±`、`5e-324`、`1.79e308`、孤立代理项、星号平面字符、`\u0085`/`　`/`﻿` 空白、数组下标键（`0`、`5`、`4294967294`、`4294967295`、`01`、`-1`）、自有的 `__proto__` 键、`length` 键、键重排、多余键。
  - 比较的内容：`success`；`data` 的 JSON 与键序（`-0` 和非有限数带标记）；issues 的 JSON 与键序（非有限数带标记）；`error.message`（`JSON.stringify(issues,null,2)`）逐字节比较。
  - 结果：除了 O1、O2、O4（以及把 `.finite({message})` 按 S3 的方式翻译引入的差异），**没有其他差异**。
- **专项 57 条**（`targeted.mjs`），全部一致：
  - `gt(Infinity)`、`lt(-Infinity)`、`min(NaN)`、`max(1e21)`、`gte(5e-324)`、`lte(-0)`；
  - 字符串 `min(NaN)`/`max(Infinity)`/`length(1.5)`；
  - 自定义消息里含 `"`、`\`、U+2028、C0 控制字符和孤立代理项；
  - enum 或 literal 值里含 `"`、`\ud800`、U+2028、`\0`；
  - literal `NaN`、`-0`、`1e21`；
  - regex 的 `source` 转义（`/`、`\n`、空模式 `(?:)`、`gimsuyd` flags 的顺序）；
  - strict 模式下 `unrecognized_keys` 的顺序（下标键在前，孤立代理项键也包括在内）；
  - `invalid_key` 嵌套 issues 的缩进格式；
  - 嵌套 `invalid_union` 和 `errors: []` 的 pretty 输出；
  - 安全整数边界，以及 `int` 带消息的情况；
  - `optional`/`default` 与 refine 的交互；
  - 对象字段为 `z.undefined()`、`union(…, undefined)`、`unknown`、`transform().optional()`、`optional().transform()`、`nullish().default(null)` 时的 optout 和键保留。
- **`toLowerCase`**：遍历全部码位（非代理项），再加 20 个 final-sigma 等上下文用例，**12,771 条全部一致**（Node 24.15 的 Unicode 17.0 / ICU 78.2）。
- **`z.coerce.number()`**：75 种输入 × 3 个 schema，**225 条全部一致**。输入包括 `0x`/`0b`/`0o` 前缀、全角数字、`1_000`、`᠎`、`[" 5 "]`、`[[5]]`、`[null]`、`{}` 等。
- **翻译接口 spot 检查**（`spot.mjs` 直接 import 真实的 `agentApiContract.ts`，Rust 端手写逐行翻译，见 `examples/spot.rs`）：
  - 覆盖 11 个 schema：`TaskListQuery`（superRefine）、`TaskCreateBody`（嵌套 passthroughObject、`.refine().optional()`）、`TaskAmendBody`（对象 refine）、`TaskHistoryQuery`（`coerce.number().int().positive()`）、`ManagedMcpCallResponse`（数组里的 DU、record）、`IntegrationAppPrepareBody`（DU 加 `...commonShape` 展开）、`ChannelArchiveResponse`（`.extend` 加 transform）、`IntegrationAppManageResponse`（S4）、`MessageReactionBody`（闭包捕获 `JsRegex`）、`TaskAssignBody`、`TaskResourceReceiptBody`（`.datetime()` 加 refine）。
  - **53 条输入全部一致**。
  - 展开 shape 可以用 `[..].into_iter().chain(common())`。重复键时 `v8_order` 取"首次出现的位置、最后一次的值"，与对象字面量的语义一致。
  - `default` 每次解析都会深拷贝；zod 用 `shallowClone`。调用方拿到的是独立的值，不存在修改共享默认值的问题。
- **深层嵌套输入**：`strip`、`passthrough`、`unknown`、`record(string, unknown)` 四种 schema 处理 1e6 层数组，在 512 KiB 线程栈上都正常完成（js-core B1 的修复有效）。
- **性能**：10 万个键的对象走 passthrough、record 或 strict，都在 60–90 ms 之间；10 万个元素的对象数组约 62 ms。
- **mapping-guide**：
  - `schema/` 下没有裸的 `as` 数值转换，都走 `usize_to_f64`、`f64_to_usize_exact`。
  - uuid 和 datetime 的模式文本与 `regexes.js` 逐字一致，走的是 `JsRegex`。
  - 解析路径上的下标访问都有长度保护；`url.rs` 的 punycode 和 ipv4 算术都用了 checked 或 saturating 运算，模糊测试中没有 panic。
  - `Schema` 和 `ZodError` 都是 `Send + Sync`，但 S2 说明这在语义上并不成立。
