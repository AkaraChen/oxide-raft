# raft_shared::js 正则（`JsRegex`）：对抗审查 B

**结论：不通过。** 共 2 个阻塞项、4 个应修项、5 个可选项。

两个阻塞项：
- 每个 `JsMatch` 都复制一份完整的输入串，`replace`、`matchAll` 和 `exec`/`test` 循环因此是 O(n·k)。一段 200K 码元的文本做 `.replace(/\s+/g, " ")`，要 37 s，占 13 GB 内存；160K 时进程被 OOM 杀掉。Node 只要 4 ms。
- 非 `u` 模式下，`i` 标志用的是 Unicode simple case folding，而不是 V8 基于 `toUpperCase` 的 Canonicalize。于是 `/^[a-z0-9_:-]{1,80}$/i` 会接受 `ſ`（U+017F）和 `K`（U+212A），而 Node 会拒绝。这类模式是上游最常见的校验正则写法。

审查范围：
- `crates/raft-shared/src/js/regex.rs`
- `crates/raft-shared/src/js/regex_golden_tests.rs`
- `tools/golden/regex-corpus.mjs`
- `tests/golden/raft-shared/regex-corpus.json`

测试与对照方式：
- `cargo test -p raft-shared regex` 的 17 个测试全部通过。
- 探针是一个 scratch crate（`scratchpad/reviewB/`，release 构建）。同一批用例的 code-unit 数组同时交给 Node 和 Rust，逐项比较结果。
- Oracle 是 `/opt/node22/bin/node`（v22.22.2）。golden 抓取时用的是 v24.15.0，凡是可能与版本有关的结论都单独注明。

---

## 阻塞

### B1. 每个匹配都复制整个输入串：`replace`、`matchAll` 和 exec 循环是 O(n·k)，大文本会 OOM
- **位置：**
  - `regex.rs:636`：`build` 里的 `input: input.clone()`。`JsString` 是 `Vec<u16>`，每次 clone 都是一次完整拷贝。
  - `regex.rs:684-698`：`replace_matches` 先把所有 `JsMatch` 收集进一个 Vec，再统一替换。
  - `regex.rs:654`：`test` 也会构建完整的 `JsMatch`。
  - `regex.rs:666`：`match_all` 一次性算出全部匹配。
- **输入**（`src/bin/perf2.rs`）：`"lorem ipsum ".repeat(n/12)`，执行 `JsRegex::new("\\s+","g").replace(&s, " ")`。

  | n（码元） | Rust 耗时 | Rust 峰值 RSS | Node 22 |
  |---|---|---|---|
  | 50K | 0.46 s | 0.8 GB | 约 1 ms |
  | 100K | 1.87 s | 3.3 GB | 约 2 ms |
  | 200K | 37.2 s | 13.1 GB | 3.7 ms，48 MB |

- 另一组输入是 `"a ".repeat(80_000)`（160K 码元）配 `/a/g`：Rust 的 `replace` 直接被内核 OOM kill。40K 码元时：
  - `replace` 需要 810 ms（Node 1.5 ms）；
  - `match_all` 需要 202 ms（Node 3.2 ms）；
  - `while re.test(&s)` 循环需要 39 ms（Node 0.4 ms）。
- **影响：** 会触发这个问题的上游调用点很多，而且都作用在消息正文、知识内容、日志和诊断文本这类没有长度上限的数据上。例如：
  - `cli/src/commands/freshness/_format.ts:36`：`(content ?? "").replace(/\s+/g, " ")`
  - `cli/src/agentCommsCore/bridge.ts:1075`
  - `cli/src/transportTrace.ts:240`
  - `shared/src/producerFactLineage.ts:47` 和 `:69`：module 级 `/g`，分别调用 `replace` 和 `matchAll`
  - `cli/src/commands/message/_format.ts:423,429,471,472`
  - `computer/src/services/diagnosticsPush.ts:324`

  一条几百 KB 的消息就能让 CLI 或 daemon 卡住几十秒甚至被 OOM 杀死。Node 在这些输入上是线性的。
- **建议：**
  1. `JsMatch.input` 改用共享所有权（`Rc<JsString>` 或 `Arc<[u16]>`），或者直接去掉这个字段，由回调另外拿到 `input`。
  2. `replace_impl` 和 `test` 直接使用 `regress::Match` 的 range，只在 `replace_with` 的回调真正需要时才构建 `JsMatch`，且只切出匹配片段和捕获。
  3. `match_all` 改成惰性迭代器，与 JS 的 `for…of` + `break` 语义一致。
  4. 增加一个 1 MB、10 万个匹配的性能回归测试，设定时间上限。

### B2. 非 `u` + `i`：大小写等价关系用的是 Unicode 折叠，不是 V8 的 Canonicalize，校验正则会放行 Node 拒绝的输入
- **位置：** `regex.rs:86-97`。`engine_flags` 只把 `icase` 交给 regress。regress 0.12 在 `unicode:false` 时仍然用 simple case folding，而规范 Canonicalize（非 u）使用 `toUpperCase`：如果一个非 ASCII 字符大写后变成 ASCII，就保持原样；多字符的大写结果也保持原样。
- **复现（Node 22 与 Rust 对比）：**

  | 表达式 | 输入 | Node | Rust |
  |---|---|---|---|
  | `/^[a-z0-9_:-]{1,80}$/i.test` | `"ſession"` | false | **true** |
  | `/^[a-z0-9][a-z0-9!#$&^_.+-]*\/[a-z0-9][a-z0-9!#$&^_.+-]*$/i.test` | `"text/Kml"` | false | **true** |
  | `/secret/i.test` | `"ſecret"` | false | **true** |
  | `/^\w+$/i.test` | `"ſ"`，以及 `"K"` | false | **true** |
  | `/i/i.test` | `"ı"`（U+0131） | false | **true** |
  | `/[k]/i.test` | `"K"` | false | **true** |
  | `/[^k]/i.test` | `"K"` | **true** | false |
  | `/^ᾈ$/i.test` | `"ᾀ"`（ᾈ/ᾀ，希腊文 titlecase 带 iota） | false | **true** |

  对 BMP 做了系统扫描：对每个码点 c，用 `/^c$/i` 去匹配 c 的单字符 upper/lower 变体，共 2447 个用例，其中 46 个不一致。它们全部属于以下几类：
  - U+0131 与 I/i；
  - U+017F 与 S/s；
  - U+212A 在字符类中的情况；
  - U+1F88–1F8F、1F98–1F9F、1FA8–1FAF、1FBC、1FCC、1FFC 与对应的小写形式。
- **影响：** golden corpus 中有 224 个 `i` 模式和 12 个 `gi` 模式，其中带锚点、用作校验的至少有：
  - `daemon/src/connection.ts:91`：`/^[a-z0-9_:-]{1,80}$/i`
  - `cli/src/commands/attachment/upload.ts:47`：MIME 校验

  另外还有大量 `/token|secret|…/i` 这样的脱敏正则，例如 `cli/src/commands/integration/_session.ts:198,201`。翻译后的代码会放行 Node 拒绝的 id 和 MIME，脱敏范围也会和 Node 不同。现有测试 `case_folding_matches_node` 只覆盖了"模式中写 `K`、输入 `k`"这一个方向，而 corpus 的 20 个输入里没有 ſ、K、ı 以及希腊文 titlecase，所以这个问题测不出来。D3 只写了"`iu` case folding"，没有列出非 u `i` 的差异。
- **建议：**
  - 在 regress 上补一张非 u 的 Canonicalize 表：vendor 或 patch regress，在 `icase && !unicode` 时改用 V8 的 `toUpperCase` 规则。
  - 或者在 `JsRegex` 层对非 u `i` 模式改写：把字面量和字符类展开成 V8 的等价集合，然后以非 icase 编译。
  - 把上面这次 BMP 扫描做成 golden，要求 0 差异。修好之前，这项差异必须按 D3 的要求登记，并且逐一审计所有 `i` 校验点。

---

## 应修

### S1. 非 `u` 模式下，`\u{…}` 和残缺的 `\u` 被 regress 按 unicode 语法解析，匹配语义错误
- **位置：** regress 的解析行为；`regex.rs:196-198` 在非 u 模式下原样交出码元，没有做任何防护。
- **复现：**

  | 表达式 | 输入 | Node | Rust |
  |---|---|---|---|
  | `/\u{41}/.exec` | `"A " + "u".repeat(41)` | index 2，匹配 41 个 `u`（Annex B：`u` 重复 41 次） | index 0，匹配 `"A"` |
  | `/^\u{3}$/.test` | `"uuu"` | true | false |
  | `/^\u{3}$/.test` | `"\u0003"` | false | true |
  | `/[\u{41}]/.exec` | `"A{1}"` | `"{"` | `"A"` |
  | `/\uD83D\u/.exec` | `"\ud83du"` | 匹配 `"\ud83du"` | 只匹配 `"\ud83d"`，末尾的 `\u` 被吞掉 |

- **影响：** `cli/src/commands/integration/manifestV1.ts:401` 对用户提供的 schema `pattern` 执行 `new RegExp(value.pattern)`，没有带 `u` 标志。现在是否合法已经和 Node 不同（见 S2）。上游只用这个模式做校验，但以后如果在 Rust 端拿它去匹配，结果也会不同。
- **建议：** 在 `new_js` 里对非 u 模式做一次预扫描：遇到 `\u` 后面不是 4 位十六进制的情况，改写成 `u` 的字面量转义。或者 patch regress，让它在 `unicode:false` 时不识别 `\u{`，也不把 `\uXXXX\u` 当作代理对。补充 golden。

### S2. `new RegExp(userInput)` 判断合法与否和 V8 不一致（manifestV1 路径）
- **位置：** `regex.rs:452`，直接采用了 regress 的接受/拒绝判断。
- **复现**（均为 `new RegExp(p, f)`）：

  | 模式/标志 | Node | Rust |
  |---|---|---|
  | `\b+`、`\B*`（任意标志） | `SyntaxError …: Nothing to repeat` | 接受 |
  | `\B{1}`/u | Nothing to repeat | 接受 |
  | `\u{41}+`、`1\u{41}*`、`\u{41}{1}x`（非 u） | Nothing to repeat | 接受 |
  | `\uD83D\u12`/u、`0\uD83D\u`/u | Invalid Unicode escape | 接受 |
  | `(?<𝒜>x)`（非 u，组名含代理对） | 接受，`groups.𝒜 === "x"` | `Invalid capture group name` |

  随机模糊测试：字母表 46 个 token，模式长度 1–6。非 u 跑了 40000 例，其中 142 例 Node 报错而 Rust 接受，全部是 `\u{N}` 后接量词的形式。u 模式跑了 30000 例，有 5 例 Node 报错而 Rust 接受。
- **影响：** `manifestV1.ts:401-403` 的实现是：`try { new RegExp(value.pattern) } catch { throw new Error(\`${path}.pattern must be a valid regular expression\`) }`。一个 Node 会拒绝的 manifest（比如 pattern 为 `\b+`）在 Rust 版会通过校验；`(?<𝒜>x)` 则相反，Node 接受而 Rust 拒绝。这属于用户可见的行为差异。
- **建议：** 在 `new_js` 中补上 V8 的这几条早期错误：
  - 量词前面是 `\b`、`\B`、`^` 或 `$` 的断言；
  - 非 u 的 `\u{`；
  - u 模式下前导代理 `\u` 之后跟着残缺的 `\u`。

  非 u 模式的组名按码点读取。把以上模式加入 golden。

### S3. 错误文本：regress 的原文泄漏，启发式分支判断错误
- **位置：** `regex.rs:393-436` 中的 `v8_reason`，其中 `other => other` 会把 regress 的原文直接透传；另外 `atom_char_reason:271`。
- **复现**（Node 22 与 Rust 对比，前缀均为 `Invalid regular expression: /p/f: `）：

  | 输入 | Node | Rust |
  |---|---|---|
  | `(?P<n>x)`、`(?i)abc`、`(?#c)`、`(?>a)`、`(?\|a)`、`(?x)`（各标志） | Invalid group | **Invalid group modifier**（V8 没有这条文案） |
  | `(?` | Invalid group | Invalid capture group name |
  | `(?<a>b)\k<a`、`\k`/u | Invalid capture group name / Invalid named reference | **Invalid named backreference syntax** |
  | `\P`/u、`\p1{`/u | Invalid property name | **Invalid character at property escape start** |
  | `[`/v、`[a`/v | Unterminated character class | **Unbalanced class set bracket** |
  | `[z-a]`/v | Range out of order in character class | **Invalid class set range** |
  | `[\d-z]`/v | Invalid character class | **Invalid class set range** |
  | `[(]`/v | Invalid character in character class | **Invalid class set character** |
  | `a{2}{3}`/u、`\d{2,}+`/u、`a{1}?+`/u、`x{3,}{2}`/u | Nothing to repeat | Lone quantifier brackets |
  | `${x}`/u | Lone quantifier brackets | Incomplete quantifier |
  | `\c`/u、`\c1`/u、`[\c]`/u | Invalid Unicode escape | Invalid escape |
  | `\00`/u、`\01`/u | Invalid decimal escape | Invalid escape |
  | `[\00]`/u、`[\1`/u | Invalid class escape | Invalid escape |
  | `\k<a>)`/u、`\k<a>(`/u、`\k<a>[`/u | Unmatched ')' / Unterminated group / Unterminated character class | Invalid named capture referenced（报错优先级不对：V8 先报语法错误，最后才检查命名引用） |
  | `(?<a>(?<a>`、`(?<a>\u{41}+`（非 u） | Unterminated group / Nothing to repeat | Duplicate capture group name / Unterminated group |

  模糊测试中文本不一致的比例：非 u 为 2996/40000，u 为 3011/30000。
- **版本说明：** Node 24 支持 modifiers，`(?i)abc` 在 Node 24 下的文案可能不是 "Invalid group"。但 "Invalid group modifier"、"Invalid class set range" 这几条在任何 V8 版本里都不存在。corpus golden 里一条错误记录都没有（`error` 字段为 0 个），所以 `v8_reason` 的各个分支都没有经过 oracle 验证，只有 18 条手写断言。
- **影响：** 上游目前唯一一处 `new RegExp(userInput)`（manifestV1）在 catch 里丢弃了原始消息，所以暂时没有用户可见的影响。但 D3 和本模块的文档声称错误文本与 V8 一致，后续单元会依赖这个说法。
- **建议：**
  - `other => other` 改成映射到 V8 的文案表，未知情况按 V8 的归类给出最接近的一条，不要透传 regress 原文。
  - 修正量词后接量词（`}` 之后的 `{`）和 `\c`、`\0d` 的分支。
  - 让 `tools/golden/regex-corpus.mjs` 额外输出一批无效模式（可以直接用本报告中的表格，外加若干随机模糊种子），在 Node 24 上抓取 golden。

### S4. golden corpus 漏掉了最关键的动态正则，输入集也覆盖不到已知的差异
- **位置：** `tools/golden/regex-corpus.mjs:40`。这里只收 `ts.isStringLiteralLike(node.arguments[0])`，模板字符串插值和 `String.raw` 标签模板都被跳过。
- **漏掉的调用点：**
  - `shared/src/raftRefs.ts:14-56,105-126,436`：全部 23 个 `new RegExp(...)`，使用 `gu`、`giu`、`iu` 标志，涉及 `\p{L}`、`\w` 在 `iu` 下的行为、`\b`、负向前瞻；
  - `computer/src/output.ts:63`：动态 lookbehind `(?<!\`)`；
  - `computer/src/osSupervisor.ts:442`：`gi`；
  - `cli/src/commands/message/_format.ts:407`：`escapeRegExp` 与 `i` 组合；
  - `manifestV1.ts:401`。
- 我用探针手工重建了 raftRefs 的 24 个模式，对 16 个输入（含 ſ、K、土耳其文 İ/ı、代理对、孤立代理）各跑 exec、matchAll、replace、split，共 1536 例，**0 差异**。所以这一项是覆盖缺口，不是行为错误。
- 同时，corpus 的 20 个输入里没有 ſ、K、ı、`\u{`、`\b` 量词这类字符或写法，B2 和 S1 因此都没有被测到。
- **建议：** 对模板字符串，先在 Node 里执行求值（或者直接 import 这些工厂函数和常量）再记录。输入集补上 B2 列出的字符。增加一组"无效模式"记录。

---

## 可选

### O1. 在 `static LazyLock` 中共享的 `/g` 正则，多线程下 `lastIndex` 会竞争
- **位置：** `regex.rs:9-11` 的文档建议把 `JsRegex` 放进 static。但 `exec`（`:644-651`）是先 load、再计算、最后 store，不是原子的读-改-写。
- **复现**（`src/bin/race.rs`）：static `JsRegex::new("a","g")`，4 个线程各自对 2000 个 `a` 执行 50 轮 `while RE.test(&s)`。每个线程的期望计数是 100000，实际得到 `[108151, 102603, 102881, 94777]`。JS 是单线程，不会出现这种情况。
- **影响：** 上游 module 级的 `/g` 只有 `producerFactLineage.ts:1-2`，它们只用于 `replace` 和 `matchAll`。这两个方法用的是局部 lastIndex，最多写回 0，所以目前是安全的。风险在于翻译者照着文档，把"module 级 `/g` + `exec` 循环"原样放进 static。
- **建议：** 文档改为：带 `g` 或 `y` 且会调用 `exec`/`test` 的正则不要放进 static，应当按调用点 clone。另外公开一个 `exec_at(&self, s, &mut usize)`，让循环自己持有 lastIndex。

### O2. 带 `y` 的匹配失败时，从 lastIndex 开始扫描到串尾
- **位置：** `regex.rs:583-585`，先做一次 leftmost 搜索，再用 `filter` 检查起点。
- **复现：** 10K 码元的串上，对 `/b/y` 设置 2000 个不同的 lastIndex 执行 `test`（全部失败）：Rust 70 ms，Node 0.1 ms；40K 码元时 Rust 330 ms。用 sticky 写的逐位置扫描器会退化成 O(n²)。上游 corpus 中没有 `y` 标志，所以暂时不会触发。
- **建议：** 用 regress 的锚定匹配，或者在模式外包一层 `^(?:…)` 的变体，做到 O(1) 失败。

### O3. API 缺口
- 没有 `search`。上游有一处：`daemon/src/drivers/kimi-sdk.ts:988` 的 `.search(/^\s*\[/m)`。规范要求 search 保存并恢复 lastIndex。
- 没有"`String.prototype.match` + `g` 返回字符串数组"的接口。上游有一处：`cli/src/commands/message/_format.ts:403` 的 `normalizedQuery.match(/"([^"]+)"|\S+/g)`。
- `set_last_index(usize)` 不接受 JS 的任意数值（负数、NaN），缺少 ToLength 语义。

以上都能绕开实现，但每个翻译点都要重新推一遍规范语义。建议补上 `search`、`match_strings` 和 `set_last_index_f64`。

### O4. `source()` 没有转义反斜杠后面的行终止符
- **复现：**
  - `new RegExp("\\\n").source`：Node 返回 `"\\n"`（反斜杠加字母 n），Rust 返回 `"\\\n"`（反斜杠加真实的 LF）。
  - `new RegExp("a\\ b").source`：Node 返回 `a b` 的转义形式，Rust 保留了原字符。
- **位置：** `regex.rs:480-484`，转义对原样拷贝时没有处理 LF、CR、U+2028、U+2029。上游不读取 `.source`。

### O5. `v` 模式和 Node 24 新特性的零散差异（上游未使用）
- `/\p{RGI_Emoji}/v.exec("👍🏽")`：Node 匹配 `"👍🏽"`，Rust 只匹配 `"👍"`，因为 regress 不支持字符串属性。
- `/[^\W]/iu.exec("K")`：Node 能匹配，Rust 返回 null。
- 与版本有关：重复命名组 `(?<a>x)|(?<a>y)` 和 modifiers `(?i:a)`、`(?-i:a)`，Rust 接受，语义也符合提案（`groups.a` 取实际参与匹配的那个分支）。Node 22 拒绝这些写法；Node 24.15.0 预期接受，但 golden 里没有相应记录。D3 要求"any difference is listed"，建议在 Node 24 上各抓一条 golden 确认。

---

## 已验证无差异的范围
以下各项均为 Node 22 与 Rust 逐项比较的结果。
- **lastIndex 状态机**：13232 例，0 差异。
  - 模式：`\d+`、`\d*`、空模式、`(?:)`、`.`、`a|`、`(?<=a)`、`$`、`^`/m、`\b`；
  - 标志：g、y、gy、gu、yu、gyu、无、u、d、gd；
  - 输入：含代理对、空串；初始 lastIndex 取 0、1、2、3、5、100；
  - 操作：连续 3 次 exec、test、matchAll、replace、split，同时比较结果和执行后的 `lastIndex`。

  确认的行为包括：
  - `matchAll` 从 `re.lastIndex` 开始，且不修改原正则；
  - 带 `g` 的 `replace` 从 0 开始，结束后 lastIndex 为 0；
  - 带 `y`、不带 `g` 的 replace 会读写 lastIndex；
  - `split` 不读也不写 lastIndex；
  - `u` 模式下 lastIndex 落在代理对中间时，从代理对起点开始；
  - 空匹配时按码点推进。
- **split 的 limit**：取 0、1、2、3、−1、2^32+1、1.5、undefined，与捕获组和空匹配组合，0 差异。
- **GetSubstitution**：随机拼接 `$$ $& $\` $' $0 $1 $2 $9 $10 $11 $01 $00 $< $<a> $<b> $<> $<a $1a` 以及孤立代理等 token，4000 例，0 差异。覆盖了 12 个捕获组、命名组未参与匹配、没有命名组时的 `$<`。
- **非全局调用的报错**：`replaceAll` 和 `matchAll` 用于非全局正则时，TypeError 的文案一致。
- **raftRefs 全部动态模式**：见 S4，1536 例，0 差异。`iu` 下 `\w`、`\b` 对 ſ 和 K 的处理与 V8 一致。
- **Annex B 与 unicode 特性抽样**：66 例 exec，除 B2 和 O5 已列出的项外全部一致。覆盖了：
  - 非 u 下的 `\p{L}`、`\k<a>`、`\8`、`\c1`、`[\c_]`、`a{,5}`、`]`、`}`、`\101`、`(a)\10`、`[\1]`、`\x4`、`\u12`、`(?=a)+`；
  - u 模式下的 lookbehind 捕获、`\s` 字符集（含 U+0085 和 U+180E）、`m`/`s` 与行终止符、`\p{Script=…}`、`\p{Lu}`/iu；
  - v 模式的集合运算与 `\q{}`。
- **flags 校验**：`gg`、`x`、`uv`、`G` 的报错一致，`dgimsuy`、`yd` 规范化后的顺序一致。

探针代码位于 `scratchpad/reviewB/`，包括 `run.mjs`、`node.mjs`、`src/main.rs`、`src/bin/{perf,perf2,race}.rs`、`fuzz.mjs` 和 `c1–c12.mjs`。
