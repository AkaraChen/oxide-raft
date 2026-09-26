# raft_shared::js::JsRegex — 对抗评审 A

结论：**不通过**。有 2 个阻塞项：一是每个匹配都整份复制输入，全局 replace/matchAll 的内存和时间随输入呈平方增长，80K 码元的字符串就要 6.4 GB；二是非 `u` 模式下 `i` 的大小写折叠与 V8 不一致，upstream 有 236 个此类正则依赖它，而 golden 测不到。另有 4 个应修项和 4 个可选项。

Oracle：
- 主要使用 Node 24.15.0（linux-x64），即 scratchpad 里已有的 `node-v24.15.0-linux-x64`，与 golden 捕获时的版本一致。
- /opt/node22（v22）只作对照。v22 会拒绝重复命名组 `(?<a>x)|(?<a>y)` 和模式修饰符 `(?i:a)`，v24 接受，regress 也接受。因此这两项以 v24 为准，结论是**一致**。
- 探针位于 scratchpad/reviewA/probe：一个 scratch crate，通过路径依赖引用 raft-shared，有独立的 CARGO_TARGET_DIR；另有 `node.mjs`/`cmp.mjs` 做逐条对比。
- 仓库自带的 17 个 regex 测试全部通过。

## 已验证无差异的范围（不在 golden 集中）
- **Unicode 属性**：`\p{L} N Cc Cf Lu Ll Lt Lm Lo Nd P S Z Cn}`、`Emoji`、`Extended_Pictographic`、`Emoji_Presentation`、`Alphabetic`、`White_Space`、`Script=Han`、`Script_Extensions=Han`、`sc=Latin`、`ID_Start`、`ID_Continue`、`Any`、`Assigned`、`Lowercase`、`Uppercase`，以及 `\s`、`\w`、`[\p{L}\p{N}_-]`，在 `gu` 下对全部 1,112,064 个码点逐一比对，差异为 0。`giu` 下的 `\p{Lu}`、`\p{Ll}`、`\w`、`\W`、`[^\p{Ll}]`、`\P{Ll}` 同样为 0 差异。可见 regress 的 Unicode 版本与 Node 24 的 ICU 一致。
- **`iu` 大小写折叠**：取 4632 个有大小写的码点（到 U+1FFFF），分别以字面量 `X` 和字符类 `[X]` 形式，对全部有大小写字符做扫描，`iu` 下 0 差异。差异全部出现在非 `u` 的 `i` 下，见发现 2。
- **GetSubstitution**：3000 个随机替换模板，由 `$ & \` ' 0 1 2 9 < > a n x` 组合而成，覆盖 0、1、2、11 个捕获组、命名组、重复命名组、`g` 与非 `g`，差异为 0。
- **split**：21 个模式 × 9 个输入 × 4 组 flags（`""`/`u`/`g`/`y`）× 8 个 limit（undefined、0、1、2、3、-1、2^32+1、1.5），共 9048 例，差异为 0。覆盖空匹配、代理对、孤立代理项、捕获组为 undefined 的情形。
- **lastIndex/g/y**：flags 取 `g y gy gu yu gyu`，模式包括 `a`、空、`a|`、`^a`、`\b`、`$`、`(?<=a)b`、`.`、`\udc00`、`\ude00`、`(?:)`；lastIndex 取 0/1/2/3/10；每组连续 exec 4 次，另测 replace、matchAll、split、replaceAll 从 lastIndex=1 开始的情形，共 676 例，只有 2 例不一致，均属发现 2。确认一致的行为：
  - `u` 下 lastIndex 落在代理对中间时回退到对首；
  - 粘连匹配失败后归零；
  - 全局 replace 先置 0、结束后仍为 0；
  - matchAll 不修改原正则；
  - split 忽略 lastIndex。
- **其他语义**：
  - `m`/`s` 与 `\r`、` `、` `、`\r\n`、`\u0085` 的组合；
  - 非 `u`/`iu` 下的反向引用；
  - lookbehind 中捕获与反向引用的顺序；
  - `d` 标志的 indices：含代理对、未参与的组、重复命名组，以及 groups 的声明顺序；
  - 粘连 exec 与 lookbehind、`\b`、`^` 同时出现，且 lastIndex>0 的情形；
  - 空匹配出现在末尾时的 `` $` ``、`$'`。
- **报错文本（非 v 模式）**：6000 个随机模式，由 57 种 token 拼接，覆盖 8 种 flags 组合，每个模式配 9 个输入，共 60000 例。非 `v` 的构造差异只有发现 6、7 列出的几类。
- **flags 报错**：`gg`、`x`、`uv`、`G`、`l` 的报错与 `flags()` 的规范顺序均一致。

## 发现

### 1. [阻塞] 每个 `JsMatch` 都整份复制输入；全局 replace/matchAll 的内存和时间都是平方级
- 位置：regex.rs:636 `build` 中的 `input: input.clone()`。`JsString` 是自有的 `Vec<u16>`，每个匹配都会复制一遍完整输入。`replace_matches`（regex.rs:684）和 `match_all`（regex.rs:666）会先把所有 `JsMatch` 收集起来再使用。
- 输入：`s = "a ".repeat(n)`，正则 `/\s/g`（release 构建，内存为 VmHWM 峰值）。

  | 操作 | n=20000（40K 码元） | n=40000（80K 码元） |
  |---|---|---|
  | `replace(s, "_")` | 0.79 s / 1.57 GB | **11.7 s / 6.4 GB** |
  | `match_all` | 0.78 s / 1.57 GB | 4.9 s / 6.4 GB |
  | `replace_with` | —（n=30000：1.7 s / 3.5 GB） | — |

  - `while re.test(&s)`：内存正常，但时间也是平方级，n=200000（400K 码元）耗时 4.46 s。
  - Node 24 对 400K 码元同时执行 test 循环、replace 和 matchAll，合计只需 81 ms / 116 MB。
- 影响：日志、消息体、文件内容只要几百 KB，做一次全局 replace 或 matchAll 就会让进程 OOM 或卡住数十秒。upstream 有大量这类用法，例如 raftRefs.ts 的 `matchAll`、`mergeOverlappingSpans`、`producerFactLineage`，以及 101 个 `g` 正则。
- 建议：
  - `JsMatch` 不要持有输入的副本。可以改为共享引用（`Arc<[u16]>`，或者借用 `&'a JsString`），捕获组存为区间，按需切片。
  - `replace_impl` 边匹配边写出，不要先收集。
  - 补一个大输入的性能回归测试，例如 1 MB 输入、50 万个匹配。

### 2. [阻塞] 非 `u` 模式下 `i` 的大小写折叠与 V8 不同，D3 未记录，golden 也测不到
- 位置：regex.rs:86 `engine_flags`。它把 `icase` 直接交给 regress。regress 在非 `unicode` 模式下仍然使用 Unicode 简单大小写折叠，而 V8 用的是规范中的 Canonicalize：取 `toUpperCase`，结果不是单个码元时保留原字符，非 ASCII 字符不会映射到 ASCII。
- 规模：4632 个有大小写的码点 × 两种形式（`X`、`[X]`），在 `i` 模式下有 **138 处不一致**，在 `iu` 模式下为 0。部分例子：

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/s/i.exec("ſ")` | null | 匹配 |
  | `/I/i.exec("ı")`（土耳其语无点 i） | null | 匹配 |
  | `/[a-z]/i.exec("K")`（开尔文符号） | null | 匹配 |
  | `/\w/i.exec("ſ")` | null | 匹配 |
  | `/\W/i.exec("ſ")` | 匹配 | null |
  | `/[^k]/i.exec("K")` | 匹配 | null |
  | `/[ß]/i.exec("ẞ")` | null | 匹配 |
  | `/[Ω]/i.exec("Ω")` | null | 匹配 |
  | `/(s)\1/i.exec("sſ")` | null | 匹配 |

  另外还有 `ᾀ`–`ᾯ`、`ᾳ`/`ᾼ`、`ῃ`/`ῌ`、`ῳ`/`ῼ` 这些希腊语 prosgegrammeni 对，以及 U+212B、U+03F4、U+FB05/FB06、U+0390/U+1FD3、U+03B0/U+1FE3。
- 对 in-scope 调用点的实际影响（均已复现）：
  - `cli/src/commands/integration/invoke.ts:176` 的 `isCredentialField`：`"Key-ſ".replace(/[^a-z0-9]/gi, "_")`，V8 得 `"_ey__"`，Rust 得 `"Key_ſ"`。凭据字段识别结果不同。
  - `cli/src/commands/attachment/upload.ts:47` 的 `MIME_TYPE_RE`：`"text/ſvg"` 在 V8 中被拒，在 Rust 中被接受。
  - `daemon/src/agentProcessManager.ts:254`：`/(?:^|[._-])token(?:s)?(?:[._-]|$)/i` 匹配 `"git_tokenſ"`，V8 得 null，Rust 在 index 3 处匹配。
- 为什么 golden 发现不了：
  - corpus 中有 236 个带 `i` 但不带 `u` 的正则，但 20 个固定输入里没有 ſ、K、ı 或任何希腊字符。
  - 手写测试只断言了 `/K/i` 不匹配 `k`。这一条恰好是 regress 字面量路径上仍然正确的少数情形。
  - D3 规定"任何差异在被翻译模式依赖前列入本记录"，但本差异没有列入。
- 建议：
  - 首选：修正 regress 非 unicode 模式的 canonicalize（vendor 或提交 patch），改用 V8 的 `toUpperCase` 规则。
  - 如果短期修不了，就在 D3 中登记这一差异，并列出受影响的调用点。
  - 无论哪种方案，都给 golden 输入加入 `ſ ı İ K Å Ω ẞ ß ς` 以及 `ᾀ ᾈ`，再补一个 `i`/`iu` 的逐码点折叠 golden（本评审的扫描方法可以直接复用）。

### 3. [应修] 非 `u` 模式下，`\u{…}` 被当成码点转义
- 位置：regress 的解析器（经 regex.rs:452 调用）。按 Annex B，非 `u` 模式下的 `\u{41}` 是 `u` 重复 41 次，而 regress 把它当作 U+0041。

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/\u{41}/.exec("A")` | null | 匹配 |
  | `/\u{2}/.exec("uu")` | 匹配 `"uu"` | null |
  | `/\u{1F600}/.exec("u{1F600}")` | 匹配 | null |
  | `/[\u{41}]/.exec("u")` | 匹配 | null |

- 影响：目前的字面量 corpus 里没有这种写法。但它会悄无声息地改变模式含义，commander 的 `wrap` 等动态拼接的非 `u` 模式也可能触及。
- 建议：在 `new_js` 中，对非 `u`/`v` 的模式预扫描 `\u{`，按 Annex B 语义改写（把 `\u` 换成 `u`），或者直接拒绝。另外补一个 golden。

### 4. [应修] 粘连（`y`）exec 失败时会扫描整个剩余输入
- 位置：regex.rs:583–585。粘连匹配是用"从 start 开始的最左搜索，再过滤 `m.start() == start`"实现的。一旦在 start 处没有匹配，regress 会一直搜到串尾。
- 输入：`s = "a ".repeat(n)`，`y = /\d/y`，对每个位置 i 执行 `y.lastIndex = i; y.test(s)`。
  - V8（n=100000，200K 码元）：4 ms。
  - Rust：n=50000 时 22.5 s，n=100000 时 **93 s**。
- 影响：D3 声明支持 `y`，而用 `y` 写的词法分析器会退化成平方复杂度。upstream 目前没有 `y` 字面量。
- 建议：粘连模式下把 pattern 包成锚定形式（例如在前面加上只在 start 处成立的断言），或者改用 regress 的锚定匹配 API，不要做"搜索后过滤"。

### 5. [应修] 模块文档推荐 `static LazyLock<JsRegex>`，但带 `g`/`y` 的正则跨线程共享时 lastIndex 会互相干扰
- 位置：regex.rs:9–11 的文档，以及 `exec`（regex.rs:644），后者是先 load、再 exec、再 store。
- 输入：`static RE = /a/g`，4 个线程各跑 20000 轮 `while RE.test(&s)`（s 为 `"a"×10`，或者 `"b"×36+"a"`）。每轮预期计数为 10 或 1，实际错误轮数分别为 3696、435、2097、1178。
- 背景：V8 是单线程，同步的 `while (re.test(s))` 循环不会被打断。翻译成 Rust 后，daemon 和 server 跑在多线程运行时上，结果会错误且不确定。
- 建议：
  - 文档改为只允许无 `g`/`y` 的正则放进 static。
  - 或者让有状态的正则 `!Sync`，例如把 lastIndex 放进 `Cell`，由编译器阻止跨线程共享。
  - 或者在 guide §6.2 写明：模块级 `/g` 正则翻译为"每次调用 clone"或局部构造。

### 6. [应修] 语法的接受与拒绝和 V8 不一致（动态模式、Ajv 的 `pattern` 会遇到）
- 位置：`new_js`（regex.rs:445）直接采用 regress 的解析结果。

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/\b+/`、`/\B*/`、`/\b{2}/`、`/\B?$/i` 等量化断言（`u`/`v` 同样） | `SyntaxError: … Nothing to repeat` | **构造成功** |
  | `/(?<𝒜>x)/`（非 u，星界字符作组名） | 接受 | `Invalid capture group name` |
  | 256 层嵌套的 `(`…`)` 或 `(?:`…`)` | 可以构造（1000 层也行，捕获组 20000 层时才报 `Stack overflow`） | 从约 256 层起报 `Regular expression is too deeply nested`（200 层正常） |

- 影响：D15 规定 Ajv 用 `JsRegex` 加 `u` 编译 schema 的 `pattern`。V8 下会编译失败的 schema，在 Rust 下能通过（或者反过来），meta 验证、编译报错的路径就和 TS 不同了。
- 建议：
  - 在 `new_js` 中预检"量化的 `\b`/`\B`"，给出 V8 的文本。
  - 把星界组名、嵌套深度登记为已知差异，或者修 regress。
  - golden 中加入这些用例。

### 7. [可选] 部分报错文本不是 V8 的原文
以下情形 V8 与 Rust 都会拒绝，只是文本不同（Node 22 与 24 的文本相同）：

| 模式 | V8 | Rust |
|---|---|---|
| `/\k/u` | `Invalid named reference` | `Invalid named backreference syntax`（regress 原文直接透出） |
| `/\c/u`、`/\c1/u`、`/[\c_]/u` | `Invalid Unicode escape` | `Invalid escape` |
| `/\01/u` | `Invalid decimal escape` | `Invalid escape` |
| `/[\1]/u` | `Invalid class escape` | `Invalid escape` |
| `/(?/`、`/(?:a)(?)/` | `Invalid group` | `Invalid capture group name` / `Invalid group modifier` |
| `/(?<n>\n(?<n>/` | `Unterminated group` | `Duplicate capture group name`（报错优先级不同） |
| `v` 模式的字符类错误，如 `/[(]/v`、`/[z-a]/v`、`/[\d-z]/v` | `Invalid character in character class`、`Range out of order in character class`、`Invalid character class` | `Invalid class set character`、`Invalid class set range` |

建议：在 `v8_reason` 中补上这些映射。v 模式目前没有调用方，可以只登记。

### 8. [可选] `u`/`v` 下 V8 的 `\B` 会在代理对中间匹配，Rust 不会
- 输入：`/\B/u.exec("a😀")`。
  - V8：index 2，位于代理对中间。`"a😀".replace(/\B/u, "|")` 得到 `a \ud83d | \ude00`。
  - Rust：index 3，replace 得到 `a😀|`。`/\B/gu`、`/\B/vg` 同样如此。
- 这是 V8 自己的怪癖，不符合规范的 AdvanceStringIndex，但 oracle 是 V8。
- 建议：在 D3 中登记为已知差异。upstream 的 `u` 模式里没有 `\B`。

### 9. [可选] `iu` 下字符类中的 `\W`，以及 `iv` 下的 `\P{…}`，与 V8 不同
- `iu` 下 `\W` 放在字符类里时：

  | 输入 | V8 | Rust |
  |---|---|---|
  | `/[\W]/iu.exec("ſ")` | null | 匹配 |
  | `/[^\W]/iu.exec("K")` | 匹配 | null |
  | `/[^\W\d]/iu.exec("ſ")` | 匹配 | null |

  单独使用 `\W`、`\w`，或在字符类中使用 `[\w]`、`[^\w/]`，都与 V8 一致。raftRefs 的 `giu` 模式经复核不受影响。
- `/\P{Lu}/iv.exec("A")`：V8 为 null，Rust 匹配。
- 建议：登记为已知差异。

### 10. [可选] `source()` 没有转义反斜杠之后的行终止符
- 位置：regex.rs:480。遇到 `\` 时，会把下一个码元原样拷贝，即使它是行终止符。

  | 输入 | V8 | Rust |
  |---|---|---|
  | `new RegExp("\\\n").source` | `"\\n"` | `"\\" + LF` |
  | `new RegExp("\\ ").source` | `"\\u2028"` | `"\\" + U+2028` |

- 建议：反斜杠后面如果是行终止符，同样走 match 分支做转义。

## 关于证明（golden 与测试）的问题，归入发现 2、6
- corpus 只收录了字面量，以及参数为字符串字面量的 `new RegExp`。raftRefs.ts 中通过模板串动态构造的 12 个 `gu`/`giu`/`iu` 模式（D3 的主要使用方），以及 `autocompleteTriggers` 等，都不在 golden 中。
- corpus 中命名组、`y`、`d` 都是 0 个。replace 只用了固定模板 `<$&|$1|$$>`；split 没有测 limit；也没有 lastIndex 序列。
- D3 声称"golden 检查 `iu` 大小写折叠"，但 golden 的输入里没有任何对折叠敏感的字符。

## 统计
阻塞 2，应修 4，可选 4。

分布：阻塞为发现 1、2；应修为发现 3、4、5、6；可选为发现 7、8、9、10。
