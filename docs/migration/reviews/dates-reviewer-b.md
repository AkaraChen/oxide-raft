# raft_shared::js 日期、时区与排序：对抗审查 B（排序、时区选择、契约）

**结论：不通过。** 共 2 个阻塞项、5 个应修项、2 个可选项。两个阻塞项：
1. ICU4X 对汉字（CJK 表意文字）的排序与 Node（ICU 78.2）不同，en-US/root 以及几乎所有 locale 都受影响。
2. `TZ` 未设置、并且 `/etc/localtime` 无法解析成 zoneinfo 路径时，或者 `TZ` 是带 DST 的 POSIX 规则、同时没有 `/etc/localtime` 时，Rust 固定使用 UTC，而 Node 走 glibc `tzname` 这条路得到正确的偏移。常见的 Docker 做法（把 `Asia/Shanghai` 复制成 `/etc/localtime`，不装 tzdata）会让所有本地时间偏 8 小时。

审查范围：`crates/raft-shared/src/js/{collate.rs,tz.rs,tz_ids.rs,date.rs}` 的排序和时区部分、`js/mod.rs` 的导出、根 `Cargo.toml` 的版本锁定。Date 解析和算术由审查 A 负责，这里只在涉及 `Env` 或契约时提到。

测试方式：
- 把 crate 快照复制到 `scratchpad/datesReviewB/crates/raft-shared`，探针 `scratchpad/datesReviewB/probe` 用 release 构建。
- 对照 Node 24.15.0（`process.versions`：icu 78.2、unicode 17.0、cldr 48.0、tz 2026a），子进程一律用 `env -i`。
- `/etc/localtime` 的各种情形在 `unshare -m` 下用 bind mount 构造，没有改动宿主机。
- 本单元自己的 17 个 date/tz/collate 单测全部通过。同一次运行里 `json_schema::golden_tests::validate_records_match_ajv` 失败，属于别人正在编辑的模块，与本单元无关。

---

## 阻塞

### B1. 汉字排序与 ICU 78.2 不一致（en-US、root、ja、ko 以及所有西文 locale）
- 位置：`collate.rs` 整体依赖 `icu_collator =2.3.1` 的 compiled data。数据的 README 自称是 CLDR 48.2.1 / ICU 78.1rc，但实际比较结果和 ICU4C 78.2 不同。
- 探针结果（LANG 未设置，Node 解析为 en-US）：

  | a | b | Node `a.localeCompare(b)` | Rust `locale_compare` |
  |---|---|---|---|
  | 缵 U+7F35 | 纓 U+7E93 | -1 | Greater |
  | 觃 U+89C3 | 規 U+898F | -1 | Greater |
  | 贠 U+8D20 | 財 U+8CA1 | -1 | Greater |
  | 鼋 U+9F0B | 鼀 U+9F00 | -1 | Greater |
  | 㝳 U+3773 | 寺 U+5BFA | -1 | Greater |
  | 𪛙 U+2A6D9 | 丁 U+4E01 | -1 | Greater |

  - 3760 个 GB2312 一级汉字，随机取 23,759 对：en-US 下 **101 对**符号相反（例如 蒸/管、袍/巡、最/定、墨/捻），ja-JP 下 14 对；zh-CN、zh-TW 下 0 对。
  - 把 BMP、SMP、SIP、TIP 的全部单个码位（260,591 个相邻对）按 Node 排好序后逐对比较，en-US 有 4274 处不一致。去掉 U+20000 以上的码位后，en-US 还剩 39 处，ja 34 处，ko 33 处，zh-CN 3 处，其余西文 locale 都是 39 处；**不一致全部落在汉字和部首上**。
  - 非汉字的结果一致：标点、空白、数字、大小写、重音、组合记号、Hangul、泰文、emoji、NUL、可忽略字符、1.2 万单位长的字符串，在 29 个 locale 下每个都比较了 2 万多对，除孤立代理项（见 O2）外都是 0 差异。
- 影响：
  - 上游的 `agentProcessManager.ts:8108`（`a.name.localeCompare(b.name)`）和 `agentInboxProjection.ts:104`（`a.target.localeCompare(...)`）都会排序 agent 名和频道名。按 D23，用户在 UTC+8，名字里有中文很正常。
  - daemon 以 systemd 或 launchd 服务运行时通常没有 LANG，会落到 en-US，正好是出错的那一类 locale。
  - golden 测不出这个问题：`tools/golden/dates.mjs` 的 `COLLATE_INPUTS` 只有「中文」「日本」两个汉字串。
- 修复建议：
  - 先弄清楚原因。可能是 ICU4X 对 Unified_Ideograph 用隐式权重（接近码位序），而 ICU4C 78 root 使用 FractionalUCA 的部首笔画显式主权重。
  - 如果 ICU4X 没有可用的版本或选项能对齐，就需要在 D24 或 D25 里改选型或记录差异。一种做法是对 Han 码位引入 Node 生成的部首笔画主权重表作为预处理。
  - 在 golden 里加入 GB2312、Big5 常用字的随机对，以及 Ext A–J 的样本。

### B2. `/etc/localtime` 无法映射到 zoneinfo 名时一律返回 UTC；ICU 在这种情况下走 `tzname` + `remapShortTimeZone`
- 位置：`tz.rs:398-425`（`system_zone_id`）。canonicalize 后的路径里没有 `/zoneinfo/`、在 zoneinfo 里搜不到内容相同的文件、或者 `/etc/localtime` 不存在时，函数都直接返回 `"UTC"`。`tz.rs:334-339` 也只处理了 `resolve_host_zone` 的 `std offset` 形式。ICU `uprv_tzname` 在这些情况下会退回 `U_TZNAME`，也就是 glibc 的 `tzname[0]`，再经 `remapShortTimeZone` 按缩写、DST 缩写和偏移映射到一个 Olson id；映射不上时用 `SimpleTimeZone(timezone)`。
- 探针结果（在 `unshare -m` 里 bind mount 假的 `/etc` 或 `/usr/share/zoneinfo`；`offs` 是 `getTimezoneOffset`，最后一列是 `new Date(2026,2,8,2,30)`）：

  | 情形 | Node | Rust |
  |---|---|---|
  | `/etc/localtime` 是 `Asia/Shanghai` 的副本，没有 `/usr/share/zoneinfo`（Docker 常见做法）；TZ 未设置 | offs 全是 -480；时区名 `America/Chicago`（来自 ICU 的 "CST"） | offs 0，`UTC` |
  | 同上，副本分别是 Tokyo、Berlin、New_York、Kolkata、London | Asia/Tokyo -540；Europe/Brussels，冬 -60、夏 -120；America/New_York，300 和 240；Asia/Calcutta -330；Europe/London，0 和 -60 | 全部是 UTC、0 |
  | `/etc/localtime` 链接到 `/opt/.../Tokyo`（路径中没有 `/zoneinfo/`） | Asia/Tokyo | UTC |
  | 没有、悬空或为空的 `/etc/localtime`，`TZ=EST5EDT,M3.2.0,M11.1.0` | America/New_York，300 和 240 | UTC、0 |
  | 同上，`TZ=<+0530>-5:30` | 时区名 undefined，-330 | UTC、0 |

  - 对照组：`/etc/localtime` 是正常符号链接、相对链接或 `Etc/GMT-14`，或者是 zoneinfo 里某个文件的普通副本（包括 `Europe/Oslo` → `Arctic/Longyearbyen` 这种同内容文件）时，两边一致。
- 影响：
  - 运行在容器或最小系统里的 daemon 和 CLI，本地时间渲染（`message/_format.ts:66-76`）和 `body.tz` 会整体错位。D23 的用户群恰好是 UTC+8。
  - `tz.rs` 的注释写着 "No /etc/localtime: glibc runs on UTC and ICU takes its "UTC" abbreviation"，但这只在 `TZ` 未设置时成立。
- 修复建议：
  - 按 ICU 实现 `U_TZNAME` 分支。用 `libc_standard_offset` 的思路，再加上从 TZif 文件或 POSIX 规则里取出 std/dst 缩写和 DST 标志，然后移植 ICU `putil.cpp` 的 `OFFSET_ZONE_MAPPINGS` 与 `remapShortTimeZone`。映射不上时用 `SimpleTimeZone(rawOffset, abbr)`，并按 `detectHostTimeZone` 的 3–4 字符规则处理。
  - 读取 `/etc/localtime` 的内容时，要用 TZif 本身的规则（jiff `TimeZone::tzif`），不能只拿来和 zoneinfo 做文件比对。
  - 为上表每一行各加一个 golden 用例（需要在 CI 容器里用 bind mount 构造）。

---

## 应修

### S1. 带 `=` 或超过 8 个字符的 `@modifier` 会让 locale 解析失败，悄悄退回 root 排序
- 位置：`collate.rs:44-72`。modifier 原样拼成 `-x-lvariant-<v>`，`Locale::try_from_str` 解析失败后，`collator()`（:88-91）用 `unwrap_or_default()` 退回 root。
- 探针结果（corpus2 共 9533 对）：

  | LANG | Node 的 locale | Rust 的 `default_locale` | 不一致对数 |
  |---|---|---|---|
  | `sv_SE@collation=phonebook` | sv-SE | `sv-SE-x-lvariant-collation=phonebook`（实际用 root） | 438 |
  | `es_ES@traditional` / `es_ES@collation=traditional` | es-ES | … | 7 |
  | `ja_JP@calendar=japanese` | ja-JP | … | 277 |
  | `ko_KR@collation=search` | ko-KR | … | 285 |
  | `zh@collation=stroke` / `zh_CN@collation=zhuyin` | zh / zh-CN | … | 262 |
  | `da-DE.@verylongmodifier` | da-DE | … | 丹麦语的顺序整体丢失 |

  - 300 个随机组合的 LANG 值里，`default_locale` 的字符串有 179 个与 Node 不同，其中 74 个的排序结果也不同。
- 原因：ICU 的 `uprv_getDefaultLocaleID` 会去掉 `@key=value` 关键字，只把不含 `=` 的 modifier 当作变体，并且不做 8 字符的限制。
- 修复建议：按 ICU `uprv_getPOSIXIDForDefaultLocale` 和 `uloc_canonicalize` 的规则处理 modifier。任何解析失败都不要退回 root，应该退到去掉 modifier 的 base。

### S2. locale 规范化与 V8/ICU 不同，并且会改变排序结果
- 位置：`collate.rs:62-64`，`LocaleCanonicalizer::new_extended()` 以及对下划线的处理。
- 探针结果：

  | LANG | Node | Rust | 排序不一致 |
  |---|---|---|---|
  | `tl`、`tl_PH` | tl（使用 root） | fil（有 ñ/ng 的定制规则） | 12 / 9533 |
  | `da__DK`、`da__DK@euro` | da-x-lvariant-dk（使用 da） | und | 818 / 9533 |
  | `en_Hant_TW`、`da_Hant_TW@latin`、`haw_Hant_TW…` | 汉字排在拉丁字母后面 | 汉字排到最前（按 Hant 脚本套用了 zh-Hant 的重排） | 有 |

  - 只影响 `default_locale()` 字符串（它是 pub API）、不影响排序的：`en_US@posix`（Node en-US，Rust en-US-posix）、`@euro`、`i_default`（en）、`en_US:de`（en）、`c`（en-US）、`posix`、`_DK`（und-DK）、`sh`（Node sh，Rust sr-Latn）、`C.UTF-8@euro`、`da_DK_POSIX`、`en@a@b`。
- 修复建议：
  - 按 ICU 的 `uloc_getDefault` 流程（canonicalize 使用 ICU 的别名表，不使用 CLDR extended）重写，或者直接用 Node 生成的 LANG→locale 映射 golden 驱动实现。
  - 构造 collator 时使用 Node 的 resolved locale，而不是从脚本推出的 locale。

### S3. ICU4X 的 compiled data 缺少几种 ICU 78 带有的定制排序规则
- 探针结果（corpus2 共 9533 对）：`se` 615、`se_NO` 615、`smn` 579、`kl` 470、`haw` 141、`lkt` 133、`mr` 4、`kok` 4。另外 124 种语言的结果都是 0。
- 修复建议：在 D24 或 D25 里记录这些 locale 与 Node 不一致，或者补充自定义数据。在 `locale_compare` 的文档里列出不支持的 locale。

### S4. 契约与文档：新增的公开名字没有登记；`Env` 与 §7 冲突；mapping-guide 的签名写错
- D3 的做法是登记超出列表的公开名字（"Public names added beyond the list above"），但本单元新增的下列名字在 `docs/` 里一处都搜不到：`default_locale`、`locale_compare_js`、`canonicalize_time_zone`、`date_parse_js`、`date_parse_absolute`、`date_utc`、`make_local_time`、`time_clip`、`utc_fields`、`local_fields`、`DateFields`、`get_*` 系列、`Env`（trait）、`SystemEnv`。
- mapping-guide §5.9 写的是 `js::locale_compare(a, b)`（两个参数），实际签名和 D24 都是三个参数 `(a, b, &Env)`。
- mapping-guide §7 规定 `Env` 是 `raft_shared::env` 里的有序 map 类型（Windows 上大小写不敏感）。`js::Env` 却是一个同名 trait，并且只为大小写敏感的 `BTreeMap`/`HashMap` 做了实现。两个名字必定冲突，Windows 语义也对不上。
- `tz.rs:32` 的注释 "Only `TZ` is read here" 已经过时，collate 也会读 `LC_ALL`、`LC_MESSAGES`、`LANG`。
- `tools/golden/dates.mjs` 的 `COLLATE_INPUTS` 只有 32 个串，没有孤立代理项，没有非 en-US 的 locale（D24 说其他 locale 靠单测覆盖，但单测只有 da、sv、zh 三个），所以 B1、S1、S2、S3 都测不出来。
- 修复建议：在 D3/D24 里登记上述名字；修正 §5.9；把 `js::Env` 改名（例如 `EnvRead`），并在 §7 写明 `ProcessEnv` 要实现它；按上面各项扩充 golden。

### S5. D25 声称「无论宿主装了什么 tzdata 结果都一样」，这与实现不符
- 实现里会读取宿主 tz 数据的路径：
  - `TZ` 是绝对路径时读取该文件（`tz.rs:262-266`）。
  - `TZ` 未设置或者是 POSIX 规则时，读 `/etc/localtime` 的 realpath，并在 `/usr/share/zoneinfo` 里逐个比对文件内容（`tz.rs:398-452`）。
  - Windows 上用 jiff 的 `tz-system`。
  - 这些是和 Node 保持一致所必需的，但与 D25 的表述矛盾。
- 另一方面，`TZ` 是 ICU 不认识、但宿主 tzdata 里有的名字时，Rust 用内置数据代替宿主数据。本机的例子：
  - `TZ=posixrules`：Node 恒为 300，Rust 为 0。
  - 本容器没有 tzdata-legacy，`TZ=PRC`/`ROC`/`ROK`/`W-SU`/`Cuba`/`Iran` 在 Node 下是 0 偏移，Rust 下是正确偏移。**这一条依赖宿主**：装了 tzdata-legacy 的机器上两边会一致。
- 修复建议：修改 D25 的措辞，列出上面这些依赖宿主的路径，并说明内置数据只用于在 ICU id 表中找得到的 id。

---

## 可选

### O1. 每次比较都重新解析 locale、重新构造 canonicalizer，还要拿一把全局锁
- `locale_compare` 每次调用都会：做 3 次 env 查询、执行 `LocaleCanonicalizer::new_extended()`、调用 `format!`，并获取全局 `Mutex` 查一次 HashMap。
- 实测：LANG 未设置时 134 ns/比较，`LANG=zh_CN.UTF-8` 时 338 ns/比较；单独调用 `default_locale` 需要 191 ns。在多线程 daemon 里还会争用锁。
- 修复建议：以原始的 `(LC_ALL, LC_MESSAGES, LANG)` 三元组作为缓存键，把 canonicalizer 放进 `OnceLock`，或者按线程缓存。时区查询是 86 ns/次，可以接受。

### O2. 孤立代理项的排序与 Node 不同
- 这是 ICU4X 的设计：孤立代理项一律当作 U+FFFD，彼此相等。ICU4C 则按代理码位排序。
- 探针结果：`"\ud800"` 与 `"\udfff"`，Node 给 -1，Rust 给 Equal；`"\ud83d"` 与 `"😀"`，Node 给 1，Rust 给 Greater。corpus1 的 11,875 对中有 50 对不一致。
- `locale_compare_js` 的注释承认了这一点，但 D24 没有记录。commander 的 `suggest_similar` 走的正是这个函数。
- 修复建议：在 D24 里记录这项差异；或者预先把孤立代理项映射到能保持 ICU4C 相对顺序的私用区码位（需要验证）。

---

## 已确认没有问题（有探针覆盖）
- 排序 locale 的环境变量优先级：`LC_ALL` > `LC_MESSAGES` > `LANG`；设置但为空时是 und；`C`、`POSIX`、`C.UTF-8` 解析为 en-US；`LC_CTYPE`、`LC_COLLATE`、`LANGUAGE` 不起作用。17 种组合与 Node 一致。
- 在 29 个常见 locale（da、de、sv、fi、tr、zh-CN、zh-TW、ja、ko、th、es、fr-CA、cs、pl、ru、ar、hi、vi、lt、hu、is、nb、he、el、hr、sk、uk、und、en-US）下，每个 locale 都比较了约 2.1 万对（随机对加上排序后的相邻对，包括只差可忽略字符的串和长串）。除了汉字（B1）和孤立代理项（O2），差异为 0。
- `canonicalize_time_zone` / `is_valid_time_zone`：1952 个输入（全部 ICU id 的原样、小写和大写形式，外加偏移量、U+2212、非 ASCII 大小写、空白、`Etc/Unknown`、`Factory` 等）与 `new Intl.DateTimeFormat("en-US",{timeZone})` 完全一致。
- 时区数据：用 628 个 ICU id 分别作为 `TZ`，每个比较 4453 个时刻的 `getTimezoneOffset` 和 1140 个本地时间（覆盖 gap 和 fold），与 Node 完全一致。例外只有 S5 中依赖宿主的 legacy 名字。
- 另外，Node 自身在同一进程里连续查询时，会因为 V8 DateCache 在个别时刻给出不同的偏移（`Africa/Tunis` 1943-04-17、`Europe/Vienna` 1945-04-04）；单独查询时和 Rust 一致。制作 golden 时应当每个时刻用新进程，或者避开这类时刻。这不是 Rust 的问题。
- 108 种 `TZ` 字符串与 Node 一致（`/etc/localtime` 为 `Etc/UTC` 时）：空值、`:`、`::x`、大小写变体、`posix/`、`right/`、`posix/right/`、绝对路径、相对路径、`..`、末尾斜杠、`JST-9`、`EST5EDT`、`GMT±n`、`Etc/GMT±n`、3–4 个字符的缩写、前后空白、垃圾值。
- `/etc/localtime` 的这些情形与 Node 一致：绝对符号链接、相对符号链接、`Etc/GMT-14`、zoneinfo 内文件的普通副本、不存在、悬空链接、空文件（以上均为 TZ 未设置）。
- 排序和时区代码里没有用 `as` 做浮点和整数之间的转换。`collate.rs:98` 的 `expect` 只在内置 root 数据缺失时才会触发，不依赖输入。
