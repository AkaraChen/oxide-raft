# crates/commander：对抗审查 B（第二轮复查）

**结论：有条件通过。** 没有阻塞项；有 2 个应修项和 1 个可选项。第一轮的 2 个阻塞项、1 个应修项、3 个可选项全部已修复，证据见下文"复查"一节。

本轮发现的问题都集中在 Windows 和环境变量上，linux 上都复现不了：
- S1：Windows 下终端列宽取的是窗口宽度，而 libuv 取的是屏幕缓冲区宽度 `dwSize.X`。
- S2：Windows 下默认 locale 按 POSIX 环境变量解析，而 ICU 在 Windows 上读的是系统用户 locale。S2 的实现归 `raft-shared`/D24，但它直接决定 commander 的建议排序，所以列在这里。
- O1：`SystemEnv` 每次比较都实时读取环境变量，而 Node 的默认 locale 在进程启动时就固定了。

审查范围：
- `crates/commander/src/*.rs` 的所有改动，包括新增的 `terminal.rs`。
- 与这些改动直接相关的 `raft_shared::js::collate`（`locale_compare_js`、`default_locale`、`SystemEnv`）。
- decisions D24 和 D26。

测试环境：
- 仓库状态会被其他单元临时改动，所以我在 `scratchpad/cmdReviewB/snap` 重新做了一份快照（`commander` 和完整的 `raft-shared`，这一轮能编译通过）。
- 在快照上跑 `cargo test -p commander`：10 个测试全部通过，其中包括 141 条 golden 和 unix pty 测试。
- clippy 在 commander 上 0 条警告，`cargo fmt --check` 无 diff。
- oracle 用 Node 24.15.0（ICU 78.2，libuv 1.51.0）。

---

## 复查（第一轮各项）

| 项 | 状态 | 证据 |
|---|---|---|
| B1 并列建议按 code unit 排序 | **已修复** | 见下方说明 1 |
| B2 默认输出配置不读 TTY 列宽 | **已修复（unix）**，Windows 见本轮 S1 | 见下方说明 2 |
| S1 golden harness 不支持新字段 | **已修复** | 见下方说明 3 |
| O1 `parent` 是 `Weak` | **已修复** | 见下方说明 4 |
| O2 布尔选项上的解析器被跳过 | **已修复** | 见下方说明 5 |
| O3 源码行号注释 | **已修复** | 在 `crates/commander/src` 里 grep `\w+\.js:\d` 为 0 处 |

1. **B1 并列建议排序**
   - 实现：`suggest_similar.rs` 改用 `similar.sort_by(|a, b| locale_compare_js(a, b, env))`，`env` 取自程序状态里的 `Command::env()`。
   - golden：141 条全部通过，包括 `ties sta/st/stx`。golden 测试用空 `BTreeMap` 作为 Env，对应 en-US。
   - 端到端对照：用默认的 `SystemEnv` 构建二进制，与 Node 在 19 种环境下逐字节比对，候选集合有 8 组，**0 处差异**。
     - 19 种环境：未设置、`C`、`POSIX`、`en_US`、`da_DK`、`lt_LT`、`sv_SE`、`tr_TR`、`de_DE@euro`、`LANG=da LC_ALL=`（空值得到 root）、`LC_MESSAGES` 覆盖、`LC_COLLATE` 无效、`LANGUAGE` 无效、`LANG=`、`C.UTF-8`、`nb_NO`、`cs_CZ`、`ja_JP`、`zh_CN`。
     - 8 组候选：`aab/zab`、`Stage/stage/stagf`、`st_x/st-x/st1`、`ya/yb/ia/ib/ha/za`、`cha/hha/ca/da`、`ååa/aaa/zza/aza`、`Äpfel/apfel/Apfel/bpfel`、`--Opt-a/--opt_b/--opt-c`。
     - `da_DK` 下的输出正确：`aab zab` 加输入 `xab`，Node 和 Rust 都给出 `(Did you mean one of zab, aab?)`。
2. **B2 TTY 列宽（unix）**
   - 实现：`terminal.rs` 中 `isatty` + `TIOCGWINSZ`，每次渲染帮助时现读。
   - 用 `script` 分配 pty 做差分：`stty cols` 取 140、100、60、30、1、0；参数取 `--help`、`grp`（裸命令组，帮助打到 stderr）、`help grp`；重定向取不重定向、`2>/dev/null`、`>/dev/null`、写入文件（stdout 走管道、stderr 是 TTY 等组合）。stdout 和 stderr 两路以及退出码与 Node 完全一致。
   - `cols 0` 退回 80，`cols 1` 和 `cols 30` 不折行，与 Node 相同。
3. **S1 golden harness**：`golden_tests.rs` 增加了 `commanderInvalid` 分支和 `action: true` 的处理，数量断言改为 141，全部通过。
4. **O1 `parent`**：改为强引用 `Option<Command>`，环引用已在文档里说明。沿用第一轮的探针，丢掉 program 句柄之后，`help_information()` 输出 `Usage: raft agent [options]`，`parent()` 返回 `Some("raft")`，与 Node 一致。
5. **O2 解析器调用条件**
   - 实现：解析器签名改为 `Option<&str>`，调用条件是 `parse_arg.is_some() && val != Null`，与 `command.js` 的 `val !== null && option.parseArg` 逐字对应。
   - 与 Node 对照的场景：`--flag --flag --no-neg --opt --opt x`，解析器为 `String(v)+"|"+String(prev)`。两边都得到 `{"neg":"undefined|true","flag":"undefined|undefined|undefined","opt":"x|true"}`，键序也一致。其中 `[v]` 不带值时是 `null`，不调用解析器。
   - 布尔选项的解析器抛出 `InvalidArgumentError` 时，两边都输出 `error: option '--bad' argument 'undefined' is invalid. nope`。

---

## 本轮回归检查（没有发现问题的部分）

- **`parse_options` 改为 code unit 实现之后逐行对照**：
  - 组合短选项的剩余部分在原数组上把 `units[start+1]` 改写成 `-` 后放回，等价于 `args.unshift('-' + arg.slice(2))`，并且会重新经过所有分支。
  - 一个重要的边界情况也一致：放回的内容恰好是 `--` 时，会被当作字面量 `--` 处理，这和 JS 相同。
  - `find_option_units`/`is_units` 与 `is()` 等价。
  - 因为被改写的是一个非代理的 BMP 单元，放回的剩余部分不会出现孤立代理。
- **差分随机测试**：
  - argv：18000 条，覆盖 6 个程序，每个程序 3000 条，包括 `ties`。basic 程序新增的 token 有 `-a-`、`-ab-`、`-a--x`、`-a--`、`-abc-c`、`-a=b`、`-v-`、`-vV`、`-aaaa`、`-a--content=z`、`-a😀`、`-😀`、`--😀=x`、`-c😀`。golden 是在 `env -i` 下生成的，**0 处差异**。
  - wrap：2 万条；完整帮助：2000 条。**0 处差异**。
- **重入**：沿用第一轮探针，在 hook、解析器、action 里回调命令，没有出现 borrow panic。`env()` 返回的是 `Rc` 克隆，比较期间不持有 `RefCell` 借用。
- **guide 规则**：
  - `terminal.rs` 的 unsafe 块都有 SAFETY 说明；整数转换用 `usize::from`/`try_from`，没有 `as`。
  - `libc =0.2.189` 和 `windows-sys =0.61.2` 已 pin，D26 有记录。
  - crate 内仍然不调用 `process::exit`，用户输入路径上没有 panic。

---

## 应修

### S1. Windows 列宽取的是窗口宽度，Node（libuv）取的是屏幕缓冲区宽度 `dwSize.X`
- 位置：`terminal.rs:64-85`（`console_columns` 返回 `srWindow.Right - srWindow.Left + 1`），以及 D26 Decision 中的 "window width `srWindow.Right - srWindow.Left + 1`"。
- 源码依据：Node 24.15.0 捆绑的是 libuv 1.51.0（`process.versions.uv`）。`process.stdout.columns` 来自 `src/win/tty.c` 的 `uv_tty_get_winsize`：它先调用 `GetConsoleScreenBufferInfo`，再调用 `uv__tty_update_virtual_window(&info)`，最后返回 `*width = uv_tty_virtual_width`。而 `uv__tty_update_virtual_window` 里的赋值是 `uv_tty_virtual_width = info->dwSize.X;`，只有高度取自 `srWindow.Bottom - srWindow.Top + 1`（我对照的 tty.c 副本中在 :440-455 和 :1158）。
- 影响：
  - 在经典 conhost 里，当屏幕缓冲区比窗口宽时，两者不同。典型场景是关闭"调整大小时环绕文本输出"，或者缓冲区宽度设成了 120、窗口只有 80：Node 按 120 折行，Rust 按 80 折行。
  - Windows Terminal 和默认 conhost 下缓冲区宽度等于窗口宽度，结果一致。
  - 本机没有 Windows，无法实测，以上依据是 libuv 源码。
- 修复建议：改为 `usize::try_from(info.dwSize.X)`，同时修正 D26 的文字。`GetConsoleMode` 判定 TTY 的前置检查保留，它对应 `uv_guess_handle` 的 `FILE_TYPE_CHAR` 分支。

### S2. Windows 上 ICU 的默认 locale 不来自 `LANG`/`LC_*`，`default_locale` 没有平台分支（归属 raft-shared/D24）
- 位置：`raft-shared/src/js/collate.rs:17-80`（`default_locale` 只读 `LC_ALL`/`LC_MESSAGES`/`LANG`，任何平台都一样），以及 D24 Facts。commander 通过 `suggest_similar` 直接受影响。
- 源码依据：ICU4C `putil.cpp` 的 `uprv_getDefaultLocaleID` 在 `U_PLATFORM_USES_ONLY_WIN32_API` 分支里调用 `GetUserDefaultLocaleName`，然后把结果转换成 ICU ID，完全不看 POSIX 环境变量。V8 的 `Intl::DefaultLocale` 取的就是这个值。
- 影响：
  - 系统区域是丹麦语的 Windows 用户：Node 按 `da` 排序，Rust 因为没有 `LANG` 而使用 en-US。
  - Git Bash 等环境会设置 `LANG=en_US.UTF-8`，Node 忽略它，Rust 却会读取。
  - 本机无法实测，以上依据是 ICU 源码。
- 修复建议：
  - `default_locale` 增加 `#[cfg(windows)]` 分支，用 `GetUserDefaultLocaleName`（`windows-sys` 的 `Win32_Globalization`，需要在 D26 或 D25 登记新的 feature）得到的名字按 ICU 的规则映射。
  - D24 的 Facts 注明这些规则只适用于 POSIX。

---

## 可选

### O1. `SystemEnv` 每次比较都实时读取环境变量，Node 的默认 locale 在进程启动时就固定了
- 位置：`command.rs` 中 `ProcessState::default` 的 `env: SystemEnv`；`raft-shared/src/js/tz.rs:59-67`（`std::env::var_os` 实时读取）；`collate.rs` 的缓存以 locale tag 为键，不会固定"第一次看到的环境"。
- 探针结果（Node 24.15.0）：
  - `env -i LANG=en_US.UTF-8 node -e 'process.env.LANG="da_DK.UTF-8"; …'` 的结果仍是 `en-US aa,z`。
  - `env -i node -e 'process.env.LC_ALL="da_DK.UTF-8"; …'` 的结果同样是 `en-US`。
- 什么时候会出现差异：D14 第 4 步在 `__service`（launchd-user/systemd-user）采集登录 shell 环境后调用 `std::env::set_var`/`remove_var`。之后 Rust 按登录 shell 的 `LANG` 排序，Node 仍然用服务启动时继承的 locale。
- 对 commander 目前观察不到：`__service` 的候选只有 `slock-home`、`raft-home`、`os-supervised`、`help`、`version`，这些小写 ASCII 名字在已知的各种裁剪规则下次序都相同。但 daemon 里其他翻译过来的 `localeCompare` 排序（D3 列出的 `agentProcessManager.ts:8108` 等）同样走 `SystemEnv`，会受影响。
- 修复建议：在 `main` 执行 D14 第 4 步之前，把 locale 相关的三个变量取一份快照放进 Env（比如在进程启动时构造一个只含 `LC_ALL`/`LC_MESSAGES`/`LANG` 的 `BTreeMap`，传给 `program.set_env`，也用于其他 `locale_compare` 调用点）。同时在 D24 里写明"取进程启动时的值"。
