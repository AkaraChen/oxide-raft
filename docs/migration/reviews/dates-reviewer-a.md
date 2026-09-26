# raft_shared::js 日期、时区与排序 — 对抗评审 A

结论：**通过（有条件）**。没有阻塞项，也没有应修项。发现 3 个可选项，另有 1 处文档数字错误，也记为可选。DateParser 移植、MakeDay/MakeTime、TimeClip、gap/fold 解析、toISOString、Intl 时区规范化，以及默认 locale（en-US）下的排序，在大规模差分测试中都与 Node 完全一致。只在以下几处发现与 V8 可复现的差异：1970 年以前、落在时区切换前最后 1 秒内、且带毫秒的时刻；ICU4X 缺少少数小语种的排序 tailoring；宿主 zoneinfo 中有、但捆绑库中没有的 TZ 文件名。

Oracle：Node 24.15.0（linux-x64，ICU 78.2，tz 2026a，V8 13.6）。每个 TZ 或 LANG 组合都在单独的子进程中运行，子进程环境只包含 PATH 和被测变量。

探针位于 scratchpad `datesReviewA/`，说明如下：
- Rust 端：`src/main.rs`，逐行读取 `[op, env, args]`。
- Node 端：`node_run.mjs`，按 env 分组派生子进程。
- 比对：`cmp.mjs`。
- 生成器：`gen_*.mjs`。

## 已验证无差异的范围（不在 golden 集中）

**Date.parse（ISO 与 legacy 两套文法），共 180,668 例，0 差异。** 测试时区为 America/New_York、Asia/Shanghai、Australia/Lord_Howe、Europe/Dublin。输入分三类：
- 约 170 个手写边界用例：
  - 扩展年份：`±YYYYYY`、`-000000`、`±275760/-271821` 的 TimeClip 边界，以及它们的本地时间版本（本地时间可越界 10 天）。
  - `T24:00` 的允许与拒绝。
  - 小数秒：1 到 14 位，含前导零与 9 位有效数字截断。
  - 时区：`Z/z`、`+hhmm`、`+hh:mm`、`+2400`、`+0860`。
  - 两位年份：0–49 映射到 20xx，50–99 映射到 19xx，`100`。
  - 星期名与月份名：长词只匹配月份，`Juneteenth` 等。
  - 括号注释：嵌套、未闭合。
  - 时区缩写：UT、UTC、GMT、EST、EDT、CST、CDT、MST、MDT、PST、PDT，以及 `GMT+5:30`、`GMT-0330`。
  - 其他：`am/pm` 与 12/13 点、`::`、`7:` 的形式、`U+0000` 截断、各类 Unicode 空白（`᠎`、`\u0085`、`﻿`、`　` 等）、全角数字、`U+2212`、在夏令时 gap 或 fold 中的 ISO 本地时间、1941 年、1800 年、9999 年。
- 60,000 个随机 token 串。
- 40,000 个结构化的 legacy 和 ISO 串。
- 另有 80,000 个字符级变异串（插入、删除、替换控制字符和空白）。

**local_fields / get_timezone_offset / make_local_time：对全部 628 个 ICU id 做了转换点扫描，共约 370 万例。**
- 做法：在 Node 中按 12 小时步长扫描 1800–2110 年，二分查找到每个转换点的精确毫秒，然后在该点前后取样：
  - UTC 时刻：前后 2 小时内的 8 个点（含 ±1 ms）。
  - 本地墙钟时间：gap 和 fold 两侧 ±3 小时，每 15 分钟一个点，外加 ±1 ms 和随机点。
- 每个时区另有 300 个随机时刻，覆盖 ±8.64e15 全范围和 ±27 万年，外加 16 个年份（1、100、1000、2400、10000、275759 等）在各类 DST 规则日的取样。
- `make_local_time`（即 `new Date(y,m,d,…)`）只在 Cuba、Iran、PRC、ROC、ROK、W-SU 上与 Node 不一致，原因是本容器缺少 tzdata-legacy（见下文“非发现”）。其余 gap/fold 结果全部一致，包括：
  - Lord_Howe 的 30 分钟 DST、Dublin 的负 DST、Troll 的 +2 小时 DST、Apia 跳过整日、Casablanca 和 Gaza 的斋月规则。
  - 远未来（ICU 使用 finalZone 规则，jiff 使用 POSIX footer 加 400 年周期平移）。
  - LMT 带秒偏移时 `getTimezoneOffset` 向零截断（例如 Asia/Calcutta 1854 年为 `-353`）。
- 结论：两种路径都按“转换前的偏移”解析 gap 和 fold，这与 ICU `getOffsetFromLocal(FORMER, FORMER)` 在 OlsonTimeZone 历史段和 SimpleTimeZone final 段上的行为一致。

**Date.UTC / new Date(7 个参数)，共 40,000 例，0 差异。**
- 随机字段混入以下特殊值：NaN、±Inf、±1e6 与 ±1e7 边界、2^31、2^53、1e20、`-0.5`、`99.9`、`-1e-9`，以及 `new Date(-0.5, …)` 的 1900 规则。
- 在 3 个时区下验证。

**toISOString 和 toLocaleDateString("en-US", {month:"short", day:"numeric", timeZone:"UTC"})：各 4,024 例，0 差异。** 覆盖 ±0、±0.5、±1.9、8.64e15 ± 0.5、NaN、±Inf、1e300，以及全范围随机值。

**TZ 环境变量解析，共 3,300 个取值 × 7 项检查。** 取值由每个 id 的原样、小写、`:` 前缀、`posix/` 前缀、`right/` 前缀组合而成，另加约 110 个手写取值：
- POSIX 规则串：`JST-9`、`EST5`、`<+0330>-3:30`、`CET-1CEST,M3.5.0,M10.5.0/3`。
- 路径形式：`Asia//Tokyo`、`./`、`..`、绝对路径、`::`、`posix/right/`、目录名、`zone.tab`、尾随 `/`。
- `GMT±N` 与 `Etc/GMT±N` 的边界、3–4 字母缩写（EST、MST、HST、IST、CST 等）、非 ASCII。
- 检查项：`resolvedOptions().timeZone`、4 个时刻的本地字段、gap 和 fold 构造、本地 ISO 解析。
- 结果：只有 legacy 短 id（环境原因）和 `posixrules`（见发现 3）不一致。TZ 未设置（系统时区）的情况也一致。

**canonicalize_time_zone，共 3,777 例，0 差异。** 输入包括：
- 每个 id 的原样、大写、小写、大小写交替，下划线换成空格，加尾随空格，`/` 写成 `//`。
- `Intl.supportedValuesOf("timeZone")` 的全部取值。
- 偏移形式：`±HH`、`±HHMM`、`±HH:MM`、`−` 和 `‒`、`+24`、`+23:60`、`+08:00:00`。
- 其他：`GMT+8`、`UTC+8`、`Etc/GMT±0` 系列、`SystemV/*`、`US/Pacific-New`、`Factory`、`Etc/Unknown`、`Aſia/Shanghai`、`Shanghaİ`、`port-au-prince` 等。

**locale_compare，共约 87 万个比较对。**
- 42 种 LANG 取值 × 3000 个随机串对。随机串包含组合字符、零宽字符、软连字符、Hangul、`ǅ`、`ﬁ`、全角字符、emoji、`\u0000`、`￿`、`U+10FFFF`。
- 另有 139 种 LANG 取值 × 1,431 对，以及 CLDR 有排序数据的约 100 种语言 × 4,465 对。
- 默认 en-US，以及 da、sv、de、zh-CN、zh-TW、ja、ko、tr、lt、cs、pl、fi、th、ar、he 等全部一致。LANG 为空（`und`）、`C`、`POSIX`、`C.UTF-8`、`@euro` 也一致。

**非发现（不计入结论）：**
1. **V8 DateCache 的历史依赖。** 首轮扫描中，America/Fortaleza 2000-10-08 至 10-22、America/Argentina/Tucuman 2004-06、Africa/El_Aaiun 1976-04、Europe/Riga 1944、Europe/Tirane 1943、Europe/Simferopol 1944、Asia/Gaza 和 Asia/Hebron 2040 与 2086 都出现了差异。这些都是两次转换间隔小于 19 天的短暂时段。复查后确认：Node 的结果取决于同一进程中此前查询过哪些时刻。例如 `TZ=America/Fortaleza`：
   - 新进程中直接查询 `new Date(970975800000).getTimezoneOffset()`，得到 `120`，与 Rust 相同。
   - 先查询前一天的时刻，同一个值就变成 `180`。
   - 同一进程中 `toString()` 也给出 GMT-0200。

   这是 V8 偏移缓存（`kDefaultDSTDeltaInSec` = 19 天）造成的，Rust 与“干净进程”的 ICU 结果一致，因此不算缺陷。建议：golden 脚本如果要覆盖这类时段，应每个查询单开一个进程。
2. **本容器缺少 tzdata-legacy。** 在 Node 中，`TZ=PRC`、`ROC`、`ROK`、`Cuba`、`Iran`、`W-SU` 触发 ICU 的“3–4 字符 id 与 libc 标准偏移不符则丢弃”规则，libc 读不到文件，取 0，于是整个时区被当作 UTC。Rust 用捆绑库模拟 libc，结果与完整主机一致。这属于环境差异，按指示不计。
3. **`LANG="garbage value"`（含空格）会让 Node 24.15.0 在首次调用 `localeCompare` 或 `Intl.Collator` 时段错误（SIGSEGV，退出码 139）。** 没有可对照的结果，Rust 返回的是 root 排序。这类取值不现实，只作记录。

## 发现

### 1. [可选] 1970 年以前的时刻，若落在转换点前 1 秒内且毫秒不为 0，会得到转换**之后**的偏移
- 位置：`tz.rs` 中的 `Rules::offset_ms_at_utc`（`Timestamp::from_millisecond(...)` 后接 `tz.to_offset(ts)`）。
- 根因：jiff 0.2.31 `tz/tzif.rs:232` 的 `to_local_time_type` 使用 `timestamp.as_second()`，即 `Duration::as_secs`，它**向零截断**。负时间戳 `-1633280400.001 s` 因此被截成 `-1633280400`，恰好等于转换时刻，于是取到了新偏移。
- 输入示例：
  - `TZ=America/New_York`，时刻 `-1633280400001`（1918-03-31T06:59:59.999Z）：
    - Node：`getTimezoneOffset()` 为 `300`，`getHours()` 为 `1`（01:59:59.999 EST）。
    - Rust：`240`，`2`（02:59:59.999 EDT）。
    - 同一秒内的 `-…400500` 和 `-…400999` 也是如此；`-1633280401000` 则一致。
  - `TZ=Asia/Shanghai`（D23 规定的测试时区），时刻 `-1600675200001`：Node 为 `-480`、23 点，Rust 为 `-540`、次日 0 点，日期也不同。
  - LMT 结束的转换也受影响，例如 `TZ=Asia/Shanghai`，时刻 `-2177481943001`：Node 为 `-485`（23:59:59），Rust 为 `-480`（23:54:16）。
- 规模：在全部 id 的转换点扫描中有 10,332 例，每个在 1970 年前有转换的时区都受影响。`make_local_time` 不受影响，因为它走 `to_ambiguous_timestamp`，civil 时间换算时向下取整。
- 影响：`local_fields`、各个 `get_*` 与 `get_timezone_offset` 在上述时刻输出错误的本地时间和偏移，也就是 `_format.ts` 的 `toLocalTimeWithOffset` 与 `search.ts` 的 `formatLocalOffsetIso` 会出错。现实输入（消息时间、提醒时间）都在 1970 年之后，所以实际上触达不到。不过它违反了 D3 与 V8 逐位一致的约定，修复只需一行。
- 建议：先按秒向下取整再交给 jiff，例如 `Timestamp::from_second(into_jiff_range(utc_ms).div_euclid(1000))`。tz 的转换点都在整秒上，所以这样做是精确的。同时加入回归用例 `(America/New_York, -1633280400001) → 300` 和 `(Asia/Shanghai, -1600675200001) → -480`。

### 2. [可选] 少数 locale 的排序与 ICU 不一致（ICU4X 缺少 tailoring，或 locale 推导不同）
- 位置：`collate.rs` 中的 `default_locale` 和 `collator`。
- 输入（`env -i LANG=<x> node -p 'a.localeCompare(b)'` 与 Rust 对照）：
  - 缺少 tailoring，Rust 回落到 root 排序：
    - `kl`/`kl_GL`：`"ä"` 对 `"å"`，Node `-1`，Rust `1`。
    - `se`/`se_NO`：`"á"` 对 `"à"`，Node `1`，Rust `-1`。
    - `smn`、`haw`、`lkt`、`bo`（例如 `"a"` 对 `"ཀ"`，Node `1`，Rust `-1`）。
    - 各 locale 在约 4,500 对中有 7 到 297 对不一致。
  - `pa_PK`：ICU 把它最大化为 `pa-Arab-PK`，使用阿拉伯字母排序（`"a"` 对 `"ا"`，Node `1`）。Rust 按 `pa`（Gurmukhi）排序，得到 `-1`。
  - `cmn_TW`：Node 的 locale 保持 `cmn-TW`，按 root 排序（`"a"` 对 `"中"` 为 `-1`）。Rust 的 `LocaleCanonicalizer` 把它改写成 `zh-TW`（stroke 排序），得到 `1`。
  - POSIX 修饰符 `@collation=…` 会被 ICU 用作排序关键字：`LANG=zh_CN@collation=stroke` 时 Node 按笔画排序（`中`/`日`/`本` 排为 `本,日,中`），Rust 忽略它，在 3,000 对中有 33 对不一致。`default_locale` 的返回值也不同：Node 的 `DateTimeFormat` locale 为 `zh-CN`/`de-DE`/`es-ES`，Rust 为 `zh-CN-x-lvariant-collation=stroke`、`de-DE-x-lvariant-phonebook`、`es-ES-x-lvariant-collation=traditional`。
- 影响：只会影响用户 LANG 为这几种小语种或带排序修饰符时，commander 候选命令的排序以及 inbox 列表的排序。D24 只要求 en-US 由语料覆盖，其他 locale 用定向用例验证。
- 建议：
  - 在 D24 中列出已知不一致的 locale，说明它们回落到 root（ICU4X 2.3.1 编译数据中没有这些 tailoring）。
  - `default_locale` 可以在规范化之前识别 `@collation=`/`@phonebook` 这类 ICU 关键字修饰符，并对 `cmn` 这类“ICU 不做别名替换”的语言跳过 `LocaleCanonicalizer::new_extended`。
  - 如果不修，请在测试中写明这是已知差异。

### 3. [可选] TZ 取宿主 zoneinfo 中有、而捆绑库中没有的文件名时，libc 标准偏移不一致
- 位置：`tz.rs` 中的 `libc_standard_offset`（相对路径只查 `tzdb_zone_exact`）。
- 输入：`TZ=posixrules`。在本容器中，`/usr/share/zoneinfo/posixrules` 链接到 America/New_York。
  - Node：`resolvedOptions().timeZone` 为 `undefined`，`getTimezoneOffset()` 为 `300`（固定 -05:00，与 glibc 读取宿主文件的结果一致）。
  - Rust：`undefined`，偏移为 `0`。
  - 7 项检查全部不一致。
- 影响：极小，只有用户把 TZ 设成非标准的 zoneinfo 文件名时才会出现。D25 的设计本来就是“捆绑库代替宿主 tzdata”，所以这是设计取舍带来的边角差异。
- 建议：在 `tz.rs` 模块注释或 D25 中注明，只识别捆绑库中的 IANA 名称，`posixrules`、`localtime` 等宿主专有文件按 UTC 处理。

### 4. [可选] D25 中的 id 数量与实际不符
- 位置：`docs/migration/decisions.md` D25 写的是 “`js/tz_ids.rs`, 639 ids”。
- 实际：`ZONE_IDS` 只有 628 条。
- 在 canonicalize 测试中，全部 628 条以及 `Intl.supportedValuesOf` 的取值都一致，说明没有可观察到的缺失 id。数字应改为 628，或者说明计数口径。

## 附：iso_ms
- 以 `+275760-09-13T00:00:00.000Z` 往返时，序列化与反序列化都一致。
- 带偏移的 legacy 串（`Tue, 21 Apr 2026 07:00:00 GMT`）会被接受；没有偏移的本地串会被拒绝。这是有意的设计，文档中已说明。
- 未发现与 `Date.parse` 语义不符之处。
