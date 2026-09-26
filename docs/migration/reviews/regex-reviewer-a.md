# raft_shared::js::JsRegex — 对抗评审 A（第二轮）

结论：**不通过**。有 1 个阻塞项：为模拟 V8 的 `\B` 而新增的"对内起点"路径让 regress 在 release 构建下**段错误**（在 debug 构建下 panic），也就是说安全 API 会触发未定义行为。另有 1 个应修项和 2 个可选项。第一轮的 10 项里，8 项已修复，第 5 项部分修复，第 8 项修复时引入了新的阻塞问题。

Oracle 和环境：
- Node 24.15.0（linux-x64），与 golden 捕获版本相同。
- 被测代码是 raft-shared 的**快照**，复制于本轮开始时，放在 scratchpad/reviewA/snap。
- 探针沿用第一轮 scratchpad/reviewA/probe 下的 `cmp.mjs`、`fold.mjs`、`scandiff.mjs`、`bench`，另新增 `perf2`、`race2`、`crash.mjs`、`f1.mjs`、`f2.mjs`、`c11`–`c14`。
- 快照上的 28 个 regex 测试（corpus、semantics 以及手写测试）全部通过。

## 已验证无差异的范围
- **大规模随机测试**：`f1.mjs` 用 3 个种子，每个种子 8000 个随机模式，共 144000 例。
  - token 共 110 种，包括 ſ K ı İ ß ẞ σ ς、代理对与孤立代理项、各类断言与环视、命名组与重复命名组、`\k`、`\1`–`\10`、Annex B 写法（`\c1`、`[\c_]`、`\8`、`\00`、`x{,3}`、`{2,1}`、`]`、`{`）、模式修饰符（`(?i:`、`(?-i:`、`(?s-i:`），以及 v 模式的 `&&`、`--`、嵌套类和 `\q{}`。
  - flags 取 `"" u i iu v iv m s im is` 共 10 种。
  - 每个模式都跑 new、`g` 下连续 exec（lastIndex 取 0–2）、`y` exec、`d` exec、4 种 replace 模板、带 limit 的 split。
  - 用 debug 版 probe 跑，这样 regress 的 debug_assert 会以 panic 的形式暴露出来。
  - 结果：**非 v 模式 0 差异**。全部差异都是 `[^\q{…}]`，见发现 4。
- **大小写折叠**：4632 个有大小写的码点，分别以 `X`、`[X]`、`(X)\1`、`[X-X]` 四种形式，在 `i` 和 `iu` 下测试，0 差异。第一轮的 138 处差异已清零。
- **反向引用和类的折叠**：取 60 个易出错字符，两两组合共 3600 对，测 `^(.)\1$`、`^(?<x>.)\k<x>$`、`^[^a]$`、`(?<=a)x`、`^(?:a)+$`、`^[\wa]$`，均在 `i` 下，共 17496 例，0 差异。
- **Unicode 属性**：沿用第一轮在全码点上的扫描方法，本轮未发现回归。第一轮的 c1–c10 全部重跑（c1、c2、c3、c4、c6、c7、c8、c9、c10），只剩 c1、c6 各有一类 v 模式差异，归入发现 4。
- **性能**：输入均为 100 万码元的 `"a ".repeat(500000)`，正则 `/\s/g`，release 构建。

  | 操作 | 耗时 | 峰值内存 |
  |---|---|---|
  | replace | 40 ms | 6.7 MB |
  | replace_with | 66 ms | — |
  | match_all | 104 ms | 87 MB |
  | `while test` | 39 ms | — |
  | split | 68 ms | — |
  | 逐位置粘连 `/\d/y` | 123 ms | — |
  | `yu` 在 emoji 串上逐位置 `test_at` | 98 ms | — |
  | 构造 1 万次 MIME 正则 | 482 ms | — |

  30 万次匹配的非 u `i` 反向引用 replace 用了 50 ms。
- **嵌套深度**：
  - `(?:` 和 `(` 嵌套到 100 万层都不崩溃，也不会栈溢出。
  - 捕获组超过 10 万时报 `Too many captures`，与 V8 一致。
  - 在 D27 残留 5 的范围内：255 层以内的非可折叠嵌套都能构造成功，256 层起报 `Regular expression too large`。

## 复查：第一轮发现逐项结论

| # | 原严重度 | 状态 | 证据 |
|---|---|---|---|
| 1 每个匹配都复制输入，平方复杂度 | 阻塞 | **已修复** | 见上面的性能表：100 万码元下 replace 为 40 ms / 6.7 MB（原先 8 万码元要 11.7 s / 6.4 GB），match_all 为 104 ms。剩余两条平方路径：一是 D27 残留 1（非 u `i` 带反向引用时的 test 循环，4 万次匹配耗时 2.7 s，V8 为 2 ms）；二是新发现 3 中 `\B`+u 的路径。 |
| 2 非 u `i` 的折叠 | 阻塞 | **已修复** | 扫描 4632 个码点，在 X、[X]、(X)\1、[X-X] 四种形式下 0 差异；c9 中的 3 个 in-scope 调用点（invoke.ts:176、upload.ts:47、agentProcessManager.ts:254）0 差异；corpus 的输入里已加入 `ſession ſecret`、`Kelvin ıd İD straße` 等。只剩 D27 残留 2：`(?i:(s)\1)` 等 3 例在 `sſ` 上仍与 V8 不同，upstream 没有修饰符和反向引用，因此触及不到。 |
| 3 非 u 下的 `\u{…}` | 应修 | **已修复** | c7 共 34 例，0 差异（`/\u{2}/` 匹配 `"uu"`，`/\u{41}/` 不匹配 `"A"`）。 |
| 4 粘连匹配扫描整个剩余输入 | 应修 | **已修复** | 100 万码元下耗时 123 ms（原先 20 万码元要 93 s）；`yu` 为 98 ms。 |
| 5 static 中 lastIndex 被跨线程干扰 | 应修 | **部分修复** | 现在会 panic，但仍有静默的错误结果，见发现 2。 |
| 6 语法接受与拒绝不一致 | 应修 | **已修复** | c2 共 305 例、c6 共 6 万例在非 v 模式下 0 差异：`\b+`、`\B*` 被拒绝，`(?<𝒜>x)` 被接受，嵌套 1000 层可以构造。 |
| 7 报错文本 | 可选 | **已修复** | c2 与 f1 中报错文本 0 差异，包括 v 模式。**更正第一轮**：当时说 `[\1]/u` 在 V8 中报 "Invalid class escape"，那是 Node 22 的文本；Node 24.15.0 报的是 "Invalid decimal escape"，Rust 与它一致。 |
| 8 `\B` 在代理对中间匹配 | 可选 | **改法引入阻塞问题** | 简单情形已对齐（`/\B/u.exec("a😀")` 得 2）。但新路径会让 regress 段错误（发现 1），同时还有未对齐的情形和平方复杂度（发现 3）。 |
| 9 `iu` 下类中的 `\W` / `iv` 下的 `\P{}` | 可选 | **iu 已修复，iv 部分未修复** | c10 在 `iu` 下 0 差异，`\P{Lu}/iv` 已对齐；`[^\W]/iv`、`[\W]/iv` 遇到 ſ 或 K 时仍不同，见发现 4。 |
| 10 `source()` 反斜杠后的行终止符 | 可选 | **已修复** | c2 中 `\`+LF 与 `\`+U+2028 均为 0 差异。 |

## 发现

### 1. [阻塞] 模拟 `\B` 的"对内起点"让 regress 发生越界：release 段错误，debug panic
- 位置：regex.rs 的 `Program::find`。当 `mid_pair_starts` 为真，即 `u`/`v` 模式且模式里含 `\B` 时，它会在每个代理对中间的位置 q 调用 `match_at`，进而调用 `find_from_utf16(hay, q)`，从低代理项开始匹配。regress 的 UTF-16 游标不支持从代理对中间起步，回溯时会越界：classicalbacktrack.rs:567 的 `debug_assert!(max >= min)` 就是在拦这种情况，而 release 构建里没有这条断言，于是直接越界访问。
- 复现（addendum 第 2 项已确认，且范围更广）：

  | 输入 | V8 | debug 构建 | release 构建 |
  |---|---|---|---|
  | `JsRegex::new("\\P{Lu}*\\cA\\B\\p{L}", "u").test("😀")` | false | regress 的 debug_assert panic：`max should be >= min` | **SIGSEGV**，进程崩溃，无法捕获 |
  | `/.*\B./u` 在 `"x😀"` 或 `"a😀b"` 上 | 正常返回 | panic | **SIGSEGV** |

  另外 `/[^a]*\B./u`、`/\W*\B./u`、`/\P{Lu}*\B./u` 在 flags 为 `u`、`v`、`gu`、`iu` 时都会段错误（见 `crash.mjs`）。在 `f2.mjs` 的 12000 个含 `\B` 的随机 u/v 用例中，debug 构建 panic 了 21 次。
- 影响：
  - 这是安全 Rust API 上的内存不安全（UB）。
  - D15 让 Ajv 用 `u` 编译 manifest schema 的 `pattern`；只要某个 schema 里出现 `.*\B.` 这类写法，一段含 emoji 的输入就能让 daemon 或 server 崩溃。
  - 它也违反了 mapping-guide §4.6"debug 与 release 行为一致"的要求：debug 下 panic，release 下 SIGSEGV。
  - 第一轮第 8 项原本只是[可选]，现在的修法把它变成了阻塞。
- 建议：
  - 首选：**删除对内起点的模拟**，把 `/\B/u.exec("a😀")` 与 V8 的差异写进 D27 的残留清单。upstream 的 u/v 模式里没有 `\B`，corpus 里一个都没有。
  - 如果一定要保留模拟，就必须保证 regress 永远不会从代理对中间起步，例如为这些位置单独匹配一个剥离了 `\B` 之外成分的程序。同时要在 release 构建下对上面的复现加回归测试。
  - 另外建议 vendor 的 regress 打开 `overflow-checks`，或者把这条 debug_assert 改成 `assert!`，这样同类问题至少会 panic，而不是越界。

### 2. [应修] 用 panic 检测跨线程竞争只能覆盖单次调用，循环中途被插入时仍会静默出错
- 位置：regex.rs 的 `exec_shared`。它只对单次 exec 的"读取 lastIndex → 写回"做 CAS。
- 输入：`static RE = /a/g`，4 个线程各跑 20 万轮 `while RE.test(&s)`（s 为 `"a"×10` 或 `"ba"`），每轮用 catch_unwind 包住。
  - 结果：4 个线程分别有 11810、791、8675、4858 轮 panic，同时仍有 **4478、270、3532、3033 轮既没有 panic、又得到错误的计数**。原因是 CAS 只保护单次 exec，一个 `while test` 循环在两次 exec 之间被别的线程改了 lastIndex，就检测不到。
- 对设计的判断：
  - D27 的规则本身可以接受：带 `g`/`y` 的正则放进 static 时，改用 `exec_at`/`test_at`。
  - 但文档说"detect a concurrent change and panic"，容易让人以为能兜底，实际上兜不住。
  - 在 mapping-guide 看来，这类 panic 不属于 §4.5"源码会让进程崩溃的断言"，而是对翻译错误的检测。只能当作尽力而为的手段，不能依赖。
- 建议：
  - 用类型从根本上阻止这种用法。例如 `exec`/`test` 在 `g`/`y` 时要求 `&mut self`，或者拆出一个 `!Sync` 的 `JsRegexCursor`。
  - 或者由 static 构造函数在模式带 `g`/`y` 时直接拒绝。
  - 至少要把 D27 和模块文档改成"尽力检测，不保证"，并在 mapping-guide §6.2 写明：模块级的 `/g` 字面量翻译成 `exec_at`，或者在使用处 clone。

### 3. [可选] `\B` 模拟仍与 V8 不一致，且有新的平方复杂度路径（D27 未记录）
- 结果不一致（`f2.mjs`，共 8 例，均不涉及 panic）：

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/\B\W*/gu.exec("x😀😀y")` | index 2，匹配空串 | index 3，匹配 `"😀"` |
  | `"x😀😀y".split(/\B\W*/u)` | `["x\ud83d","\ude00","y"]` | `["x😀","y"]` |
  | `/\B/yu`，lastIndex=2，在 `"a😀b"` 上 | 在 2 处匹配 | null |

- 平方复杂度：`find` 每次都先做一遍完整的最左搜索（找不到时会扫到串尾），再逐个检查代理对中间的位置。
  - `"😀a".repeat(80000).split(/\B/u)`：Rust 37.9 s（2 万次重复时为 2.4 s），V8 53 ms。
- D27 残留 3 的措辞暗示"`\B` 已经对齐"，与上面的结果不符。
- 建议：如果按发现 1 删除了模拟，这一项也随之消失，只需登记为残留。

### 4. [可选] v 模式：`[^\q{…}]` 的结果被取反，`[^*?]` 被误拒，`[\W]` 在 iv 下不同（D27 未记录）
- 位置：new_js 对 v 模式的处理是"用 V8 语法校验通过后，把原文交给 regress"。

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/[^\q{a}]/v.exec("ab")` | `"b"` @1 | `"a"` @0（语义反了） |
  | `/[^\q{a\|b}]/v.exec("abc")`、`/[^[\q{a}]]/v` | 同样 | 同样取反 |

  - f1 在 144000 例中的 80 处差异全部来自这一类，涉及 exec、`g`、`y`、`d`、replace、split。
  - `/[^*?]/v`：V8 接受，Rust 报 `Regular expression too large`。这是 regress 拒绝了一个合法模式，而报出的原因不对。
  - `/[^\W]/iv.exec("ſ")`、`"\u212A"`：V8 匹配，Rust 为 null；`/[\W]/iv.exec("ſ")`：V8 为 null，Rust 匹配。
- 影响：upstream 没有 v 模式，所以是[可选]。但 `[^\q{a}]` 会静默给出取反的结果，必须写进 D27 的残留清单；也可以直接拒绝"v 模式下的否定类里出现 `\q`"。

## 关于 addendum 两项
- **(1) `[\01`、`[\1` 在 u 下报 "Invalid decimal escape"，而 V8 报 "Invalid class escape"**：在 Node 24.15.0 上**不成立**。
  - V8 24 对 `[\01`、`[\1`、`[\01]`、`[\1]` 在 `u` 下报的都是 `Invalid decimal escape`，Rust 与之一致（c11 共 48 例，0 差异）。
  - 无 `u` 时，V8 与 Rust 对未闭合的 `[\01`、`[\1` 都报 `Unterminated character class`。
  - "Invalid class escape" 是 Node 22 的文本。第一轮第 7 项里的这一条也是 Node 22 造成的误差，已在上面更正。
  - 不计为发现。
- **(2) regress 的 debug_assert**：已确认，而且 release 构建下是段错误，已并入发现 1，严重度为[阻塞]。

## D27 残留：是否可以接受
依据：in-scope 的 corpus 共 2306 个模式，其中没有反向引用、没有模式修饰符、没有 `\B`、没有 v 模式（已逐一 grep 确认）。另外还要看输入是否现实。

| 残留 | 判断 |
|---|---|
| 1 非 u `i` 带反向引用时的循环是平方复杂度 | **可以接受**：in-scope 没有反向引用。已实测：4 万次匹配的 test 循环 2.7 s，V8 2 ms。 |
| 2 `(?i:)` / `(?-i:)` 与反向引用组合时的折叠 | **可以接受**：已复现 3 例（`(?i:(s)\1)`、`(s)(?i:\1)`、`(s)(?-i:x)\1`/i 在 `sſ` 上），in-scope 没有这种写法。 |
| 3 u/v 下只模拟了 `\B` | **不可接受**：模拟本身就是发现 1 的来源，而且 `\B` 也没有真正对齐（发现 3）。改为"不模拟"，把这一条整体登记为残留即可接受。 |
| 4、5 | **可以接受**：v 模式和 255 层以上的嵌套在 in-scope 中都不出现。已实测：`(?:a\|` 嵌套 255 层可以构造，256 层以上报 `Regular expression too large`。 |
| 6 | 可以接受，但应扩展到发现 4 中的三类 v 模式差异。 |
| panic-on-race | 规则可以接受，但"检测"兜不住（发现 2），应改用类型约束，或者把措辞改成"尽力而为"。 |

## 统计
阻塞 1，应修 1，可选 2。

分布：阻塞为发现 1（`\B` 模拟导致 regress 越界，release 下段错误）；应修为发现 2（竞争检测不完整）；可选为发现 3（`\B` 仍不对齐且有平方路径）和发现 4（v 模式的 `\q`、`[^*?]`、`[\W]`/iv）。
