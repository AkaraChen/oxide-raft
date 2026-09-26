# 评审 B：测试枚举与 parity 工具（tools/scope, tools/test-list, tools/test-parity）

范围：只看工具是否正确，不评范围决策。所有结论都在本机复现过（macOS，node v24.15.0，只读运行；临时脚本放在 /private/tmp/claude-501/）。

已确认正确的部分：cli 列出 918 例，与上游 `packages/cli/test-execution-manifest.json` 的 `total: 918` / `fileCount: 103` 一致；六个包里没有同文件同键的重复用例，没有标题含 ` > ` 或换行，也没有「父 test 与其 t.test 子测试同时成为用例」的情况；按 §1.6 生成的函数名在所有文件内没有撞名，也没有空名或 Rust 关键字；committed JSON 里没有 `/Users/...` 这类绝对路径；graph.json 里没有未解析的相对路径说明符。

## 阻塞

### B1. list.mjs 吞掉了加载失败的测试文件，oar 少列了用例，gate 却看不到
- 位置：`tools/test-list/list.mjs:55-64`（listVitest 不看 `spawnSync` 的退出码，不看报告里的 `success` 和 `numFailedTestSuites`，也不看每个 `testResults[].status`/`message`）；`list.mjs:36-49` 对 node:test 同样忽略 `result.status`（只处理被信号杀掉的情况）。
- 证据：`target/test-list/macos/oar.json` 中 `success=false`，`numFailedTestSuites=3`：
  - `upstream/oar/tests/cli-progress.test.ts`：`Failed to resolve entry for package "@botiverse/oar"`（静态可数 6 个 `test(`）
  - `upstream/oar/apps/coxswain/test/agent.test.ts`、`activity-model.test.ts`：同样是解析失败
  这三个文件的 `assertionResults` 为空，所以 `upstream-macos.json` 里一例都没有。check.mjs:139 又把不在列表里或不在 files.json 里的文件默认当成 `out-of-scope`，于是这些用例在 parity 报告里连「范围外」都不算，直接消失了。cli-progress.test.ts 在 files.json 里标成 out-of-scope，这可能正是因为它在列表里是 0 例。
- 后果：只要上游的某个前置构建（这里是 oar 包的 dist）没做，整个文件的用例就会悄悄漏掉，而 D9 要求列表来自 oracle 的完整运行。
- 修法：任何文件加载失败、任何 `failed` 结果、runner 退出码非 0，都要写成 `{error}` 记录（check.mjs 已经会把 `c.error` 报成 problem），或者让 list.mjs 以非 0 退出。在 list.mjs 头部写明前置步骤（比如先 `pnpm -C upstream/oar build`，或者 vitest 用 `alias`/`conditions` 指向 src），修好后重新生成列表。

### B2. Gate 5 的「每个 crate 的 Rust 测试数 = 范围内 − 豁免」没有工具实现，也验证不了
- 位置：`tools/test-parity/check.mjs:62-90,160-173`。
- 问题一：只扫 `// test:` 行，从不统计没有键行的 `#[test]`/`#[tokio::test]`。多出来的 Rust 测试（没有键行、自造的、重复的）既不计数也不报错，所以 GOAL.md 里这个等式没人算。
- 问题二：`*_tests.rs` 是否真的被 `#[cfg(test)] #[path = "x_tests.rs"] mod tests;` 引入并参与编译，工具不检查。没被 mod 引入的文件永远不编译，里面的键行照样算「ported」。
- 构造输入：新建 `crates/raft-cli/src/foo_tests.rs`，写上合法的键行和 `#[test] fn ...`，不在任何地方 `mod` 它。结果 check 判定已移植，`cargo test` 却一个也不跑。
- 修法：对每个 crate 数出全部测试函数（带属性的 fn），报告里输出「rust-tests」一列，并断言它等于 ported；对每个 `*_tests.rs` 验证存在同名兄弟 `.rs` 里的 `#[path = "<name>_tests.rs"]`（或者改用 `cargo test -- --list` 的实际测试名做核对，这样最稳）。

## 应修

### S1. 按 OS 列表在 Windows 上路径全错
- 位置：`list.mjs:47,60` 用 `path.relative` 生成 `file`，Windows 上会得到 `upstream\raft-source\...`；`check.mjs:69` 用 `relative(root, file)` 作为 rustKeys 的键，也是反斜杠；而 `rustTestFileFor`（check.mjs:40-45）和 files.json 用的是正斜杠。
- 后果：在 Windows（Gate 6 要求跑）上，`scope.tests[c.file]` 全都查不到，所有用例都落进 out-of-scope（missing 会是 0，属于假通过）；同时所有 Rust 键都报「matches no upstream case」。
- 修法：所有路径统一 `.split(sep).join("/")`（上游 run-tests-with-manifest.mjs 就是这么做的，见其 `toPosixPath`）。

### S2. 不在 files.json 里的文件默认算 out-of-scope，而且不报告
- 位置：`check.mjs:139` `scope.tests[c.file]?.status ?? "out-of-scope"`。
- 证据：coxswain 的 `activity-size`、`ipc`、`say-bridge`、`say-protocol`、`usage-model` 等测试文件在列表里，却不在 files.json 里，都被静默计为 out-of-scope（oar 行 out-of-scope=65）。上游新增一个测试文件时也会静默消失。
- 修法：files.json 里显式列出每个被列出的测试文件（包括 out-of-scope 的，写上理由）；缺失时报 problem。

### S3. 豁免表解析会被转义竖线打断，且 `--write-file-waivers` 自己就会生成这种行
- 位置：`check.mjs:95-99` 先 `line.split("|")` 再去处理 `\|`，这个顺序是错的。
- 构造输入：`| \`a.test.ts\` | x \| y | dropped | r |` 切分后是 5 格，`cells.length !== 4`，整行被静默丢弃。`check.mjs:108` 生成整文件豁免时，会把 reason 里的 `|` 写成 `\|`，所以只要某个理由含 `|`，写出来的行就会立刻失效。按用例写的键里如果有 `|`，也无法豁免。
- 修法：用 `/(?<!\\)\|/` 切分；不是 4 格的表格行要报错，不要跳过。

### S4. 豁免没有交叉校验
- 位置：`check.mjs:93-101,140-157`。
- 没有检查以下几点：类别是否属于 §12/D9 允许的集合；豁免指向的文件或键是否存在于上游列表（写错的或过期的豁免会静默无效）；`*` 行覆盖的文件在 files.json 里是否是 `port`/`port-partial`（`*` 优先于 status，一行就能把在范围内的文件整个豁免掉）。
- 证据：files.json 里 9 个 `port-partial` 文件（如 `computer/src/service.test.ts`、`daemon/src/agentProcessManager.codex.test.ts`）在 tests-waived.md 里都没有对应的逐例行。工具没有能力区分「应当豁免的那部分」和「漏移植」，只能等它们全部变成 missing。
- 修法：检查类别白名单；报告未命中任何用例的豁免行；`*` 只允许用于 status=waive 的文件；对 port-partial 文件要求逐例行。

### S5. 函数名规则的「转写到 ASCII」有歧义，实现者很难自然写对
- 位置：`check.mjs:59-63`：NFKD 之后去掉组合符，其余非 ASCII 一律当成分隔符。
- 证据：上游有 242 个标题含非 ASCII 字符，例如 `(§10 isolation still honored)`、`authorize → onUserAction`、`—`。用 Rust 常见的 `deunicode` 转写时，`§` 变成 `SS`，得到 `ss10_...`，而工具要求 `_10_...`；`→` 变成 `->`，碰巧结果一致。这些情况会产生大量「test fn is X, §1.6 gives Y」噪音。
- 修法：在 mapping-guide §1.6 把算法写死（NFKD，去掉 U+0300–036F，其他非 ASCII 视为分隔），或者提供 `cargo xtask test-name "<title>"` 或 `node tools/test-parity/check.mjs --name` 供实现者调用。

### S6. dedupe 按「整个 Rust 文件」计数、按 Rust 文件中出现的顺序计数，规范没有说明
- 位置：`check.mjs:77-83`。
- 构造输入：上游 `describe("a") it("works")` 和 `describe("b") it("works")`。实现者自然会写成 `mod a { fn works }` 和 `mod b { fn works }`，工具却要求第二个叫 `works_2`。如果实现者调换了两个测试的顺序，`_2` 就落到另一个函数上。另外 `seen` 用 base 计数，不检测「`a` 出现两次」加上「真有标题 `a 2`」时的撞名（当前数据里没有，但工具不会报）。
- 修法：§1.6 写明「在同一个 `_tests.rs` 内、按上游顺序」；工具应按上游顺序计算期望名，并检查生成名的唯一性。

### S7. 键行和函数的配对很脆弱
- 位置：`check.mjs:72-75`。只看后面 7 行，`fn` 取窗口里第一个；测试属性只认 `#[test]` 和 `#[tokio::test...]`。
- 构造输入：
  - 两个 `// test:` 行连着写在同一个 fn 上方：两行都通过「后面有测试」检查，同一个 fn 算作移植了两例，只有名字检查可能碰巧拦住。
  - 多行 `#[cfg_attr(windows, ignore = "upstream skip: ...")]` 加上 `#[tokio::test(flavor = "multi_thread", ...)]` 和几行 doc 注释，超过 7 行就会误报。
  - `#[rstest]`、`#[test_log::test]` 被判定为「不是测试」。
- 修法：从键行向下跳过属性和注释，一直到第一个 `fn`，要求其间恰好有一个测试属性，并且下一个键行出现在该 fn 之后。

### S8. 上游跳过的用例既没有记下理由，也没有核对
- 位置：`list.mjs:62-63` vitest 分支没有记录跳过原因；`check.mjs:151` 只数 upstreamSkipped，不检查 Rust 侧是否带 `#[cfg_attr(<cfg>, ignore = "upstream skip: …")]`。
- 证据：macOS 上 vitest 的 51 个跳过用例（computer 14 / daemon 21 / oar 16）都没有 skipReason。D9 要求带着同样的条件移植，工具却对「没加 ignore 就直接跑」或「平白加了 ignore」一概放行（GOAL.md 禁止额外 `#[ignore]`）。
- 修法：vitest 用 `vitest list --json`（D9 原文）或 JSON 里的 `meta` 记录跳过原因；check 对 skipped 用例要求 ignore 属性，对非 skipped 用例禁止 `ignore`。

### S9. graph.mjs 漏掉「按路径 spawn 或读取」的依赖边
- 位置：`tools/scope/graph.mjs:81-97` 只识别 import、export、`import()`、`vi.mock/doMock/importActual` 和 `import("x")` 类型。
- 证据：`computer/src/concurrency.test.ts:157` 用 `join(import.meta.dirname, "test-fixtures", "mutationLockCompromiseChild.ts")` spawn 夹具；`computer/src/osSupervisor.test.ts:461` 引用 `osSupervisorRuntime.ts`；`cli/src/parserOutput.test.ts:29` spawn `src/index.ts`；`cli/src/commands/agent/login.test.ts:146`、`computer/src/shellEnvCapture.test.ts:1151`、`daemon/src/agentProcessManager.claude.test.ts:1302` 用 `new URL("./x.ts", import.meta.url)`。这些都不是图里的边。当前夹具靠 classify 规则兜底成了 test-support，但图本身不完整。另外 `require()` 不被识别，`scanRoots`（graph.mjs:39-47）也不含 `upstream/oar/packages/cli`，所以 cli-progress.test.ts 的目标文件不在图的节点里。
- 修法：再识别三类形式：`new URL("<lit>", import.meta.url)`；`join`/`resolve` 的参数全部是字面量或 `import.meta.dirname`/`__dirname`，且以 `.ts`/`.mjs` 结尾；`require("<lit>")`。把这些作为 `kind: "path"` 边，并把 oar 的 packages/* 全部加入 scanRoots。

### S10. 点号文件名的 Rust 落点只有工具注释里有，mapping-guide 没写
- 位置：`check.mjs:31-45`（注释写着 `a.b.test.ts → a_b_tests.rs`）。
- 证据：`agentProcessManager.claude.test.ts` 对应 `agent_process_manager_claude_tests.rs`，但不存在 `agent_process_manager_claude.rs` 可以用 `#[path]` 引入它；`apiClient.authclient.test.ts`、`*.contract.test.ts`、`*.e2e.test.ts` 同理。oar 的 `upstream/oar/tests/x.test.ts` 映射到 `crates/oar/tests/x_tests.rs`，这是 Cargo 集成测试目录，每个文件都是独立的 crate，和 §1.6「放在代码旁边，从 `x.rs` 用 `#[path]` 引入」相冲突。另外 `.test.mjs` 不会被去掉后缀（得到 `*_test_mjs_tests.rs`，目前只影响已豁免的文件）。
- 修法：在 §1.1/§1.6 补上规则（例如从主模题 `agent_process_manager.rs` 用 `#[path = "agent_process_manager_claude_tests.rs"] mod claude_tests;` 引入；oar `tests/` 的归属也写清楚），正则改为 `/\.test\.(m?[jt]sx?)$/`。

## 可选

- O1. `list.mjs:66-75` 合并已有 JSON 时用本次的 `process.version` 覆盖 `merged.node`；分包在不同 Node 版本上列出时，记录会失真。建议每个包各记 `node`、上游 commit，以及生成命令（按 §12 的 `.cmd` 惯例放一个 `upstream-<os>.cmd`）。
- O2. `graph.mjs:28-36`、`list.mjs:23-31`（vitest 部分依赖 vitest 的顺序）没有对 readdir 或测试结果排序；Linux ext4 上的目录顺序不确定，JSON 的 diff 会抖动。建议输出前按文件排序（用例保持文件内的顺序）。
- O3. `list.mjs:56` 设置了 `CI=1`，这会改变上游行为（例如 oar 的 vitest.config 里 `update: process.env.CI === undefined ? "all" : "none"`）。如果某个测试用 `skipIf(process.env.CI)`，跳过数也会不同。应当写明这是有意为之，并和 Gate 6 的 CI 环境保持一致。
- O4. shared 的 `test:message-lockstep`（`scripts/check-agent-api-message-lockstep.mjs`）是 `pnpm test` 的一部分，但不在任何列表里，也没有写进豁免或范围说明。
- O5. `check.mjs:76` 从 Rust 键里取 `split(" > ").at(-1)` 当标题。如果标题本身含 ` > `（目前 0 例），函数名校验会算错。应改为直接用匹配到的上游用例的 `title`。
- O6. `caseKey` 只转义了 `\n`，`\r`、制表符和首尾空白都没处理（目前 0 例），而 rustfmt 或编辑器会去掉行尾空白。建议规定对这些字符也做转义。

## 结论
cli 的枚举与上游 manifest 吻合，键的构造也基本可靠；但列表会静默吞掉加载失败的文件（oar 已实际丢失用例），Gate 5 的计数等式和「测试文件是否真的被编译」都没有实现，所以这个 gate 目前还不能作为「1:1 已移植」的证据。

**判定：不通过（2 阻塞，10 应修，6 可选）。**

## 第二轮复查

方法：把 tools、crates、tests/parity、docs/migration/scope、tests-waived.md 复制到 `/private/tmp/claude-501/r2`（upstream 用软链接），在副本的 crates 下放 Rust 夹具文件后运行 check.mjs；仓库本身没有改动，副本已删除。另外在仓库里重新生成 graph.json 和 files.json，结果与提交版本逐字节一致。

### 逐条复核
- **B1 已修复。** list.mjs 现在会检查 runner 退出码、vitest 的 `success`、文件加载失败和 failed 用例，并对 oar 先执行 prebuild。重新生成的列表里 `error` 记录为 0；cli-progress 和 coxswain 的用例已列出（相关文件共 22 例），oar 共 140 例。
- **B2 已修复。** 夹具验证了下面几种情况：
  - 没有被 `mod` 引入的 `orphan_tests.rs` 报「not compiled」；
  - 没有键行的 `#[test]` 报 problem；
  - 带键行、但函数没有测试属性的，报「0 test attributes」；
  - 键行数和移植数不一致时，报「Gate 5: … keyed test fns 3, ported 4」。
  报告多了 rust-tests 和 extra 两列。
- **S1 已修复。** 所有路径都经过 `posix()` 转成正斜杠。
- **S2 已修复。** 列表里出现但 files.json 没有收录的文件会报 problem；当前 0 例。
- **S3 已修复。** 含 `a \| b` 的断言行能正确解析；`--write-file-waivers` 往返运行后文件不变（幂等）。
- **S4 已修复。** 类别白名单、`*` 只能用于 status=waive、逐例行只能用于 port-partial、过期行、「已移植又被豁免」都能报出来（夹具逐项触发）。
- **S5 已修复。** §1.6 写明了算法，并提供 `--name`。
- **S6 已修复。** dedupe 改成按上游顺序、按上游文件计算，并检查撞名和关键字。
- **S7 已修复。** 下面几种写法都能正确识别：
  - 多行 `#[tokio::test(...)]`；
  - 夹在中间的 doc 注释；
  - `cfg_attr`；
  - 一个函数上方两个键行；
  - 裸 `#[ignore]`。
- **S8 部分修复。** 见下面的新问题 R2。
- **S9 已修复。** `concurrency.test.ts` 到夹具、`osSupervisor.test.ts` 到 `osSupervisorRuntime.ts` 的边都在，扫描目录也包含了 oar 的 packages/cli。残留一处：`parserOutput.test.ts:29` 在 spawn 参数数组里用裸字面量 `"src/index.ts"`（相对 cwd），这条边仍然缺失。目标文件本来就在范围内，所以列为可选。
- **S10 已修复。** 规则已写进 §1.6；`.test.mjs` 会去掉后缀；oar 映射为 `src/tests/`。
- **O1–O6 已修复。** 每个包记录 meta（node 版本、upstreamCommit、执行的命令），并生成 `.cmd` 文件；输出已排序；CI=1 已注明；lockstep 脚本作为一个用例列出；函数名直接用上游 title 计算；`\r` 和 `\t` 会被转义（目前没有标题含反斜杠，所以不存在转义后与字面反斜杠混淆的问题）。

### 新问题
- **R1（应修）集成测试目录被判为「未编译」，与 §12 矛盾。**
  - 位置：`check.mjs` 中的 `compiledFiles` 只把 `src/lib.rs`、`src/main.rs`、`src/bin/*.rs` 当作编译根。
  - 夹具：`crates/raft-cli/tests/cli.rs` 里写一个 `#[test]`，报「not compiled」。
  - 为什么矛盾：mapping-guide §12 要求进程类测试 spawn `env!("CARGO_BIN_EXE_raft")`，而 Cargo 只在编译集成测试和 bench 时设置这个变量。按 §1.6 放在 `src/*_tests.rs` 里的单元测试用 `env!` 会编译失败；放进 `tests/` 又会被工具拒绝。`parserOutput.test.ts`、`login.test.ts` 这类 spawn `src/index.ts` 的测试，两条路都走不通。
  - 修法：二选一。
    - 在 §12 改用运行时定位二进制（例如 `std::env::current_exe()` 的上两级目录，或者 `option_env!` 加 cargo 构建）。
    - 允许 `crates/*/tests/*.rs` 作为编译根，并规定这类上游文件对应的 Rust 路径。
- **R2（应修）按环境变量开启的上游跳过被当成了 OS 跳过。**
  - 证据：macOS 列表里有 67 个 skipped 用例。可以确认的环境门控至少包括：`claude.integration.test.ts` 的 4 例（未设置 `RUN_CLAUDE_INTEGRATION_TESTS=1`，或本机找不到 Claude 二进制时跳过）；codex、grok、pi 的 integration 测试；oar 的 `sea-trial/vendor/*` 共 16 例。
  - 夹具：在函数里写运行时 `if env::var("RUN_CLAUDE_INTEGRATION_TESTS").is_err() { return; }`，这是忠实的移植，但被报成「needs cfg_attr ignore active on macos」。
  - 后果：只能加 `#[cfg_attr(target_os = "macos", ignore)]`，这样 macOS 上即使设置了开关也永远不跑，违背 GOAL「不允许额外 #[ignore]」的本意。而且列表结果取决于列表机器上装没装 claude、codex 等 CLI，不可复现。
  - 修法：列表时记录跳过原因（vitest 可以用 `vitest list --json` 或 task meta），把跳过分成 os 和 opt-in 两类，写进 D9；opt-in 类改为要求 Rust 侧有同名环境变量的运行时门控，不再要求 cfg_attr。
- **R3（可选）内联嵌套模块会被误判为未编译。** `lib.rs` 中 `mod inl { mod deep; }` 对应 `src/inl/deep.rs`，Rust 会编译它，工具却报「not compiled」。原因是 compiledFiles 不跟踪内联 `mod x {` 的目录层级。可以在 §1.6 规定测试模块不允许放在内联 mod 里，或者让解析器支持这种写法。

### 第二轮结论
第一轮的 2 个阻塞项都已修复，没有阻塞项残留；S8 只是部分修复（其余部分记为 R2）。新发现 2 个应修（R1 集成测试目录与 `CARGO_BIN_EXE` 相矛盾、R2 环境开关跳过被当成 OS 跳过）和 2 个可选（S9 残留、R3）。

**判定：有条件通过（0 阻塞，2 应修）。**
