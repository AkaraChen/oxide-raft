# raft_shared::js core — 对抗评审 A

结论：**不通过**。有 1 个阻塞项（深嵌套 Value 会让进程因栈溢出 abort），另有 3 个应修项和 2 个可选项。

Oracle：Node 24.15.0（arm64）。探针代码在 /private/tmp/claude-501/revA/（这是一次性的 scratch crate，通过路径依赖引用 raft-shared）。

## 已验证无差异的范围（不在 golden 集中）
- JSON.parse 共 60 个错误输入均与 V8 一致，覆盖：消息文本、position、`\r\n`/`\r\r` 下的行列号、>20 字符源码的片段截断、`"undefined"`/`NaN`/`[object Object]` 特判、`__proto__` 键、重复键、`1e400`、`-0`、超长整数。
- to_number：约 50 个用例，覆盖全部 Unicode 空白、`\u180e`、`\u200b`、`\u0085`、`0x`/`0b`/`0o` 边界、符号加进制前缀、`1_0`、超长数字。
- number_to_string：20000 个随机值，外加 1e21、1e-7 边界、次正规数和 ±0。
- to_fixed：20000 个随机值，外加 1e21 边界、±Infinity、`-1e-10`、100 位精度。
- parse_int：radix 为 2/4/8/10/16/32 时 5000 个随机用例（最长 400 位），另有 radix 推断、ToInt32 回绕、NaN/Infinity/小数 radix。
- 其他一致的部分：math_round、response_text（13 类非法 UTF-8 与 TextDecoder 一致）、pretty 的 indent>10 截断、to_display_string/to_number_value（嵌套数组）、less_than（13 组）、对象键序（`4294967295`、`-0`、`01`、`1.0`）、encode_uri_component（孤立代理项会抛 URIError）、replace/replaceAll/split（`$'`、`` $` ``、`$<`、`$1`、空模式、代理对）、pad、slice。

## 发现

### 1. [阻塞] 深嵌套 Value 在 Drop、clone 和递归遍历时栈溢出，导致进程 abort
- 函数：`Value` 的隐式 Drop 和 Clone，以及 `json_stringify`、`to_display_string`（json.rs:44 `write_json_value`、value.rs:113 `push_display` 都是递归实现）。
- 输入：`"[".repeat(N) + "]".repeat(N)`，在 2 MiB 栈的线程上运行（与 cargo test 默认栈相同）。
  - V8：`JSON.parse` 能成功处理 N=100000。`JSON.stringify` 和 `String()` 会抛出可捕获的 `RangeError: Maximum call stack size exceeded`。
  - Rust：json_parse 本身是迭代实现，可以成功，但**丢弃结果时**就溢出了。debug 构建下 N=10000 即溢出，release 构建下 N=100000 溢出。报错为 `fatal runtime error: stack overflow, aborting`，整个进程被杀死，无法捕获。stringify、display、clone 同样如此。
- 影响：只要有一段不可信的 JSON（例如服务端响应、用户文件）就能让进程崩溃。
- 建议：
  - 给 `Value` 手写迭代式 `Drop`：把子节点 take 到一个显式栈里再逐个释放。
  - 迭代式 Drop 能修复"解析后丢弃"这条路径，但 stringify、display、clone 仍会溢出。因此还要在 json_parse 中加深度上限，超限时返回 `JsError`（V8 的消息是 RangeError），或者把这几条路径都改成迭代实现。

### 2. [应修] parse_int 在 radix 不是 2 的幂且不是 10 时，与 V8 的舍入不一致
- 位置：number.rs:190 `other_radix_value`（以及它的调用方 number.rs:273）。
- 输入：`parseInt("1".repeat(25), 7)`。
  - V8：`223511436610660830000`
  - Rust：`223511436610660800000`
- 另一个例子：`parseInt("02324313404031122424440310302231040101004121441440234", 5)`，V8 为 `1.2067526155139742e+36`，Rust 为 `…744e+36`。
- 规模：radix 取 3/5/6/7/9/11/12/13/20/35/36，长度 1–60 的随机输入，20000 个中 3407 个不一致（约 17%）。
- 根因有两个，已在 JS 中模拟 200000 个随机用例验证，模拟结果与 V8 **0 差异**：
  1. V8 在分块累加之前会跳过前导 `0`。Rust 把前导零也计入 32 位分块，导致分块边界错位。这一点与平台无关。
  2. V8 的 `result = result * multiplier + part` 在 arm64 构建上被编译成了 FMA（只做一次舍入）。
- 建议：
  - 先 `digits` 去掉前导 0。
  - 把累加改为 `value.mul_add(f64::from(multiplier), f64::from(part))`。
  - 注意第 2 点依赖平台：x64 版 Node 可能不融合乘加。需要在 D3 中注明 oracle 以 arm64 为准，或者把这个差异列为可接受。

### 3. ~~[应修] trim、trim_start、trim_end 没有去掉 U+0085~~ **【更正：误报，撤回】** 我当时用 `JSON.stringify` 在终端里打印结果，而终端不显示 U+0085，所以把 `"\u0085a\u0085"` 误看成了 `"a"`。复核 `"\x85a\x85".trim().length === 3`，说明 V8 不会去掉 U+0085。以下原文作废。
- 位置：string.rs:150/157/163，它们共用 `is_js_whitespace`（string.rs:101）。
- 输入：`"\u0085a\u0085".trim()`（trimStart、trimEnd 同理）。
  - V8：`"a"`
  - Rust：`"\u0085a\u0085"`
- 背景：这是 V8 的实现怪癖。V8 的 `Number()`、`parseInt` 和 `/\s/` 都**不**把 U+0085 当空白，Rust 在这三处是一致的，所以只有 trim 系列需要改。
- 建议：给 trim 系列单独用一个谓词，即 `is_js_whitespace(u) || u == 0x85`。to_number、parse_int、collapse_js_whitespace 保持现状。

### 4. [应修] 数字键的 Map 在 bridge 中无法往返
- 位置：bridge.rs:106 `property_key` 会把数字键转成字符串。但 bridge.rs:497 `integer` 只接受 `Value::Number`，而 MapDeserializer（bridge.rs:535）总是以 `Value::String` 的形式交出键。
- 输入：`to_value(&BTreeMap::from([(10u32,1),(2,2)]))` 得到 `{"2":2,"10":1}`，这一步正确。但接着 `from_value::<BTreeMap<u32,i32>>` 报 `Err("invalid type: string, expected u32")`。
- 建议：给键使用一个 `MapKeyDeserializer`。当目标是整数或浮点数时，对键字符串做规范数字解析（`number_to_string(parse) == key`）。

### 5. [可选] 错误消息里的孤立代理项被替换成 U+FFFD
- 位置：json.rs:241/247 的 `from_utf16_lossy`。
- 输入：`JSON.parse("😀")`。
  - V8：`Unexpected token '\ud83d', "😀" is not valid JSON`（消息里含孤立高代理项）。
  - Rust：`Unexpected token '�', …`
- 类似地，当 10 个码元的片段切在代理对中间时，V8 保留孤立半个代理项，Rust 输出 U+FFFD。
- 限制：`JsError.message` 是 `String`，无法无损表示孤立代理项。如果这类消息会展示给用户或在测试中比对，建议记为已知差异，并在 golden 中用 `toWellFormed()` 做归一化。

### 6. [可选] `from_value::<String>` 静默做有损转换
- 输入：`from_value::<String>(&json_parse("\"\\ud800\"")?)`
- 输出：`Ok("\u{FFFD}")`，没有报错。
- 建议：这点可以接受，但应当写进文档。需要保真的字段应该用 `JsString`。

## 统计
阻塞 1，应修 3，可选 2。

## 第二轮复查

结论：**A1–A4 均已修复，阻塞与应修项已清零**。A5、A6 未修复，但仍属可选。本轮新发现 2 个可选项。

复查使用原探针（/private/tmp/claude-501/revA），对照 Node 24.15.0（arm64），并针对这次修复新写了若干探针。

| # | 严重度 | 状态 | 复查结果 |
|---|---|---|---|
| A1 深嵌套导致栈溢出 | 阻塞 | 已修复 | release 构建下 N=100000 和 N=1000000 的 parse、drop、stringify、display、clone 全部正常完成。N=200000 时 clone 后比较 `==` 为 true。 |
| A2 parse_int 非 2 的幂 radix | 应修 | 已修复 | 20000 个随机用例（radix 3–36，长度 1–60）差异为 0。radix 为 2 的幂和 10 的 5000 例仍然一致。 |
| A3 trim 系列未去 U+0085 | 应修 | 已修复 | `trim("\u0085a\u0085")` 得到 `"a"`。`Number` 和 `parseInt` 仍不把 U+0085 当空白，与 V8 一致。 |
| A4 数字键 Map 往返失败 | 应修 | 已修复 | `BTreeMap<u32,_>` 往返结果为 `{2:2,10:1}`。非规范键 `"-0"`、`"01"` 转为整数键时报错，这对类型化反序列化来说是合理的。 |
| A5 错误消息中的孤立代理项 | 可选 | 未修复 | 仍输出 U+FFFD，可按已知差异处理。 |
| A6 `from_value::<String>` 静默有损 | 可选 | 未修复 | 同上。 |

**回归检查（无回归）：**
- JSON.parse：60 个错误和边界用例全部与 V8 一致。
- 混合用例全部与 V8 一致，覆盖：number_to_string、to_fixed、to_number、response_text、pretty 输出、display、less_than、键序、encodeURIComponent，以及 replace、split、pad、slice。
- Object 新索引的键序在以下操作序列中与 V8 一致：先删除 `"2"` 再重新插入，先删除 `"b"` 再重新插入，覆盖已有键 `"a"`，以及 `Object.assign` 到一个已有 `"z"`、`"5"` 的目标对象。键 `"4294967295"` 被正确排在字符串键区。
- `pad_start("a", 1e10, "x")` 报 `Invalid string length`，与 V8 的 RangeError 一致。`pad_start("a", 1e10, "")` 返回原串，也与 V8 一致。
- JsString 的 serde 桥接：带孤立代理项的 JsString 字段（包括嵌在 Vec 和 Option 中的）经 `to_value` → `from_value` 往返后无损，`json_stringify` 输出 `\ud800`。

**新发现：**
- **N1 [可选]** 对 200000 层嵌套数组，V8 的 `JSON.stringify` 和 `String()` 会抛 `RangeError: Maximum call stack size exceeded`，Rust 则成功返回结果。这是"Rust 能成功、V8 会抛错"的方向，对调用方无害，建议在 D3 中注明即可。
- **N2 [可选]** 通过 `serde_json::to_string` 序列化带孤立代理项的 JsString 时会有损（输出 U+FFFD）。它与 `to_value` 桥接的无损行为不一致。如果有代码经由 serde_json 落盘或发送 JsString，应当知晓这一点。

统计：阻塞 0，应修 0，可选 4（A5、A6、N1、N2）。

## 第三轮复查

结论：**阻塞 0、应修 0**。可选项 A5、A6、N1、N2 不变。

**A3 更正：** A3 是误报。V8 的 trim 系列不去 U+0085（`"\x85a\x85".trim().length === 3`）。修复方回退后，Rust 的 `trim` 保留 U+0085，与 V8 一致。第二轮表中 A3 记为"已修复"一项同样作废。

**对象键序：** 对 Object 的新实现（墓碑删除 + 压缩、超过 16 个键时惰性建立索引）做了随机差分测试。
- 共 3000 个随机操作序列，每个序列最多 120 步，混合了 set、delete、`Object.assign`（每次最多 20 个键）。
- 键池包括：数组索引 0–29；`k0`–`k39`；`"4294967295"`、`"4294967294"`、`"-0"`、`"01"`、`"1.0"`、`""`。
- 操作过程中对象会反复跨越 16 个键的阈值，并多次触发压缩。
- 检查内容：`json_stringify` 的最终键序与 V8 逐字比对，并校验 `get_js`、`len` 与遍历结果一致。
- 结果：debug 和 release 构建各跑一遍，差异均为 0。

**json_parse 大对象与重复键：** 300 个随机对象，每个最多 200 个键，含大量重复键和索引键。与 V8 `JSON.stringify(JSON.parse(s))` 比对，差异为 0。

**其他复跑（无回归）：**
- parse_int：20000 个随机用例差异为 0。
- 深嵌套：N=1000000 时解析、释放、stringify 均正常。
- pad：超长长度仍报 RangeError。
- 原有混合用例：全部与 V8 一致。
