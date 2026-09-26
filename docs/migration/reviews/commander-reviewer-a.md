# crates/commander — 对抗评审 A（第二轮复查）

结论：**通过，无阻塞项。** 第一轮的 5 项发现全部修复，并已用原来的探针逐项复现验证。

复查中新发现 1 个应修项和 2 个可选项：
- 应修：Windows 的终端宽度取值与 libuv 不一致。本机无法在 Windows 上运行，这一项是按源码比对确认的，下文注明了证据。
- 可选：畸形 locale 环境变量下的排序差异。这个问题属于 `raft_shared::js::default_locale`，commander 只是受它影响。
- 可选：累积型解析器被重复调用上万次时，常数因子较大。

Oracle 为 Node 24.15.0（`process.versions.uv` = 1.51.0）加上 upstream 锁定的 commander 12.1.0。探针位于 `scratchpad/cmdReviewA/`，构建时使用了 `crates/commander` 和 `crates/raft-shared` 的快照（`snap2/`，与仓库当前文件逐字节相同）。在快照上运行 `cargo test -p commander`，10 个测试全部通过，其中 golden 测试断言了全部 141 个用例。

## 复查：第一轮发现的处理情况

| 第一轮 | 状态 | 证据 |
|---|---|---|
| 1. [应修] suggestion 同分项按码元排序 | **已修复** | golden 的 141 例在探针中 0 差异（原来 #113/#115/#116 共 6 个字段不同）。`units` 探针中 5000 个随机 `suggest_similar` 调用只剩 5 处差异，全部是探针输入含孤立代理项 `"x\ud83d"`：Rust 的 `&str` 无法表示它，这是测试工具的限制，不是 crate 的问题。在真实环境变量下（不调用 `set_env`，走 `SystemEnv`），用 `env -i` 覆盖 24 种 locale 设置（未设置、`LANG=da_DK.UTF-8`、`LC_ALL=da_DK`、`LANG=da_DK LC_ALL=en_US`、`LC_ALL=`、`LANG=`、`C`、`POSIX`、`LC_MESSAGES=da_DK`、`LC_COLLATE=da_DK`、`LANGUAGE=da`、`de_DE@euro`、`sv_SE`、`tr_TR`、`nb_NO`、`ja_JP`、`xx_YY` 等），分别对 `qtx`、`xab`、`sx` 三个输入（候选为 `aab`、`zab`、`Stx`、`stx`、`s_x`、`s-x`）比较 Node 与 Rust 的 `unknown command` 建议，全部一致。这组用例确实能区分 locale：en 下为 `stx, Stx` 和 `aab, zab`，da 下为 `Stx, stx` 和 `zab, aab`。 |
| 2. [应修] 默认帮助宽度忽略 TTY | **已修复（unix）** | 原 `ttyprobe` 在 120 列 pty 下的输出与 Node 相同（首行 24 个 `word`）。`ttyprobe2` 在 pty 中测了 4 种重定向组合，Node 与 Rust 的 md5 全部相同：`--help 2>/dev/null`、`--help >file`（此时 stdout 不是 TTY，按 80 列）、组命令错误帮助 `2>file`、`>/dev/null`（此时 stderr 仍是 TTY，按终端列宽）。pty 列数取 0、1、30、41、42、200、65535 时两边的 md5 也全部相同；0 列时两边都回退到 80。Windows 的情况见新发现 1。 |
| 3. [可选] 父命令只持有 Weak | **已修复** | `probe2` 的输出：`Command::new("root").command("sub")` 得到 `Usage: root sub [options]`；root 在辅助函数内被 drop 后，`parent()` 仍返回 `Some("root")`，与 JS 相同。 |
| 4. [可选] 布尔选项上的解析器被跳过 | **已修复** | `probe2` 中，`--flag` 带解析器时得到 `{"flag":"parsed:None:None"}`，对应 JS 的 `parsed:undefined:undefined`。新增的 `fuzz3` 共 4000 例，程序里在布尔、取反、仅取反（`--no-solo`）、可选值（带默认值和不带）、组合短旗标（`-b`、`-c`、`-r <v>`）以及子命令同名选项上都挂了 trace/collect/int/invalid/commanderInvalid/抛普通 Error 的解析器；exitOverride 开和关都测了。stdout、stderr、calls 全部一致；outcome 只在"普通 Error"这一类上不同：Node 探针记录的是 `String(err)`（带 `Error: ` 前缀），Rust 探针记录的是 `to_string()`，差异来自两边探针的记录方式，与 crate 无关。 |
| 5. [可选] 组合短旗标呈二次复杂度 | **已修复** | `probe4`（release 构建）：N=10000 用时 8.7 ms，N=40000 用时 33 ms，N=130000 用时 111 ms，呈线性（第一轮分别为 0.2 s、2.95 s、31 s；Node 为 22 ms、92 ms、570 ms）。另外 `-` 后接 65000 个 `ab`、`-ab` 后接 130000 个字符、20000 个 `-a` 这几种输入，Rust 都比 Node 快。 |

## 已验证无差异的范围

以下各项都是 Node 与 Rust 的差分比较。

- **golden**：`tests/golden/commander/scenarios.json` 的 141 例全部一致。
- **真实 CLI 的帮助文本**：沿用第一轮从 oracle 导出的 raft（116 个命令）和 raft-computer（28 个命令）命令树。`help.json` 中的 help 和 parse 用例共 175 个，commander 层面的输出 0 差异。
- **模糊测试**：第一轮的两组共 16200 例全部重跑，0 差异，无 panic。新增 `fuzz3` 4000 例覆盖新的解析器签名，结论见上表。
- **线性化后的组合短旗标**：新增 204 例边界输入，放在 `send #g X …`、`send X …`、`X send #g …` 三种位置，结尾分别接空、`#g`、`-- z`、`-x`，0 差异。X 包括：`-a-`、`-a--`、`-ab-`、`-a---`、`-a-b`、`-a-h`、`-ah`、`-a😀`、`-😀a`、`-aé`、`-ab--content`、`-ac-`、`-a-c1`、`-a=`、`-a-=x`。这组用例检验了"放回的剩余部分恰好是 `--`"时是否仍按字面量 `--` 处理，以及代理对不会被切开。
- **`Help.wrap`**：6000 个随机输入，0 差异。
- **Option 和 Argument 定义**：30 种 flags 写法和 11 种参数写法，0 差异。
- **Rc 环**：父指针改为强引用后，`Debug` 只打印名字，没有派生 `PartialEq` 或 `Drop`，不存在沿环递归的路径。环本身在进程生命周期内泄漏，review note 已写明。golden 的 141 个程序加上 fuzz 的 2 万多个程序都建在同一个进程里，内存和时间都没有异常。
- **RefCell 重入**：我重新检查了新代码（`find_option_units`、`put_back`、`env()`/`set_env`），用户回调（解析器、写函数、宽度函数、钩子）被调用时都没有持有借用。

## 发现

### 1. [应修] Windows 上的帮助宽度取的是窗口宽度，而 Node（libuv）取的是屏幕缓冲区宽度
- 位置：`terminal.rs` `console_columns`，计算的是 `srWindow.Right - srWindow.Left + 1`。decisions.md D26 也写成了"window width"。
- 源码证据：Node 24.15.0 捆绑的 libuv 版本是 1.51.0（`process.versions.uv`）。在 libuv v1.51.0 的 `src/win/tty.c` 中：
  - `uv_tty_get_winsize` 调用 `uv__tty_update_virtual_window(&info)`，然后返回 `*width = uv_tty_virtual_width`。
  - `uv__tty_update_virtual_window` 里有一行 `uv_tty_virtual_width = info->dwSize.X;`，即屏幕缓冲区宽度。
  - 只有高度用的是 `srWindow.Bottom - srWindow.Top + 1`。
- 具体输入：在经典 conhost 中把"屏幕缓冲区大小 → 宽度"设为 200，把窗口宽度设为 120，然后运行 `raft-computer --help`。Node 的 `process.stdout.columns` 是 200，按 200 列折行；Rust 按 120 列折行。在 Windows Terminal 和默认 conhost 中，缓冲区宽度通常等于窗口宽度，所以多数情况下看不出差别。
- 这一项**没有在 Windows 上实际运行复现**（本机是 Linux），结论来自上面的 libuv 源码比对。isTTY 的判定是一致的：libuv 的 `uv_guess_handle` 用 `GetFileType == FILE_TYPE_CHAR` 加 `GetConsoleMode`，Rust 只用 `GetConsoleMode`，对 NUL 设备等输入结果相同。
- 建议：改为返回 `info.dwSize.X`（小于等于 0 时返回 `None`），同时修正 D26 的表述；如有条件，在 Windows CI 上补一个设置了缓冲区宽度的用例。

### 2. [可选] 畸形 locale 值的解析与 ICU 不一致（`raft_shared::js::default_locale`，commander 受影响）
- 位置：`crates/raft-shared/src/js/collate.rs` `default_locale`，以及 `Env` 读取非 UTF-8 值的方式。这属于 D24 和 js-core 单元的范围，commander 只是通过 `SystemEnv` 间接受影响。
- 输入：使用 `locprobe` 程序（候选为 `aab`、`zab`、`Stx`、`stx`、`s_x`、`s-x`），argv 为 `qtx`，用 `env -i LANG=<v>` 运行。

  | `LANG` | Node | Rust |
  |---|---|---|
  | `da__DK` | `Stx, stx`（按 da 排序） | `stx, Stx`（Rust 解析失败后落到 `und`） |
  | `da_DK` 后接字节 0xFF（非 UTF-8） | `Stx, stx` | `stx, Stx` |
  | `da_DK.UTF-8@\xff`（字面反斜杠） | `Stx, stx` | `stx, Stx` |

  `../../etc`、`a b`、`@`、`_`、`.`、`i-default`、`x-private`、`en_US@collation=phonebook`、`root`、`und` 这些值两边一致。
- 影响：只有在 locale 值畸形、并且出现同分建议时才会显现。真实 CLI 的候选集在 en 和 root 下不会产生排序分歧（第一轮已检查 4826 对）。
- 建议：转给 raft-shared 或 D24 的负责人。在 `Locale::try_from_str` 失败时，按 ICU `uloc_forLanguageTag`/POSIX 规范化的容错方式，只取第一个可用的子标签（如 `da`）；非 UTF-8 的值按 `var_os` 做有损转换后再处理，不要当成未设置。

### 3. [可选] 累积型解析器被重复调用上万次时，比 Node 慢约 6 倍
- 输入：`basic` 程序，argv 为 `send #g` 后接 20000 个 `--tag=x`（`collect` 解析器）。Node 用 1.78 s，Rust（release）用 10.3 s。
- 原因：两边的算法都是二次的，因为 `[...previous, v]` 本身就要复制数组。但 Rust 每次调用 `get_option_value` 时都会深拷贝整个 `Value::Array` 作为 `previous`，`emit_option` 又要克隆一次 `val`，所以常数因子更大。
- 影响：可以忽略。真实参数个数受 `ARG_MAX` 限制，重复 1000 次的耗时差异在毫秒级。这里只作记录。
- 建议（可选）：在调用解析器之前，把旧值从 `option_values` 中 `take` 出来，而不是克隆（解析器失败时再放回），这样 `previous` 就是按移动传递。

## 严重度计数
阻塞 0，应修 1，可选 2。
