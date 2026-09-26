# raft_shared::js 正则（`JsRegex`）：对抗审查 B（第二轮复查）

**结论：有条件通过。** 本轮共 0 个阻塞项、2 个应修项、3 个可选项。

第一轮的 2 个阻塞项（B1 性能、B2 非 u `i` 折叠）和 S1–S4 都已修复，并用第一轮的原探针复测确认。本轮新发现的两个应修项都属于契约和文档层面：
- sticky（非 g）的 `replace` 在共享正则上会 panic，而模块文档和 D27 都声明 `replace*` 可以安全共享；
- mapping-guide §6.2 没有随新 API 和共享规则一起更新，翻译者会照旧把函数内的 `/g` 字面量提升成 static。

两项都只需要改文档或做很小的代码改动，改完即可通过。D27 列出的 6 个遗留项在当前上游范围内都可以接受，但第 4、6 项的描述偏窄，另外还漏列了两处 `v` 模式差异，见 O1。

审查范围：
- `regex.rs`、`regex_parse.rs`、`regex_emit.rs`、`regex_fold.rs`
- `regex_golden_tests.rs`、`regex_semantics_tests.rs`
- `tools/golden/regex-{corpus,semantics}.mjs` 及对应的 golden
- decisions.md D27

测试与对照方式：
- `cargo test -p raft-shared regex` 的 28 个测试全部通过。这次测试是在其他单元把 `schema/parse.rs` 改坏之前跑的。
- 之后 raft-shared 整体编译失败（`schema/parse.rs:563` E0509，`:710` E0004，与本单元无关），所以我在 scratchpad 里做了快照：`reviewB/snap/`，其中 `js/` 目录与仓库逐字节相同，只移除了 `pub mod schema`。所有探针都在这个快照上重新编译运行。
- Oracle 是 `/opt/node22/bin/node`（v22.22.2），机器上没有 Node 24。凡是与 Node 22 的差异可能来自版本变化的，都对照了 Node 24.15.0 抓取的 `regex-semantics.json`，并单独注明。

---

## 复查：第一轮各项的状态

| 第一轮 | 状态 | 证据（均为本轮重新运行的结果） |
|---|---|---|
| B1 每个匹配复制整个输入串，O(n·k) | **已修复** | 详见表下。 |
| B2 非 u `i` 用了 Unicode 折叠 | **已修复** | 详见表下。 |
| S1 非 u 的 `\u{N}` 和残缺的 `\u` | **已修复** | c4、c11 共 27 例，0 差异。`/\u{41}/` 匹配 41 个 `u`，`/\uD83D\u/` 匹配 `"\ud83du"`，与 V8 一致。 |
| S2 `new RegExp(userInput)` 的接受/拒绝判断与 V8 不一致 | **已修复**（`v` 模式除外） | 详见表下。 |
| S3 错误文本 | **已修复** | 详见表下。 |
| S4 golden 漏掉动态正则 | **基本修复** | 详见表下。 |
| O1 static 下 `lastIndex` 竞争 | **改为检测** | 详见表下。 |
| O2 sticky 匹配失败时扫到串尾 | **已修复** | 10K、40K、160K 码元上，2000 次失败的 sticky test 分别耗时 182、209、159 µs，与串长无关（第一轮是 70 ms 和 330 ms）。 |
| O3 API 缺口 | **已修复** | 新增 `search`、`match_global`、`set_last_index_value`。与 Node 对照了 7 个用例（g、gy、y、gu 空匹配、`\B`/gu），以及 lastIndex 取 −1、1.5、NaN、1e300 时的 exec，全部一致。 |
| O4 `source()` 没有转义反斜杠后面的行终止符 | **已修复** | c4 中 `new RegExp("\\\n").source` 和 `a\\ b` 与 Node 一致。 |
| O5 `v` 模式和新特性的零散差异 | **部分修复** | 详见表下。 |

- **B1：**
  - 原探针 `perf2`（`"lorem ipsum "` 重复，执行 `.replace(/\s+/g," ")`）：
    - 200K 码元：**4.4 ms，峰值 RSS 3.4 MB**（第一轮是 37 s、13 GB）；
    - 2M 码元：46 ms，10.7 MB。
  - 原探针 `perf`，160K 码元配 `/a/g`（第一轮直接 OOM）：
    - replace 4.5 ms；
    - match_all 12.7 ms；
    - test 循环 4.1 ms；
    - split 8.4 ms。
  - 这些数字都随长度线性增长。
- **B2：**
  - c6、c7、c8：第一轮的 BMP 大小写变体扫描，2447 例，**0 差异**（第一轮 46 处）。
  - 新增 c15：把 0x0000–0xFFFF 全部码元拼成一个串，用 20 个类做 `gi` 和 `giu` 的 replace，包括 `[a-z]`、`[^a-z]`、`\w`、`\W`、`[\u0080-￿]`、希腊文、西里尔文、拉丁扩展、切罗基文、`[İıſK…]` 等。共 40 例，**0 差异**。
  - 新增语义模糊测试 `sfuzz.mjs`：字母表包含 ſ、K、ı、İ、ß、µ、反向引用、lookaround、`\b` 等。flags 分 4 组（i/gi/im、iu/giu、无、u/y/iy/giy），每组 3 万例，共 12 万例 exec/matchAll/replace/split，**0 差异**。
- **S2：**
  - 第一轮的列表（`\b+`、`\B*`、`\u{41}+`、`\uD83D\u12`/u、`(?<𝒜>x)`）已全部一致。
  - 按接受/拒绝判断做模糊测试，共 48 万例：
    - 非 u（3×4 万）、u（3×4 万）、i/gi/m（3×4 万）：**0 例**判断不一致；
    - v（3×4 万）：6 例是 Node 接受而 Rust 拒绝，见 O1。
  - manifestV1 的路径没有带 `v`，所以该路径已经对齐。
- **S3：**
  - 同一批 48 万例中，非 u 共 2 例文本不同，u 共 12 例，其余全部逐字一致（第一轮非 u 为 2996/40000，u 为 3011/30000）。剩下的差异只有两类，都与版本有关：
    - `[\1`/u、`[\2`/u、`[\00]`/u：Node 22 报 "Invalid class escape"，Rust 报 "Invalid decimal escape"。Node 24 的 golden 中 `[\1]`/u 就是 "Invalid decimal escape"，所以 Rust 与 Node 24 一致。
    - `(?-:` 系列：Node 22 报 "Invalid group"，因为 22 不支持 modifiers。Rust 报 "Invalid flag group"。这条文案没有 Node 24 的 golden 可以对照，见 O3。
  - regress 的原文不再透出。
- **S4：**
  - `regex-semantics.json` 的 dynamic 部分收入了 raftRefs 的全部 21 个动态模式，以及 `_format.ts` 和 osSupervisor 的模式形状。
  - corpus 的输入里加入了 ſ、K 等折叠敏感字符。
  - errors 部分有 147 条。
  - 仍然缺少 `computer/src/output.ts:63` 的动态 lookbehind（我在 c1 中验证过，0 差异），见 O3。
- **O1：**
  - 原探针 `race`（4 个线程在 static `/a/g` 上跑 `while test`）现在 3/4 的线程会 panic，报 "lastIndex changed by another thread…"，不再悄悄算错。
  - 可以安全共享的方法（match_all、replace/g、test_at）在 4 线程×200 轮下没有出现 panic。
  - 但新引入了 S1：sticky replace 也会 panic。
- **O5：**
  - modifiers 已由 Node 24 的 golden 确认（`(?i:a)` 接受，`(?i-i:a)` 报 "Repeated flag in flag group"）。
  - `\p{RGI_Emoji}` 在不加锚点时仍然只匹配第一个码点，见 O1。
  - 重复命名组的语义仍然没有 golden，见 O3。

---

## 应修

### S1. sticky（非 g）的 `replace`/`replace_with` 会读写共享的 `lastIndex`，在共享正则上会 panic，与文档写的"共享安全"相矛盾
- **位置：**
  - `regex.rs:798`：非 g 分支调用 `self.exec_shared(&hay)`，对 `y` 正则会做 CAS 并可能 panic。
  - `regex.rs:22-25` 的模块文档和 D27 Decision 第 4 条都写着 "`replace*` … are safe on a shared regex"。
  - panic 文案写的是 "during exec/test"，但实际发生在 replace 中。
- **复现**（`src/bin/race2.rs`）：`static Y = JsRegex::new("a","y")`，4 个线程各调用 20000 次 `Y.replace(&"aaa…", "b")`。结果有 2 个线程 panic：`JsRegex /a/y: lastIndex changed by another thread during exec/test…`，回溯栈是 `exec_shared ← replace`。
- **说明：** 规范规定 sticky 非全局的 replace 确实要读写 `re.lastIndex`，所以行为本身没错，错的是契约。翻译者看了文档，会把 `/…/y` 放进 static 再调用 replace。上游目前没有 `y` 标志，所以不可达，但这是本单元对外声明的 API 契约。
- **建议：**
  - 模块文档和 D27 都改为："`replace*` 在 `y` 且非 `g` 时同 `exec`，是有状态的方法"。
  - panic 文案改成通用的 "during a lastIndex-updating call"。
  - 加一条单元测试，固定这条契约。

### S2. mapping-guide §6.2 没有随新 API 和共享规则更新，翻译者提升 `/g` 字面量会改变语义
- **位置：** `docs/migration/mapping-guide.md` 的 "2. **Regex**" 段，现在仍然只列出 `test, exec, match_all, replace, replace_all, replace_with, split`。它没有提到：
  - `exec_at`/`test_at`/`search`/`match_global`/`set_last_index_value`；
  - D27 的共享规则；
  - "函数内的正则字面量每次求值都是新对象"这一点。
- **上游触发点：**
  - `daemon/src/drivers/kimi.ts:348-350`：函数内的 `const lineRe = /^\s*\[models\.(.+?)\s*\]\s*$/gm; while ((match = lineRe.exec(raw)) !== null)`；
  - `daemon/src/drivers/kimi-sdk.ts:975-978`：同样的写法。

  这两处都在 daemon（tokio 多线程）里。同时，已经翻译好的 `crates/commander` 把所有正则都提升成了 `static LazyLock<JsRegex>`（例如 `help.rs:41-45`、`option.rs:75-86`、`command.rs:320-322`）。可见"提升成 static"是现有的翻译习惯。
- **复现（单线程即可出现）：** 设 `static RE = JsRegex::new("a","g")`，函数 `f(s) = RE.test(s)`，连续调用两次 `f("a")`。
  - JS 中 `function f(s){ return /a/g.test(s) }` 两次都返回 `true`，因为每次求值都是新对象，lastIndex 为 0。
  - Rust 返回 `true`，然后 `false`，因为 lastIndex 为 1 被带进了第二次调用。
  - 如果在 tokio 的两个任务里并发执行上面的 exec 循环，就会触发 D27 设计的 panic。它只会让单个任务失败（JoinError）；如果发生在 `block_on` 的主任务里，整个进程会退出。仓库里没有 `panic = "abort"` 配置，所以不会直接中止进程。
- **评价：** 用 panic 检测"翻译错误"作为兜底可以接受，因为它比静默错位好。但它只能在两个调用真正交错时才触发（CAS 发生在调用结束时）。上面那种顺序执行时的语义漂移完全检测不到，只能靠翻译规则防住。
- **建议：** 在 §6.2 写明以下规则：
  1. `g`/`y` 正则如果要用 `exec`/`test`，就必须是局部变量（每次调用 `JsRegex::new`，或者从 static `clone()` 一份），或者用 `exec_at`/`test_at` 并把 `let mut li = 0` 放在调用方。
  2. 只用 `replace*`（不带 `y`）、`match_all`、`match_global`、`search`、`split` 的正则可以提升成 static。
  3. 列出新的 API 映射：`.search` 对应 `search`，`.match(/g)` 对应 `match_global`，`re.lastIndex = x` 对应 `set_last_index_value`。

---

## 可选

### O1. D27 遗留项漏列和描述偏窄的几处 `v` 模式差异（上游不用 `v`）
- **漏列：`v` 类中合法的单个标点被拒绝。**
  - `/[.$]/v`、`/[*.]/v`、`/[!$]/v`、`/[!+^]/v`、`/[!+^]\uD83D/v`：Node 接受（`[.$]` 匹配 `"."`），Rust 报 "Regular expression too large"。
  - 规范中的 ClassSetReservedDoublePunctuator 只禁止同一个字符连写两次（例如 `..`），相邻的两个不同标点是合法的。regress 把它们也当成了双标点。
  - 单个出现的 `[.]`、`[$]`、`[*]` 与 Node 一致（c13）。
- **漏列：`\p{RGI_Emoji}` 不加锚点时只匹配第一个码点。**
  - `/\p{RGI_Emoji}/v.exec("x👍🏽")`：Node 匹配 `"👍🏽"`，Rust 只匹配 `"👍"`。V8 对字符串属性优先匹配最长的字符串。
  - golden 里只有带锚点的 `^…$` 用例，这个问题因此测不出来。
- **遗留项 4 描述偏窄：** 不带 `i` 时也会出现。`/\uD83D\u{41}*/v`、`/B\uD83D\u{41}{1}?/v`、`/k\uD83D\u{41}+B./v` 在 Node 中合法，Rust 报 "Regular expression too large"。
- **遗留项 6 已复现：**
  - `/[^[\p{RGI_Emoji}]]/v`：Node 报 "Negated character class may contain strings"，**Rust 接受**。
  - `/[^\p{RGI_Emoji}]/v`：Rust 报 "Regular expression too large"。
- **建议：** 把以上几项写进 D27 的遗留项。同时在 mapping-guide 里写明：出现 `v` 模式时，需要逐条核对这些遗留项。

### O2. 每次调用都新建 `iu`/`giu` 正则，构造成本是 Node 的数百倍
- **复现**（`src/bin/perf4.rs`，2000 次，每次构造一个正则并执行一次 test）：

  | 正则 | Rust | Node 22 |
  |---|---|---|
  | `#([\p{L}\p{N}_-]+):([\da-f]{6,8})`/giu | 287 µs | 0.8 µs（V8 有 RegExp 编译缓存） |
  | `gu` 用户引用正则 | 35 µs | 6 µs |
  | `<TagN>…</TagN>`/gi（每次模式都不同） | 16 µs | 18 µs |

- **影响：** raftRefs 的 `extractRaftRefs` 路径（`raftRefs.ts:443-485`）对每个 chunk 都会调用 5 个 `createRaft*RefRegex()` 工厂函数，每个都是 `giu`。逐字翻译后，每个 chunk 大约多花 1.5 ms。列出上百条消息时，总计会多出百毫秒级。结果本身正确，只是慢。
- **建议：** 在 mapping-guide 中说明：模式固定的工厂函数应翻译成 `static` 加 `clone()`。因为 `match_all` 可以安全共享，直接复用 static 也可以。也可以考虑在 `JsRegex::new` 里按 `(source, flags)` 做一个小型编译缓存（`Arc<Program>` 本身就能共享）。

### O3. golden 仍有几处缺口
- 重复命名组 `(?<a>x)|(?<a>y)` 的接受判断和 `groups.a` 的语义没有 Node 24 的 golden。Rust 接受，并且按照提案取实际参与匹配的分支；Node 22 拒绝，所以无法用 Node 22 对照。
- `(?-:a)` 的 "Invalid flag group"、`(?i-:a)` 等 modifiers 错误文案，只有 `(?i-i:a)` 这一条有 golden。
- `computer/src/output.ts:63` 的动态 lookbehind（``(?<!`)``…）没有进 dynamic 部分。我在 c1 中验证过，0 差异。
- `\p{RGI_Emoji}` 只有带锚点的用例（见 O1）。
- **建议：** 在 `regex-semantics.mjs` 的 errors 和 cases 部分各补几条，然后在 Node 24 上重新抓取。

---

## D27 遗留项逐条评估

| # | 遗留项 | 是否可接受 | 依据 |
|---|---|---|---|
| 1 | 非 u `i` 且带反向引用的模式，每次调用都要把整个输入 canonicalize 一遍，exec 循环因此是二次方 | **可接受** | 实测 `/(a)\1/gi` 在 40K 码元上：`while test` 循环 237 ms，`test_at` 循环 261 ms，`match_all` 3.6 ms。二次方的情况确实存在，而且 `test_at` 也逃不掉，因为每次调用都要重新 canonicalize。不过 corpus 中没有任何"非 u `i` + 反向引用"的模式。建议在 D27 中写明：这类模式要用 `match_all`/`replace` 这样一次性处理的方法。 |
| 2 | 不带 `i` 标志的模式里，`(?i:…)` 内的反向引用仍使用 regress 的折叠 | **可接受** | 需要同时出现 modifiers 和反向引用，上游没有这种写法，而且差异只涉及 ſ、ı、K。 |
| 3 | `u`/`v` 下只模拟了 `\B` 在代理对中间的行为 | **可接受** | 模糊测试 3 万例 u/y 组合，0 差异。`\B` 模拟也没有引入新的性能问题：`\Bx`/gu 在 160K 码元 emoji 串上 replace 7.5 ms，`\B`/gu 的 match_all 11.8 ms，split 10.1 ms，都是线性的。 |
| 4 | regress 拒绝 `\ud83d\u{41}*`/iv | **可接受，但描述偏窄** | 不带 `i` 也会出现，见 O1。上游没有使用 `v`。 |
| 5 | 超过 255 层的嵌套超出 regress 的限制 | **可接受** | manifestV1 限制 pattern 最长 256 个字符，嵌套深度不可能超过 128。实测：256 层 `(?=` 时 V8 接受而 Rust 拒绝；1000 层 `(?:a\|` 时 Rust 拒绝，而 Node 在 2 万层时自己就因 OOM 崩溃了。Rust 在所有情况下都没有崩溃。 |
| 6 | "Negated character class may contain strings" 只对 `\q{}` 检测 | **可接受** | 已复现 `[^[\p{RGI_Emoji}]]`/v 被错误接受。上游没有使用 `v`。 |

---

## 已验证无差异的范围
- **第一轮全部探针重新运行（c1–c12）**：
  - raftRefs 动态模式 1536 例：0 差异；
  - lastIndex 状态机 13232 例：0 差异；
  - GetSubstitution 4000 例：0 差异；
  - BMP 折叠扫描 2447 例：0 差异；
  - Annex B 与 unicode 特性抽样 66 例：只有 `\p{RGI_Emoji}` 一处差异。
  - c2、c3、c12 中其余的差异全部属于 Node 22 与 24 的特性差异（modifiers 和重复命名组）。
- **上游调用点调查（新 API）**：
  - 用到的标志：`g i m s u`，以及 `gi gm gu giu iu mu`。
  - 用到的特性：lookbehind `(?<!`（静态和动态都有）、lookahead、`\p{L}`/`\p{N}`/`\p{Cc}`/`\p{Cf}`、`\b`、命名组、回调替换（8 处，对应 `replace_with`）、`.search`（1 处，对应 `search`）、`.match(/g)`（1 处，对应 `match_global`）、`matchAll`（26 处）、`.split(/…/)`（20 处）、`lastIndex = 0`（`raftRefs.ts:386`，对应 `test_at(&s, &mut 0)`）。
  - 以上各项都有对应的 API，并且已被 golden 或本轮模糊测试覆盖。
- **模糊测试规模**：
  - 接受/拒绝判断与错误文本：48 万例，结论见"复查"部分的 S2 和 S3；
  - 匹配语义：12 万例；
  - 两者都是 0 个非版本类差异，只有 O1 所列的 `v` 模式差异。
- **新的二次方路径排查**：
  - `haystack()` 的 Cow 拷贝只在 `canonical_input` 时发生，即遗留项 1。
  - `find()` 的代理对中间位置扫描是线性的，每个位置只做一次锚定尝试。
  - `build` 只复制捕获到的片段。
  - `get_substitution` 直接从范围切片，不复制整个串。
  - 编译开销见 O2。
- **嵌套与崩溃**：cap、nc、alt、la、cls 五种形状，嵌套深度 100 到 20000，Rust 都没有栈溢出或中止。

探针代码位于 `scratchpad/reviewB/`，包括 `run.mjs`、`node.mjs`、`fuzz.mjs`、`sfuzz.mjs`、`c1–c16.mjs`、`nest.mjs`、`src/bin/{perf,perf2,perf3,perf4,race,race2,api,nest}.rs`，以及快照目录 `snap/`。
