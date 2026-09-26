# crates/commander — 对抗评审 A

结论：**没有阻塞项，但当前不通过 golden 门禁**。共有 2 个应修项和 3 个可选项。第 1 项使扩展后 golden 中的 3 个 `ties` 用例（6 个字段）失败。第 2 项是 D1 明确列入输出契约的 TTY 宽度，crate 的默认实现没有做到。

所有发现都用具体输入复现过。Oracle 是 Node 24.15.0 加上 upstream 锁定的 commander 12.1.0。探针位于 `scratchpad/cmdReviewA/`。评审期间 `crates/raft-shared` 正被其他单元改动（`json_schema/` 缺子模块，无法编译），所以探针构建用的是两个 crate 的快照（`snap/`）。快照里的 commander 与仓库版本 `diff -r` 相同，raft-shared 只删掉了 `pub mod json_schema;` 这一行。

## 已验证无差异的范围

- **golden（141 例，含新增的 `extras`/`ties`）**
  - 我把 `golden_tests.rs` 的构建逻辑搬到探针里，补上了 `commanderInvalid` 解析器和 `action: true`，逐字段比较 stdout、stderr、calls、outcome、result。
  - 138 例完全一致。只有 #113 `ti sta`、#115 `ti st`、#116 `ti stx` 在建议排序上不同（见发现 1）。
  - 以下新增场景全部一致：`--color`/`--no-color` 的各种先后组合、有自身 action 的父命令（`ex`、`ex start --no-color`、`ex zzz`）、`--opt=`、`--opt= start`、解析器抛出 `CommanderError(1,"commander.invalidArgument")`，以及走默认退出路径的 `extras`。
  - 同一套 Node 复刻脚本的输出与 golden 文件 0 差异，说明比较方法本身可靠。
- **真实 CLI 的帮助文本**
  - 我在 oracle 进程里加载 `cli/src/main.ts`（拦截 `parseAsync`）和 `computer/src/cli.ts`，导出完整命令树：
    - raft：116 个命令，10 段 `addHelpText("after")`，8 个隐藏选项，带 preAction 钩子。
    - raft-computer：28 个命令，7 个隐藏命令，两处 `--no-start`，`--lines` 带解析器。
  - 用 crate 按同一棵树重建后：
    - raft 的 116 条帮助输出中 115 条与 `tests/golden/raft-cli/help.json` 逐字节一致。唯一差异是根 `--help` 里 `-V` 的位置。原因在探针脚本：`apply_common` 先调用 `version()`，而 main.ts 先注册 `-p`。用相同构造顺序的 Node 输出与 Rust 一致，所以不算 crate 的问题。
    - raft-computer 的 21 条帮助输出全部一致。
  - 两份文件中 commander 负责的 parse 用例输出也一致：stdout、`raft help` 系列的 `commander.help`/exitCode 0、组命令把帮助打到 stderr 并退出 1、`help help`、`help nosuch`。剩余 stderr 差异全部来自 raft 自己的渲染器或 action，不在本 crate 范围内。
- **差分模糊测试（16200 个 argv）**
  - 覆盖所有 golden 程序，以及真实 raft/raft-computer 命令树；种子 2 个；exitOverride 开和关都测了；随机设置 `process.exitCode` 和 out/err 宽度。
  - token 包括：`--`、`-`、`""`、组合短旗标（`-abz`、`-Vv`、`-hV`、`-y5`）、`--x=`、`--no-start=0`、`-p=x`、`-😀x`、`--é=1`、`\u0000`、`help help` 等。
  - stdout、stderr、action 参数、`opts`、`optsWithGlobals`（包括键序）和 outcome 全部一致，没有任何 panic。
  - 这同时覆盖了：父命令在子命令之后消费自己的选项、`--` 之后不再消费、未知选项之后的参数全部归入 unknown、`-h` 在 unknown 中触发帮助、version 在组合短旗标中触发、mandatory 选项、解析器的 `previous` 语义（`trace` 解析器能区分 None、默认值和上一次的值）。
- **`Help.wrap`**：6000 个随机输入，0 差异。输入包含代理对 emoji、首个 `\r\n`、`​`、NBSP、` `、`\v\f`、U+0085、手工缩进，`minColumnWidth` 取 0/10/40，宽度取 20–120。宽度设为 65535 和 1e6 时既不 panic，也不慢。
- **Option 和 Argument 定义**：30 种 flags 写法和 11 种参数写法（如 `-sw, --short-word`、`--foo <a> <b>`、`--no-foo`、`-f|--foo`、`--foo--bar`、`<...>`、`[a...]`）在 long、short、required、optional、negate、attributeName 以及抛错点上全部一致。
- **RefCell 重入**：我逐一检查了所有用户回调的调用点：解析器、writeOut/writeErr、宽度函数、outputError、钩子、action。调用前 `Ref`/`RefMut` 都已释放，回调里再调用 `configure_output`、`opts` 或 `command` 不会触发 borrow panic。

## 发现

### 1. [应修] `suggest_similar` 的同距离候选用 UTF-16 码元排序，而源码用 `localeCompare`（ICU root）
- 位置：`suggest_similar.rs:103` `sort_default(&mut similar)`，源码是 `suggestSimilar.js:83`。
- golden 输入：`ties` 程序，argv 为 `["sta"]`。
  - Node：`error: unknown command 'sta'\n(Did you mean one of st_x, st-x, start, state, stats?)`
  - Rust：`… one of st-x, st_x, start, state, stats?)`
  - `["st"]` 和 `["stx"]` 同理：Node 为 `st_x, st-x`，Rust 为 `st-x, st_x`。stderr 和 outcome.message 两个字段都不一致，所以 golden 现在失败 6 个字段。
- 另外两个直接调用的例子（5000 个随机用例中有 30 个排序不同）：
  - `suggest_similar("--foo", ["--fo_o","ete","--foo2_","--Foo","a.b","a-bx"])`：Node 为 `--fo_o, --Foo`，Rust 为 `--Foo, --fo_o`。
  - `suggest_similar("s", ["STATE","Stage","a1","sts","ss","és","--foo2s"])`：Node 为 `és, ss`，Rust 为 `ss, és`。
- 真实 CLI 不受影响。我对两个 CLI 每一层可见的命令名集合和选项名集合（含祖先选项和 `help`）做了两两比较，4826 对中码元序与 `localeCompare` 序 0 分歧。
- 建议：
  - 在 `raft_shared::js` 中实现 `locale_compare`（`icu_collator =2.3.1` 已是 raft-shared 的锁定依赖），用 root locale、tertiary 强度，与 V8 的默认 `Intl.Collator` 一致。
  - 然后替换这里的排序。之后把 golden 断言的用例数改为 141。

### 2. [应修] 默认的 `get_out_help_width`/`get_err_help_width` 总是返回 `undefined`，终端里帮助固定按 80 列折行
- 位置：`command.rs:128-160` `OutputConfiguration::default`。源码 `command.js:63-66` 在流是 TTY 时返回 `process.stdout.columns`/`process.stderr.columns`。
- 输入：在 120 列的 pty 中运行 `new Command('t').description('word '.repeat(40)).option('--x','y')`，然后执行 `--help`（`scratchpad/cmdReviewA/ptyrun.py` 会先设置 TIOCSWINSZ 再 exec）。
  - Node：描述第一行有 24 个 `word`（120 列），然后折行。
  - Rust（`ttyprobe`，默认配置）：每行 16 个 `word`（80 列），共三行。
- 影响：
  - D1 的 Facts 把"流是 TTY 时取其列宽"列为 `raft` 的输出契约。
  - raft-computer 完全没有调用 `configureOutput`。raft 只替换了 `outputError`，宽度仍用默认值。所以两个二进制在交互终端里的 `--help`、组命令错误帮助，以及 `help <cmd>` 的输出都会与 oracle 不同。
  - 现有 golden 都是非 TTY 采集的，发现不了这个问题。
- crate 里的 review note 把这件事推给了二进制，但 decisions.md 和 mapping-guide 里都没有对应的决定或依赖，raft-cli 和 raft-computer 的单元也没有这项任务。
- 建议（二选一，并记入 D1）：
  - 在 crate 里实现默认值：`std::io::IsTerminal` 判断是否为终端，列宽用 ioctl `TIOCGWINSZ`（Windows 用 `GetConsoleScreenBufferInfo`）。这需要按 mapping-guide §13 锁定一个依赖，例如 `rustix` 或 `terminal_size`。
  - 或者在 D1 中明确由两个二进制通过 `configure_output` 注入宽度，并把"pty 120 列下 `--help`"加入两个二进制的验收用例。

### 3. [可选] 父命令只用 `Weak` 持有，根句柄被 drop 之后，子命令的祖先链会静默消失
- 位置：`command.rs` 中的 `CommandState.parent: Weak<…>`（第 419 行赋值，`parent()` 在 1414 行）。
- 输入：
  - `Command::new("root").command("sub").help_information(Default::default())`
    - Rust：`Usage: sub [options]`
    - JS（`new Command("root").command("sub").helpInformation()`）：`Usage: root sub [options]`
  - 一个辅助函数先给 root 注册 `-p, --profile <slug>`，再返回 `root.command("sub")`，root 随函数返回被 drop：
    - Rust 的 `parent()` 返回 `None`，帮助行变成 `Usage: sub [options]`。之后 `opts_with_globals()` 会丢掉全局 `profile`，钩子也不再包含 root 的 preAction。
    - JS 由 GC 保住父对象，结果依然是 `root sub`。
- 影响：
  - 模块文档说 `Command` 是"与 JS 对象引用一样的共享句柄"，但这里的行为不同。
  - 目前 main.ts、cli.ts 和移植过来的测试（例如 `knowledge/get.test.ts:60-75`）都在作用域里一直持有 program，所以暂时不会触发。
  - 将来如果把注册代码拆进返回子命令的辅助函数，就会出现静默错误。
- 建议：
  - 改为强引用 `Rc` 父指针。形成的环在 CLI 进程生命周期内可以接受，需要写一条 review note。
  - 或者在 `command()` 的文档里写明必须持有根句柄，并在 `parent()` 升级失败时 `debug`/`panic`。

### 4. [可选] 布尔选项上的自定义解析器被跳过，源码会以 `(undefined, previous)` 调用它
- 位置：`command.rs:671-690` `handle_option_value`，只在 `Value::String` 时调用解析器。源码是 `command.js:667`（`val !== null && option.parseArg`）。
- 输入：`addOption(new Option("--flag","f").argParser((v,prev)=>\`parsed:${v}:${prev}\`))`，argv 为 `["--flag"]`，`from: "user"`。
  - Node：`{"flag":"parsed:undefined:undefined"}`
  - Rust：`{"flag":true}`
- 影响：两个 CLI 都没有在布尔或取反选项上挂解析器（我在导出的命令树中确认过），crate 的 review note 也写明了这一点。但 API 允许这样写，而且会静默给出不同结果。
- 建议二选一：
  - 把 `ParseArgFn` 的值参数改为 `Option<&str>`，按源码调用。
  - 或者在 `add_option` 时对"布尔/取反 + parse_arg"直接 panic，写明未移植，避免静默分歧。

### 5. [可选] 超长组合短旗标的解析比 Node 慢约 55 倍（单个 argv 就能让进程卡住半分钟）
- 位置：`command.rs:809-826`。每剥离一个字符就会调用一次 `JsString::from(arg)`、`js_slice`、`format!`，以及 `maybe_option`/`LONG_WITH_VALUE.test` 的整串转换，每步都是 O(N) 的 UTF-16 重建。
- 输入：程序声明了 `-a`（布尔）和 action，argv 为 `["-" + "a".repeat(N)]`，`from: "user"`。Rust 用 release 构建。

  | N | Node | Rust |
  |---|---|---|
  | 10000 | 22 ms | 0.20 s |
  | 40000 | 92 ms | 2.95 s |
  | 130000 | 570 ms | 31.1 s |

  130000 仍小于 Linux 的 `MAX_ARG_STRLEN`。
- 影响：输出没有分歧，但一个用户或 agent 传入的参数就能让 `raft` 或 `raft-computer` 挂起约 30 秒。两者都是二次复杂度，Node 用 sliced/cons string 把常数压得很低。
- 建议：在循环外把 arg 转成一次 `JsString`，剩余部分以 `JsString`（或"原串 + 偏移量"）的形式放回队列，`maybe_option` 和正则判断直接作用在 `JsString` 上，只在 emit 值时才转成 `String`。

## 严重度计数
阻塞 0，应修 2，可选 3。
