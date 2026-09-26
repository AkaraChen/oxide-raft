# raft_shared::js core：对抗审查 B（设计、契约、健壮性）

**结论：不通过。** 共 3 个阻塞项、9 个应修项、6 个可选项。阻塞项是：深层嵌套的值会让进程栈溢出后直接中止；`error_to_string` 的签名和 §4.2 不一致，而且没有提供 `js_name` trait；对象键序的 golden 比较有一半是同义反复（期望值也经过被测代码构造）。

审查范围：`crates/raft-shared/src/lib.rs`、`src/js/*.rs`。`cargo test -p raft-shared` 31 个测试全部通过。探针代码放在 `/private/tmp/claude-501/probe`（release 构建，对照 Node v24.15.0）。

---

## 阻塞

### B1. 深层嵌套的 `Value` 在 drop、clone、stringify 和 String(x) 时栈溢出，进程中止
- 位置：`value.rs:7-18`（`Value`/`Object` 用编译器派生的递归 `Drop`/`Clone`/`PartialEq`/`Debug`）、`json.rs:45`（`write_json_value` 递归）、`value.rs:111`（`push_display` 递归）、`bridge.rs` 整个 serde 桥（递归）。
- 违反的规则：D3 写明 "`json_parse` … f64 numbers, lone surrogates allowed, **no depth limit**"；mapping-guide §6.4 也写了同样的要求。
- 探针结果：
  - `json_parse("[" * 1e6 + "]" * 1e6)` 能解析成功（解析器是迭代实现，这一点做对了），但随后一次普通的 `drop(v)` 就触发 `fatal runtime error: stack overflow, aborting`（rc=134）。
  - 嵌套 1e5 层时，`json_stringify`、`to_display_string` 和 `clone` 同样中止进程。
  - Node 下 `JSON.parse` 能正常处理 1e6 层；`JSON.stringify` 和 `String(v)` 抛出的是**可以捕获的** `RangeError: Maximum call stack size exceeded`。
  - tokio 工作线程的默认栈只有 2 MiB，阈值会低得多。任何一个 stdin、profile 文件或服务端响应都能让 CLI 或 daemon 直接中止，不会走到错误处理路径。
- 修复建议：
  1. 给 `Value` 手写迭代式 `Drop`（用显式栈接管子节点）。
  2. `Clone`/`PartialEq`/`Debug`、`write_json_value`、`push_display` 和桥接代码二选一：改成显式栈迭代，或者设一个深度计数器，超限时返回 `Err(JsError::range("Maximum call stack size exceeded"))`。`json_stringify` 的返回值要相应改成 `Result<Option<String>, JsError>`。
  3. 增加 1e6 层嵌套的回归测试。

### B2. `error_to_string` 的签名不符合 §4.2，也没有 `js_name` 的公共 trait
- 位置：`error.rs:59` 是 `pub fn error_to_string(name: &str, message: &str) -> String`。
- 违反的规则：mapping-guide §4.2 要求 "`String(err)` and `${err}` → `js::error_to_string(&err)`"，并且 "Every error type exposes `js_name()`"；D3 的写法也是 "`error_to_string(&err)` = `"{js_name}: {message}"`"。
- 后果：
  - 后续单元里有 103 处调用点，只能各自写 `error_to_string(e.js_name(), &e.to_string())`。
  - 目前 `js_name` 只是 `JsError` 上的固有方法。`CliError` 这类别的错误类型没有可以实现的 trait，`anyhow::Error` 也没有统一的取名方式。
  - 这正是 pilot O2 描述的情况：D3 规定的名字被一个形状不对的 helper 占用了。
- 修复建议：
  - 在 `js` 中定义 `pub trait JsNamedError: std::error::Error { fn js_name(&self) -> &str; }`，由 `JsError` 实现。
  - 提供 `pub fn error_to_string(err: &(impl JsNamedError + ?Sized)) -> String`。可以另外提供一个 `&anyhow::Error` 的变体，按 downcast 取名，取不到就用 `"Error"`。
  - 现有的 `(name, message)` 版本改为私有，或者改名为 `error_to_string_parts`。

### B3. 对象键序 golden 的期望值由被测代码构造，键序规则的错误测不出来
- 位置：`golden_tests.rs:62-69`。`entries()` 用 `Object::insert` 构造**期望**对象，而 `insert` 本身就会按 `array_index` 重排键。`json_parse_golden`（:405）、`object_from_entries_golden`（:484）和 `object_assign_golden`（:460）比较的都是 `same_value(got, val(expected))`。
- 失败场景：假设 `array_index` 错误地把 `"4294967295"` 或 `"01"` 当成数组下标，期望对象和实际对象会被同一个 bug 以同样的方式重排，`same_value` 依然判为相等。结果是 D3 的核心事实（例子 `1, 2, 4294967294, b, a, 01, 4294967295`）没有真正对照 V8 验证。只有"排序是否被执行"这一点能测出来，"排序规则本身对不对"测不出来。
- 修复建议：比较期望值时不经过 `Object`。直接把 `got.keys()` 与 golden 里原始的 `[key, value]` 序列逐项比较，把 golden 的顺序当作真值。也可以给 `entries()` 加一个只按原顺序 push 的测试专用构造函数，比如 `Object::from_raw_ordered`（仅 `#[cfg(test)]`）。

---

## 应修

### S1. `Object` 查找和插入都是 O(n)，大对象会退化成 O(n²)
- 位置：`object.rs:43-48, 59-74`。每次 `insert` 都要做一次线性 `position` 查找；`get(&str)` 对每个条目都要重新 `encode_utf16` 再比较。
- 探针结果：`json_parse` 一个有 2 万个键的对象要 0.86 s，8 万个键要 **29.2 s**（Node 只需毫秒级）。服务端返回的大 map（按 id 索引的表、inbox 快照）完全可能达到这个量级。`from_pairs` 和 `object_assign` 也是同样的复杂度。
- 修复建议：在 `entries: Vec` 旁边维护一个 `HashMap<JsString, usize>` 索引（serde 规则 §13 之外不能引入 `indexmap` 的话就手写）。`remove` 时重建索引或者使用墓碑。

### S2. `pad_start`/`pad_end` 遇到过大的 maxLength 会中止进程，V8 抛的是 RangeError
- 位置：`string.rs:185`（`Vec::with_capacity(fill_len)`）。
- 探针结果：`pad_start("a", 1e15, " ")` 触发 `memory allocation of 1999999999999998 bytes failed`（rc=134）。V8 的行为是 `RangeError: Invalid string length`，最大字符串长度是 2^29−24。
- 修复建议：返回 `Result<JsString, JsError>`。长度超过 `536_870_888` 时返回 `JsError::range("Invalid string length")`。

### S3. `JsString` 的 `Serialize` 实现在非 `js` 序列化器下会悄悄改变数据形状
- 位置：`bridge.rs:34-47`。
- 探针结果：`serde_json::to_string(&JsString::from_units(vec![0x61, 0xD800]))` 输出 `[97,55296]`，字符串变成了数字数组。只要有一个带 `JsString` 字段的 derive 结构被 `serde_json`、日志或者任何第三方 serializer 序列化，就会得到一个类型错误而且无法察觉的输出。
- 修复建议：遇到非 `JS_STRING_TOKEN` 的 serializer 时，按 Node 写流的语义输出 `serialize_str(&lossy)`（U+FFFD）。保留 code units 的路径只在 `ValueSerializer` 内部走：在 `serialize_newtype_struct` 里识别 token，直接取 units，不要依赖 `collect_seq` 的回退。

### S4. 缺少 `js` 的数值转换出口，§4.6 的要求在后续单元里无法遵守
- 违反的规则：§4.6 规定 "No bare `as` casts between floats and integers; use the `js` conversions."
- 现状：
  - 本模块内部的 `as` 转换都带了 review 注释，分别在 `string.rs:130,133,179,239,249`、`number.rs:232`、`bridge.rs:94,102,234,466,469`、`bigint.rs:11,30,34,46,50,55,76,94,127,139,149,153`。
  - 但是对外没有导出任何转换函数。翻译 `arr[i]`、`new Array(n)`、`--lines` 转 `usize` 这类代码时，要么写裸 `as`，要么各自手写一份。这也违反 §6 "Do not re-implement them inline"。
- 修复建议：导出以下函数：
  - `js::to_usize_index(f64) -> Option<usize>`
  - `js::f64_to_i64_exact(f64) -> Option<i64>`
  - `js::usize_to_f64(usize) -> f64`（`.length` 转成 `Value::Number` 时需要）
  - `to_uint32`/`to_int32`（目前是私有的，`string.rs:233` 和 `number.rs:226` 各有一份，重复实现，应合并）

### S5. 缺少拼接接口，模板字面量写不出来
- 位置：`string.rs:43,47`。`push_str`/`push_units` 是 `pub(crate)`，而且没有 `Add`/`Extend`/`FromIterator`/`concat`。
- 后果：在 crate 外，`${a}:${b}` 这种写法只能用 `JsString::from_units([a.as_units(), …].concat())` 拼出来。`String(x)` 的结果 `to_display_string` 返回 `JsString`，也没法直接与 `&str` 拼接。
- 修复建议：
  - 把 `push_str`/`push_units`/`push_js` 改为 `pub`。
  - 实现 `Add<&JsString>`、`Extend<u16>` 和 `FromIterator<JsString>`。
  - 提供一个 `js_format!`，或者 `JsString::concat(&[&JsString])`。

### S6. `Value` 缺少字面量构造，翻译对象展开和 JSON 输出时很啰嗦
- 位置：`value.rs:34-93`。
- 现状：没有 `From<f64|bool|&str|String|JsString|Vec<Value>|Object> for Value`，也没有 `From<Option<T>>`（None 转 Undefined）；`Object` 没有 `IntoIterator`（拥有所有权的版本）、`retain`、`iter_mut`。
- 后果：`{ ...(x ? {k: v} : {}) }` 这种写法只能写成多行的 `insert` 加 `Value::Number(...)`。
- 修复建议：补上这些 `From` 实现，以及 `Object::iter_mut` 和 `impl IntoIterator for Object`。

### S7. `parse_int` 的签名与 D3 不一致
- 位置：`number.rs:237` 是 `parse_int(s: &JsString, radix: Option<f64>)`；D3 写的是 `parse_int(s, radix: Option<u32>) -> f64`。
- 评价：实现本身更贴近 JS 语义（因为 `ToInt32(radix)`），但代码和决策记录必须一致。
- 修复建议：更新 D3 的文字，或者改代码，二选一。

### S8. clippy 有 3 条警告
- `bridge.rs:709`：`collapsible_if`
- `number.rs:40`：`manual_range_contains`（`x < 0.0 && x >= -0.5`）
- `golden_tests.rs:245`：`type_complexity`（`[(&str, fn(&JsString) -> JsString); 3]`）

### S9. `cargo fmt --check` 不通过，9 个文件共 84 处 diff
- 各文件 hunk 数：`golden_tests.rs` 28、`bridge.rs` 20、`json.rs` 13、`number.rs` 11、`string.rs` 6、`bigint.rs` 2、`object.rs` 2、`error.rs` 1、`mod.rs` 1。
- 原因：仓库没有 `rustfmt.toml`，默认 `max_width = 100`，而代码是按约 120 列写的。
- 修复建议：运行 `cargo fmt -p raft-shared`，或者在工作区显式加一个 `rustfmt.toml`，并在 D 记录里说明。

---

## 可选

- **O1. 部分 D3 helper 尚未实现，且没有记录。** `normalize_nfkc`/`normalize_nfkd`、`to_locale_lower_case_en_us`、`locale_compare`、`JsRegex`、日期和时区相关函数、`Utf8StreamDecoder`、`ReadlineSplitter` 都还没有。如果"core"单元有意把它们留到后面，请在 decisions 或 scope 里写明，避免被当作已完成。另外，`collapse_js_whitespace`、`less_than`、`is_finite`、`to_number_value`、`trim_start`/`trim_end` 是 D3 列表之外新增的公共名字，请补登记到 D3（参照 pilot O2）。
- **O2. `json_parse(&str)` 不接受 `JsString`。** 经过 `utf16_slice` 截断后含孤立代理的文本，要先有损转换才能解析。可以增加 `json_parse_js(&JsString)`，让 `&str` 版本调用它。
- **O3. `JsString` 的 `Debug` 用的是 lossy 转换**（`string.rs:52-56`）。golden 失败信息里看不到孤立代理，排查时会误判。建议打印成 `\u{d800}` 的形式。
- **O4. `parse_int`/`to_number` 在 2 的幂进制下是 O(n²)。** `power_of_two_radix_value`（`number.rs:179`）每读一位都对整个 BigUint 做一次 `shl`，10⁵ 位的十六进制串大约要 1e9 次操作。可以先按位打包再构造 BigUint。
- **O5. `from_value` 在 `visit_object`（`bridge.rs:579`）里多收集了一次 `Vec`，每个 map key 也都 clone 成 `Value::String`**（:508, :537）。可以直接迭代 `object.iter()`，key 用借用。
- **O6. golden 测试通过 `include_str!("../../../../tests/golden/...")` 引用 crate 目录之外的文件**（`golden_tests.rs:7`）。这会导致 `cargo package` 失败。目前不影响，建议记一笔。

## 已确认没有问题
- §13 依赖：只有 `serde` 和 dev 依赖 `serde_json`（带 `float_roundtrip`，所以 golden 里的数值解析是精确的）。
- 没有 `unsafe`。`JsError`/`Value`/`JsString`/`Object` 都是 `Send + Sync`。`JsError` 的 `Display` 等于 message，并提供了 `js_name()`。
- `json_parse` 用显式栈迭代实现，不会因为解析本身栈溢出。
- golden 的数值比较使用 `to_bits`，能区分 −0 和 0，NaN 视为相等。字符串按 code unit 逐个比较，孤立代理可以保留。

---

## 第二轮复查

**结论：仍不通过。** 新发现 1 个阻塞项、2 个应修项。第一轮的 3 个阻塞项和 9 个应修项里，S1 只修了一半（按键删除仍然是 O(n)），其余都已修复。

本轮核验：`cargo test -p raft-shared` 45 个测试通过（耗时 19 s），clippy 0 条警告，`cargo fmt --check` 无 diff。探针在 release 构建下运行，对照 Node v24.15.0。

### 第一轮各项的状态
| 项 | 状态 | 核验方式 |
|---|---|---|
| B1 深层嵌套导致栈溢出 | 已修复 | 线程栈只给 2 MiB：1e6 层数组经过 parse、stringify、clone、==、drop 全部正常；2e5 层对象 drop 正常。`Value` 用显式栈实现 Drop/Clone/Eq；Debug 超过 64 层后截断输出。 |
| B2 `error_to_string(&err)` 与 trait | 已修复 | 新增 `JsErrorName` trait 和 `error_to_string<E: JsErrorName + ?Sized>`，保留 `error_to_string_parts`，D3 已登记。 |
| B3 键序 golden 是同义反复 | 已修复 | `matches_golden`（golden_tests.rs:84）直接拿 V8 记录的键序列比对，json_parse、assign、fromEntries 三处都已切换过去。 |
| S1 Object O(n²) | **部分修复** | 8 万个键的 parse 从 29.2 s 降到 0.09 s，但 `remove_js` 仍是 O(n)，见下方 N2。 |
| S2 pad 超长时进程中止 | 已修复 | 现在返回 `RangeError: Invalid string length`。 |
| S3 JsString 经 serde_json 序列化后形状改变 | 已修复 | serde_json 输出 `"a�"`；`to_value` 输出 `String("a\u{d800}")`，保留原始 code units。 |
| S4 数值转换出口 | 已修复 | 新增 `convert.rs`，导出 13 个转换函数。 |
| S5 JsString 拼接 | 已修复 | `push_*` 已改为 pub，并提供 `concat`、`Add`/`AddAssign`/`Extend`/`FromIterator`。 |
| S6 Value 字面量构造 | 已修复 | 提供了 `From<bool/f64/i32/u32/&str/String/JsString/Vec/Object/Option<T>>`。另外 `Object` 新增 `iter_mut`、`retain`，以及拥有所有权和借用两种 `IntoIterator`。 |
| S7 parse_int 签名 | 已修复 | D3 已改为记录 `Option<f64>`。 |
| S8 clippy / S9 fmt | 已修复 | 0 条警告，0 处 diff。 |
| O1 D3 列表外的名字、未实现 helper 的推迟 | 未修复 | D3 没有登记 `collapse_js_whitespace`、`less_than`、`is_finite`、`to_number_value`、`trim_start`/`trim_end`、`json_parse_js`，也没写明 regex、locale、日期等 helper 推迟到后续单元。 |
| O2 `json_parse_js` | 已修复 | |
| O3 JsString Debug | 已修复 | |
| O4 2 的幂进制 O(n²) | 已修复 | 改用 `from_radix_digits`。 |
| O5 visit_object 多余的收集 | 未核实 | 属于可选项，本轮未复查。 |
| O6 include_str 引用 crate 外文件 | 未修复 | 属于可选项。 |

### 新发现

#### N1（阻塞）`trim` 会去掉 U+0085，与 V8 不符；D3 记下的是一条错误事实，单元测试断言的也是错误行为
- 位置：`string.rs:210-214`，即 `is_trim_whitespace = is_js_whitespace || u == 0x0085`。单元测试在 `string.rs:514`（`trim_strips_u0085_but_whitespace_class_does_not`）。D3 第 293 行写着 "Only the trim family strips U+0085"。
- 对照结果：在 Node 24.15.0 中，`const s = String.fromCharCode(0x85) + "x" + String.fromCharCode(0x85); s.trim()` 的结果长度是 3，code units 为 `[133, 120, 133]`；`trimStart`/`trimEnd` 的结果长度也都是 3。U+0085 不属于 ECMAScript 的 WhiteSpace 或 LineTerminator，V8 不会去掉它。
- 为什么没被发现：golden 输入里没有 U+0085。这条"V8 怪癖"只有一个手写测试在断言，没有 oracle 对照。它违反了 D3 "oracle-generated golden tests … for each function" 的要求。
- 后果：一个带着 D3 名字的 helper 行为错误（pilot O2 描述的情况），而且它的文字还写进了决策记录。
- 修复建议：
  1. 删除 `is_trim_whitespace`，trim 系列直接使用 `is_js_whitespace`。
  2. 删掉或修正这个单元测试，并把 `"\u0085x\u0085"` 加进 `tools/golden/js-core.mjs` 的 trim 输入。
  3. 删除 D3 中的这句话。

#### N2（应修）`Object::remove_js` 是 O(n)，删除大量键时退化成 O(n²)
- 位置：`object.rs:102-116`。每次删除都要 `Vec::remove`，再把 `slots.values_mut()` 全部扫一遍。
- 探针结果：在一个 8 万个键的对象上删掉一半键，耗时 **128.7 s**。
- 修复建议：用墓碑实现删除，例如把 `named` 改为 `Vec<Option<(JsString, Value)>>`，墓碑数量超过一半时再压缩；或者改成带插入序号的索引映射。同时补一个"大对象删除一半键"的性能回归测试。

#### N3（应修）`Value` 从约 40 字节涨到 96 字节，每个空 Object 都带一个 HashMap 和一个 BTreeMap
- 探针结果：`size_of::<Value>() = 96`，`size_of::<Object>() = 96`。数组里的每个元素，哪怕只是数字，都占 96 字节。每个对象还要初始化一个 `RandomState`、复制一份键（`slots` 里存了键的克隆）。上面那个 2 MiB 线程的探针峰值 RSS 达到 668 MB。服务端返回的大数组会因此多占 2 到 3 倍内存。
- 键序本身是确定的：迭代只依赖 BTreeMap 和 Vec，HashMap 只用来查找，不影响顺序，这一点没有问题。
- 修复建议：
  - 把变体改成 `Value::Object(Box<Object>)`，或者让 `Object` 内部只持有一个 `Box<Inner>`，使 `Value` 回到 32 字节左右。
  - 小对象（比如少于 16 个键）不建 `slots`，直接线性查找，超过阈值再建索引。

#### N4（可选）serde 桥的 128 层深度上限
- `from_value`/`to_value` 超过 128 层会返回 RangeError，而 `json_parse` 本身没有深度限制。这一点 D3 已登记，typed struct 一般也不会这么深，所以只作记录。
- thread-local 捕获的安全性已核查，没有问题：`CaptureGuard` 在 drop 时会复位标志；`Units::serialize` 用 `replace(false)` 一次性消费标志；嵌套调用和经过 serde `Content` 回放时都不会串值。捕获状态是线程局部的，不跨线程。
- 但在 `#[serde(flatten)]`、untagged 或 internally-tagged 的情况下，`JsString` 会先被 Content 当作普通字符串缓存，这时孤立代理变成 U+FFFD。建议在 `to_value` 的文档里写明这一点。

#### N5（可选）Drop 语义和测试耗时
- `Value` 实现了 `Drop` 之后，`match v { Value::Array(a) => … }` 这种按值移出的写法编译不过（E0509），翻译者只能用 `into_array`/`into_object`/`into_js_string` 或 `mem::take`。文档已经写明，但 mapping-guide §6 还没提示，建议补一句。
- 测试耗时从 0.05 s 增加到 19 s，原因是 debug 构建下的 1e6 层嵌套测试和 16 万个键的测试。可以缩小规模，或者只在 release 下运行。

### 汇总
仍有阻塞项 1 个（N1），应修项 2 个（N2 是 S1 未修完的部分，另一个是 N3）。O1 的登记工作建议和 N1 的 D3 修正一起做。

---

## 第三轮复查

**结论：通过。没有剩余的阻塞项或应修项。**

本轮核验：`cargo test -p raft-shared` 46 个测试通过（1.67 s），clippy 0 条警告，`cargo fmt --check` 无 diff。探针在 release 构建下运行。

### 第二轮新发现的复查
| 项 | 状态 | 核验方式 |
|---|---|---|
| N1 trim 去掉 U+0085 | 已修复 | `is_trim_whitespace` 已删除，`string.rs:509` 的单元测试改为断言 U+0085 不是空白。golden trim 新增 `[133,120,133]` 用例，期望的三个结果都保持原样。D3 已更正。 |
| N2 删除键为 O(n) | 已修复 | 8 万个键的对象删一半键，从 128.7 s 降到 **0.058 s**。 |
| N3 Value 体积 | 已修复 | `Object` 改为 `Box<Data>`，`size_of::<Value>() <= 32` 有断言（`object.rs:356`）。不超过 16 个非下标槽位时不建 HashMap。在 2 MiB 线程栈上跑 1e6 层数组和 2e5 层对象的探针，峰值 RSS 从 668 MB 降到 336 MB。 |
| O1 D3 登记 | 已修复 | D3 第 301 行起记录了推迟到后续单元的 helper 和新增的公共名字。 |
| O6 include_str 引用 crate 外文件 | 未修复（可选） | `golden_tests.rs:7` 仍然用 `../../../../tests/golden/...`。只影响 `cargo package`，维持可选。 |
| N5 Drop 与按值移出 | 部分修复（可选） | `Value` 的文档注释和 D3 都已写明，但 mapping-guide §6 里仍然没有提示。 |

### 新 Object 布局的回归检查
我写了一个随机操作探针，用一个参考模型（按 V8 规则排序的 Vec）逐步对照。
- 覆盖范围：300 轮，每轮 3000 步。键空间分别取 5、20、60、300，从而跨过 16 的线性查找阈值和压缩阈值。操作混合了 insert、覆盖、delete、按下标键再插入和 `retain`。
- 每 97 步校验一次：`iter` 的顺序和值、`get` 查找结果、`clone` 后的 `==`、`len`、按所有权的 `IntoIterator`、`keys().rev()` 的长度。每一步都会调用一次 `iter_mut`。
- 结果：全部与参考模型一致。

代码审读的结论：
- `iter`、`iter_mut` 和 `IntoIterator` 都用 `flatten` 跳过墓碑。
- `len` 使用 `named_live` 计数。
- `retain` 之后总是压缩；压缩后按槽位数重建或丢弃索引，槽位编号与压缩后的 Vec 一致。
- 删除时同步从 HashMap 里移除键；插入新键前先写入索引。
- 已删除的键再插入时会追加到末尾，符合 V8 的行为。
- `Value` 的 Drop 通过 `iter_mut` 取子节点，墓碑里不持有值，所以不会有遗漏或重复释放。

### 可选观察（不影响结论）
- `Object::new()` 和 `Default` 都会分配一次 `Box`，包括 `into_object` 里的 `mem::take`。每个空对象多一次堆分配。可以把 `Data` 改为 `Option<Box<Data>>` 懒分配，但目前的开销可以接受。

### 汇总
第一、二轮的所有阻塞项和应修项都已修复，只剩 O6 和 N5 两个可选项。
