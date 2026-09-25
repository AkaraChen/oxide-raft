# Goal

按 `docs/migration/README.md` 的契约，把 `upstream/raft-source` 移植成 Rust，按顺序通过 Gate 1–7。范围内的上游测试逐个搬运、全部通过，最后用我的 raft.build 账号跑通端到端。

## 完成条件

全部满足才算完成。

- **Gate 1**：`cargo check --workspace --all-targets` 零错误。
- **Gate 2**：`cargo build --release` 产出 `raft` 和 `raft-computer`。
- **Gate 3**：`raft --help`、`raft-computer --help`、`raft-computer status` 的输出与 oracle 生成、存放在 `tests/golden/` 的 golden 一致。唯一例外见 D13：`raft-computer --help` 恰好少 `channel`、`operation`、`upgrade` 三行。
- **测试一对一搬运**：范围内每个上游测试（按 D9 的 oracle 列表，模板标题展开、循环逐条计），无论单元测试还是集成测试，都有且只有一个对应的 Rust 测试。输入和断言与上游相同（mapping-guide §12），并带 `// test:` 键行，函数名按 §1.6 规则生成。
  - 集成测试一个不少：上游起真实本地服务器的，用 `127.0.0.1:0` 上的 axum/hyper 服务器；spawn `src/index.ts` 的，改为 spawn 编译出来的 `raft` / `raft-computer` 二进制，环境变量和参数与上游一致。
- **Gate 5**：`cargo test --workspace` 全部通过；`tools/test-parity` 显示遗漏数为 0，每个 crate 的 Rust 测试数 = 上游范围内测试数 − 豁免数。
- **豁免**只允许 mapping-guide §12 列明的类别：检查 TS 源码文本、打包、Node 版本预检、npm 打包、已丢弃的功能。每条都写进 `tests-waived.md`，注明上游文件、标题和理由。上游本身在某个 OS 上跳过的测试，按 D9 用同样的 cfg 条件标注。除此之外，不允许 `#[ignore]`，不允许削弱断言，不允许删测试；遇到这些类别以外、确实无法移植的测试，停下来问我。
- **Gate 6**：GitHub Actions 在 macOS、Linux、Windows 上跑完整测试套件并全部通过，每个 OS 的测试数与该 OS 的 parity 报告一致，没有静默跳过。
- **Gate 7**：用我的 raft.build 账号做端到端测试。
  - 登录方式：用 `op` CLI 取凭据，在 agent browser 里登录，建一个测试用的 computer。
  - 用 Rust 版 `raft-computer` 完成 setup、attach、start；在本机装好的 codex、cursor-agent、grok 上各跑一个 agent，通过 Rust 版 `raft` 收发消息；最后 stop，并清理干净。
  - 每次运行的命令和输出都存到 `docs/migration/evidence/`。
  - 开始 Gate 7 前，先在 `decisions.md` 记一条：我的账号就是 README 里说的「dedicated test server」。

## 工作顺序

按 README「Order of work」依次做 commander、raft-shared、raft-trace-client → raft-cli → oar、raft-daemon-core → raft-computer，然后依次过 Gate 6、Gate 7。

每个阶段开始前，先由编排者运行需要的 oracle 脚本（`tools/golden/`、`tools/zod-golden/`、D9 的 parity 列表），生成 golden 并提交。`tools/test-parity` 在第 1 阶段就要先建好，之后每个单元都用它核对。

## 每个工作单元的闭环

按 README「Work units and roles」：

- 一个单元 = 一个源文件，加上它的测试文件。测试和实现在同一个单元里一起搬运，不能先搬实现、以后再补测试。
- 实现者只翻译分到的文件。
- 两个对抗评审在隔离的上下文里各写一份报告，放到 `docs/migration/reviews/`。评审要核对该文件的每个上游测试是否一对一搬运。
- 修复者逐条处理，不能没有证据就驳回。
- 复查时两份报告都不再有「阻塞」和「应修」才算通过；「可选」项登记到 `HANDOFF.md`。
- 评审 agent 必须等到跑完、把结果收回，不能留着不管。
- 只有编排者跑构建和测试，也只有编排者操作 git。
- 上一轮试点的两份评审报告（`docs/migration/reviews/pilot-claim-reviewer-a.md`、`-b.md`）作为已知坑的清单，比如 safe-int、async、JsString、V8 数字和日期规则、zod issue 格式、测试命名、golden 放置位置。

## 提交

- 每个工作单元通过闭环、并且该单元的测试全部通过后，在 `master` 上提交并立即推送到 `origin/master`。
- 每个阶段结束时更新 `HANDOFF.md`（进度、门禁状态、test-parity 数字、登记的可选项），保证下一次会话能直接接上。

## 边界

- 不修改 `upstream/`。
- 不重开 D1–D21 的决定。实现需要改动契约时，先在 `decisions.md` 新增记录并说明理由。
- 在 raft.build 上只操作为测试新建的 computer 和 agent，账号里其他已有的东西一律不碰。

## 停下来汇报

遇到以下情况，停下来汇报，不要硬推：

- README「Stop and report」列出的三种情况；
- 需要我来做产品或范围决定；
- 有测试在允许的豁免类别之外却无法移植；
- 缺依赖、工具或者网络，而且无法绕开；
- raft.build 登录需要人工介入（比如二次验证）；
- 同一个工作单元连续三轮复查仍有阻塞项。
