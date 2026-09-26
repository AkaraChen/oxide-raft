# crates/commander：对抗审查 B（逐行翻译保真、D1 API、guide 规则、ICU 排序）

**结论：不通过。** 共 2 个阻塞项、1 个应修项、3 个可选项。阻塞项是：建议列表的并列排序用的是 UTF-16 code unit 序，而不是 ICU `localeCompare`，新增的 3 条 `ties` golden 因此失败；默认输出配置在 TTY 上不读终端列宽，所有交互式 `--help` 都按 80 列折行，而 D1 明确把"TTY 列宽"写进了输出契约。

审查范围：`crates/commander/src/*.rs`，逐函数对照 `commander@12.1.0/lib/{command,option,argument,help,suggestSimilar,error}.js`。

测试环境：
- 仓库里的 `raft-shared` 目前编译不过（`json_schema/mod.rs` 声明的 `compile`/`safe_regex`/`validate` 等文件还不存在，另一个单元正在写）。所以我把 `commander` 和 `raft-shared/src/js` 原样复制到 scratch 工作区（`scratchpad/cmdReviewB/snap`），去掉了 `json_schema`/`schema` 两个模块，所有探针都跑在这份快照上。复制后 `diff -r` 确认 commander 与仓库一致。
- 在快照上跑 `cargo test -p commander`：3 个测试通过，`commander_golden_scenarios` 在数量断言处失败（141 ≠ 122）。clippy 在 commander 上 0 条警告，`cargo fmt --check` 无 diff。
- oracle 用 Node 24.15.0（ICU 78.2），commander 从 `upstream/raft-source/packages/cli/node_modules/commander` 加载。

---

## 阻塞

### B1. 建议列表的并列项按 code unit 排序，不是 ICU `localeCompare`，3 条新 golden 失败
- 位置：`suggest_similar.rs:409` 调用了 `sort_default(&mut similar)`，模块注释在 `:354-359`。源码是 `suggestSimilar.js:86` 的 `similar.sort((a, b) => a.localeCompare(b))`。
- 违反的规则：mapping-guide §6.9 规定 "`a.localeCompare(b)` → `js::locale_compare(a, b)`, never `str::cmp`"；D3 规定 `locale_compare` 用 ICU4X collator，locale 按 Node 的方式从环境解析。
- 探针结果：
  - 用新的 141 条 golden 跑（scratch harness 已补上 S1 缺的两项），138 条通过，失败的 3 条全是这个问题：
    - `ties ["sta"]`：期望 `st_x, st-x, start, state, stats`，实际输出 `st-x, st_x, …`
    - `ties ["st"]`：期望 `st_x, st-x`，实际相反
    - `ties ["stx"]`：期望 `st_x, st-x`，实际相反
  - 用 9000 条随机 argv 做差分（6 个程序，包括 `ties`），170 条不一致，全部是这一种排序差异，没有别的差异。
- 模块注释里的说法站不住。注释说"两种顺序在只含小写 ASCII 字母和数字的名字上一致"，这只在 root/en 下成立：
  - `LANG=da_DK.UTF-8`：同一个程序注册 `aab`、`zab` 两个命令，输入 `xab`，Node 输出 `(Did you mean one of zab, aab?)`；en-US 下是 `aab, zab`。
  - `lt_LT` 下 `y` 排在 `i` 后面、`s` 前面。
  - `da` 的 `caseFirst` 是 `"upper"`，所以 `Stage < stage`；en 下是 `stage < Stage`。
- 要让结果和 Node 一致，`locale_compare` 需要满足以下条件（全部经 Node 24.15.0 实测）：
  1. 语义是不带参数的 `localeCompare`，等价于 `new Intl.Collator(undefined)`。`resolvedOptions()` 为 `usage:"sort"`、`sensitivity:"variant"`（tertiary 强度）、`ignorePunctuation:false`（alternate = non-ignorable，所以标点有 primary 权重，不会被忽略）、`numeric:false`、`caseFirst` 取 locale 的默认值（en/root 是 `"false"`，da 是 `"upper"`）。
  2. locale 按 ICU 的默认规则取：`LC_ALL` 优先，其次 `LC_MESSAGES`，再次 `LANG`。三个都没有时是 `und`（root）；值为 `C`/`POSIX` 时是 `en-US`。`LC_COLLATE` 和 `LANGUAGE` 不起作用（实测：`LANG=en_US LC_COLLATE=da_DK` 仍是 en-US；`LANG=en_US LC_MESSAGES=da_DK` 是 da-DK）。
  3. root/en 下相关字符的次序：`_` < `-` < 数字 < 字母（不分大小写，同一字母小写在前）。实测 `st_x < st-x < st1 < stA < Stage < start < state`。code unit 序在三处与此不同：`-`/`_` 的先后、`_` 与数字和大写字母的先后、大小写交错。
  4. 比较的对象是 `--` 已经剥掉的名字（`suggestSimilar.js:62-65`），排好之后再加回 `--`。排序必须稳定（V8 用的是 TimSort）。候选已经去过重，比较结果为 0 的情况只会出现在 ICU 判定为等价的字符串上。
  5. 数据版本：工作区已经 pin 了 `icu_collator = "=2.3.1"`（`Cargo.toml:29`），但 `raft_shared::js` 里还没有 `locale_compare`（grep 无结果）。这个修复要等它落地。ASCII 名字在 CLDR root 各版本间的次序是稳定的。
- golden 本身也依赖环境：`tools/golden/commander.mjs` 没有记录生成时的 locale，在 `da_DK` 下重新生成会得到不同的 `ties` 记录。
- 修复建议：
  1. 改成 `similar.sort_by(|a, b| js::locale_compare(a, b))`，删掉 `:354-359` 的注释。
  2. 生成脚本固定 `LC_ALL=C`（或者把 `Intl.Collator().resolvedOptions().locale` 写进 JSON）。Rust golden 测试用同一个 locale 调用：`locale_compare` 需要提供可注入 locale 的形式，或者测试进程设置相同的环境变量。
  3. 在 `suggest_similar_edge_cases` 里加一个 `da` locale 的用例：`aab`/`zab` 期望 `zab, aab`。

### B2. 默认输出配置不读 TTY 列宽，交互式 `--help` 一律按 80 列折行
- 位置：`command.rs:126-153`（`OutputConfiguration::default`，`get_out_help_width`/`get_err_help_width` 恒为 `|| None`），review note 在 `:129-134`。源码是 `command.js:63-66`：`process.stdout.isTTY ? process.stdout.columns : undefined`，stderr 同理。
- 违反的规则：D1 Facts 第一条把帮助的折行宽度写进了 `raft` 的输出契约："the columns of the stream the help goes to … when that stream is a TTY; otherwise 80"。D1 Decision 要求 crate 翻译 `configureOutput`，但并没有允许把 TTY 探测下放给各个二进制。
- 探针结果（pty 由 `script -qc "stty cols N; …"` 分配；程序是一段 130 字符的描述，加一个长描述的 `--server-url <url>`）：
  - `cols 140`：Node 的描述和选项都在一行内，没有折行；Rust 在第 72 列和第 76 列折行，和 80 列时完全一样。
  - `cols 60`：Node 在 60 列内折成三行；Rust 仍然按 80 列输出。
- 后果：
  - `raft-computer` 的源码根本不调用 `configureOutput`，`raft` 也只替换了 `outputError`。按 guide §11 "declare … in the same order as the source" 翻译时，两个 `main` 都不会去设置宽度，所以两套 CLI 在终端里的全部帮助输出（`--help`、`help x`、裸命令组打到 stderr 的帮助）都会和 Node 不一样。
  - 116 条帮助 golden 都是在非 TTY 下录制的，这个问题测不出来。
- 修复建议：
  1. 默认闭包每次调用时现查，对应 Node 在 SIGWINCH 后更新 `columns` 的行为：unix 上 `isatty(fd)` 为真时读 `ioctl(fd, TIOCGWINSZ)` 的 `ws_col`；Windows 上用 `GetConsoleScreenBufferInfo`，取 `srWindow.Right - srWindow.Left + 1`（libuv `uv_tty_get_winsize` 的算法）。不是 TTY 时返回 `None`。
  2. `libc` 和 `windows-sys` 已在 guide §13 的允许清单里，但还没有 pin。按 `Cargo.toml` 的注释要求补 pin，并在 decisions 里记一笔。
  3. 加一个 pty 测试（unix），或者把"读列宽"抽成一个可注入的函数，单独测它和折行宽度之间的衔接。

---

## 应修

### S1. golden 测试跑不了新增的两种 spec 字段，扩到 141 条之后除了改数量还要改 harness
- 位置：`golden_tests.rs:54-86`（`parser()` 只有 4 个分支）、`:171-173`（只给叶子命令挂 action）、`:405`（数量断言写死 122）。
- 生成脚本新增的内容：
  - `commanderInvalid` 解析器：值为 `"bad"` 时抛 `new CommanderError(1, "commander.invalidArgument", "Bad value.")`。
  - `action: true`："true also gives a command with subcommands its own action"。
- 现状：
  - 构建 `extras` 程序时会 panic，报 `unknown parser commanderInvalid`。
  - 即使补上这个分支，`extras` 根命令也拿不到 action。结果是 `extras []` 输出帮助而不是 action 调用，`extras zzz` 报 unknown command，`--opt=` 这几条也都会错。
- 实现本身没问题：在 scratch 里补上这两处（`commanderInvalid` 返回 `Box<CommanderError>`；`(leaf && !action_disabled) || action == true`）之后，19 条新记录有 16 条逐字节一致，剩下 3 条就是 B1。这 16 条覆盖了：`--color`/`--no-color` 的双向覆盖和默认值（没有默认值时 `opts` 为 `{}`）、`--opt=` 取空串、父命令在子命令之后消费 `--no-color`、根命令同时有 action 和子命令时不生成 help 子命令、`CommanderError` 形式的 invalidArgument 走 `error()` 路径。
- 修复建议：按上面两处修改 harness，数量改为 141，再修 B1。

---

## 可选

### O1. `parent` 是 `Weak`，丢掉 program 句柄后子命令就成了孤儿，usage、全局选项和必填检查都会变
- 位置：`command.rs:258`（`parent: Weak<…>`）、`:414`、`:1422-1425`。
- 探针结果：`fn build() -> Command { let p = Command::new("raft"); p.option("-p, --profile <slug>", …); p.command("agent") }` 返回子命令、丢掉 `p` 之后：
  - `agent.help_information()` 输出 `Usage: agent [options]`，而 Node 同样的写法输出 `Usage: raft agent [options]`。
  - `parent()` 返回 `None`。`opts_with_globals()` 丢失父命令的选项，`check_for_missing_mandatory_options` 也不再向上检查。
- 在 JS 里子命令通过 `cmd.parent` 让父命令一直存活。上游测试和 CLI 里目前没有"只留子命令句柄"的写法（多行 grep `new Command(…).command/.option/.name` 无结果），所以只是一个 API 陷阱。
- 修复建议：二选一。一是改为强引用 `Rc`：program 活到进程结束，环引用造成的泄漏无害。二是在 `Command` 的文档里写明"必须保留根句柄"。

### O2. 布尔选项上的自定义解析器被静默跳过
- 位置：`command.rs:656-683`（review note 已说明）。
- 探针结果：`--flag` 挂一个解析器 `(v, prev) => "P:"+v+":"+prev`。Node 得到 `{"flag":"P:undefined:undefined"}`，Rust 得到 `{"flag":true}`。
- 可达性：两套 CLI 的 15 处 `parse:` 和 `--lines` 都挂在 `<…>` 选项上，目前碰不到。但 D1 的原文是 "custom option parsers called as `(value, previous)`"，这里静默偏离，将来翻译出错也不会被发现。
- 修复建议：在 `add_option` 时遇到"布尔或取反选项加解析器"直接 panic，属于定义期错误，调用方会立即发现。或者把解析器签名改成 `Option<&str>`。

### O3. 每个 doc 注释都带源码行号，违反 guide §1.3 "No other provenance comments"
- 位置：`command.rs` 49 处、`help.rs` 5 处、`option.rs` 4 处、`argument.rs` 3 处、`suggest_similar.rs` 2 处，形如 `(command.js:143-168)`。
- 修复建议：删掉。如果认为第三方库的翻译需要逐函数定位，就在 D1 里写一条豁免。

---

## 已确认没有问题（附探针）
- **逐函数对照源码没有发现遗漏或乱序**：
  - 已逐个对照的函数：`_parseCommand`（dispatch → help 子命令 → 无参时显示帮助 → `_outputHelpIfRequested(parsed.unknown)` → mandatory → unknown option → `_processArguments`，与源码顺序相同；未移植的 default command、`command:*` 和 legacy 分支在两套 CLI 中都不可达）、`parseOptions`（`--`、组合短选项、`--x=v`、`dest` 切换；`activeVariadicOption` 已随可变参数选项一起删除，两套 CLI 都没有 `<v...>` 形式的选项）、`_dispatchSubcommand`/`_dispatchHelpCommand`（包括 `help ""` 和 `help nosuch` 回退到父命令 stderr 帮助的路径）、`_checkNumberOfArguments`（12.1.0 的 `_allowExcessArguments` 默认为 true）、`_processArguments`、`unknownOption`/`unknownCommand`、`error`（`exitCode || 1`、`code || 'commander.error'`）、`_exit`、`help`（`process.exitCode || 0`，错误帮助时强制为 1）、`outputHelp` 和 after 文本、`version`、`_getHelpCommand`/`_getHelpOption` 的惰性创建与继承、`copyInheritedSettings`（输出配置对象共享，不是复制）、`addOption` 的取反默认值，以及 `handleOptionValue` 里 `previous` 为 `None` 的条件。
  - help.js：`formatHelp`、`padWidth`、`visibleOptions` 的 help 选项冲突隐藏、`subcommandTerm`、`commandUsage`、`optionDescription`、`wrap`。
  - suggestSimilar：OSA 距离、相似度大于 0.4、单字符候选跳过、去重。
- **差分随机测试**：
  - argv：9000 条，覆盖 6 个程序，包括 25% 的非 exitOverride 路径、20% 的随机宽度和 10% 的 `process.exitCode`。除 B1 外全部一致。
  - `Help.wrap`：2 万条随机字符串，字符包括 `\t \v \f \r\n \r \n NBSP ZWSP U+2028 U+3000 U+FEFF U+0085 😀 é`，宽度、缩进和 `minColumnWidth` 也都随机。
  - 完整 `helpInformation`：2000 条。
  - 以上 0 处差异。宽度设到 70000 时正则的量词上限没有出问题。
- **重入**：preAction hook 里调用 `program.opts()`、`this.opts_with_globals()`、`configure_output()`、`action.output_help()`；解析器里调用本命令的 `opts()`/`help_information()` 和父命令的 `opts()`；action 里调用 `output_help()` 和 `parse_options()`。没有出现 RefCell borrow panic，各处都是先克隆出闭包、释放借用之后才调用回调。
- **guide 规则**：
  - crate 内没有 `process::exit` 和 `std::process`。
  - 没有浮点和整数之间的 `as` 转换，宽度转换走 `usize_to_f64`/`i64_to_f64`/`try_from`。
  - 所有正则都通过 `JsRegex` 构造，模式文本与源码逐字相同：`([^ ]+) *(.*)`、` +`、`^--[^=]+=`、`^--no-`、`^--`、`^-`、`^no-`、`[ |,]+`、`^[[<]`、`^-[^-]$`、`^`(gm)、manualIndent、wrap 正则。其中 wrap 正则里的 `\n` 是真实换行符，与 JS 模板字面量一致。
  - `\r\n` 用的是 `string_replace_first`。
  - 所有 `panic!`/`expect` 都在定义期触发，对应源码在同一位置 throw 的情况（冲突的 flag 或命令名、非末尾的可变参数、空命令名、无法 camelcase 的 flag）。用户输入不会走到 panic：`=` 的位置由前面的正则匹配保证；组合短选项遇到代理项时直接判定为"没有这个选项"。
- **D1 API 清单**：
  - 已具备：`commands()`、`name()`、`alias()`（返回 `None`）、`create_help().visible_commands()`、`opts`/`opts_with_globals`、`options()` 以及 `Option` 的 `long`/`short`/`flags`（均为 pub）、`registered_arguments()` 以及 `Argument::name()`（对应 `_args`）、`Option::new(...).hide_help()` 加 `add_option`、`output_help`、`help_information`、`exit_override`、`configure_output`（`out`/`err` 两个宽度分开设置）、`hook(PreAction)`、`add_help_text(After)`、`version`、`CommanderError { exit_code, code, message }`、`ParseFrom::{Node, User}`、`InvalidArgumentError`。
  - 核对过的调用点：`cli/src/main.ts`、`core/command.ts`、`computer/src/cli.ts`，以及 `agent/login.test.ts:212-263`、`bridge.test.ts:56-60`、`app.test.ts:318`、`cliServerArgContract.test.ts:50,92`、`cliHelpCopy.test.ts`、`knowledge/get.test.ts:69`、`command.test.ts`。
  - D1 列为不移植的项确实都没有移植。
