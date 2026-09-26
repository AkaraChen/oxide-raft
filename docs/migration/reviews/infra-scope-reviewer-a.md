# 基础设施评审 A：测试/范围核算（范围决策）

评审对象：`docs/migration/scope/rules.json`、`files.json`、`graph.json`、`tests-waived.md`、`tests/parity/upstream-macos.json`、`node tools/test-parity/check.mjs`。
依据：README Scope、GOAL.md、decisions.md D9/D11/D13/D16–D21、mapping-guide §12。
当前 parity 输出（macOS）：cli 918/7 豁免，computer 956/316 豁免，daemon 1674/168 豁免/12 out-of-scope，shared 438/139 out-of-scope，oar 126/65 out-of-scope。

## 阻塞

### B1. `osSupervisor*.ts` 通配把 D11 明确保留的 `osSupervisorLifecycle.ts` 标成 dropped
- 文件：`rules.json` drop `computer/src/osSupervisor*.ts`；`files.json` 中 `computer/src/osSupervisorLifecycle.ts` = dropped。
- 违反：D11 "Legacy supervisor entries: `parseLegacyOsSupervisorInvocation` and its refusal line … are translated exactly and kept despite the legacy-migration drop"；`__service` 校验 `--os-supervised` 并写 `RAFT_COMPUTER_OS_SUPERVISOR_KIND`；D11 的 Governs 里还有 `os_supervisor_lifecycle.rs`。README 也写 "The refusal of retired OS-supervisor entries stays"。
- 证据：in-scope 的 `index.ts:38,375-378`、`service.ts:96,133`、`runnerChildEnv.ts:1` 都从它导入（`parseLegacyOsSupervisorInvocation`、`OS_SUPERVISOR_KIND_ENV_VAR`）。drop 理由自己写着 "the refusal … stays per D11"，但文件照样被删。
- 后果：源码核算说这个文件不用移植，而 D11 要求保留它；`computerExistingKMacosAcceptance.contract.test.ts` 也因导入它，用错的理由整文件豁免了（见 O2）。
- 修复：把通配收窄为 `osSupervisor.ts` 和 `osSupervisorRuntime.ts`，`osSupervisorLifecycle.ts` 回到 in-scope。

### B2. `serviceUpgradeStart.ts` 整文件 dropped，但 D13 保留 `upgrade-start` 的拒绝分支
- 文件：`rules.json` drop `computer/src/serviceUpgradeStart.ts`（理由 "update (README Dropped)"）；`service.test.ts` 是 port-partial，droppedImports 里有 serviceUpgradeStart。
- 违反：D13 "The IPC method `upgrade-start` stays registered, and its only remaining branch is the `K_COORDINATOR_SEA_ONLY` rejection"；D8/D13 的 Governs 都列了 `service_upgrade_start.rs`。
- 后果：`service.test.ts:692`（"upgradeStart on a non-SEA service is a typed UPGRADE_START_REJECTED (K_COORDINATOR_SEA_ONLY)…"）和 `:848`（`describeUpgradeStartRejection`）测的是保留的行为。但 port-partial 加上 "dropped import"，会引导移植者把这两个 case 当作 dropped 豁免掉。
- 修复：把它放进 `rules.json.partial`，注明保留 `createServiceUpgradeStart` 的非 SEA 拒绝分支和 `describeUpgradeStartRejection`。`serviceUpgradeStart.scope.test.ts` 的两个 case 测的是 K seam，可以继续按 dropped 豁免。

### B3. release channel 属于 dropped，却被核算为 in-scope/port
- 文件：`computer/src/channel.ts`、`computer/src/lib/channelState.ts` = in-scope；`computer/src/channel.test.ts` = port（17 case：parseChannel/readChannel/writeChannel/runChannelVersions…）。
- 违反：README Dropped "`channel` (release channel)"；D13 "The release channel file `computer/channel` is not read or written"，而且 `channel` 命令不注册。
- 后果：这 17 个 case 会一直算 missing，要么逼着移植一个已删除的功能，要么 parity 门永远关不上。
- 修复：加 drop `computer/src/channel.ts`（理由写 README Dropped / D13），`channel.test.ts` 按 dropped 豁免。`lib/channelState.ts` 在 in-scope 代码里只剩 `lib/api.ts`（Electron 面）在用，要确认后一起 drop。

### B4. 含大量 dropped case 的测试被标成 `port`；doctor 的迁移输出被删掉却没有决策记录
- 文件：`computer/src/setup.test.ts` = port。其中约 20 个 case 测迁移探测、picker、`--machine` 采纳、adopt-by-fingerprint、诊断推送，例如 :1150、:1378、:1492、:1792、:1933、:1972、:2005、:2031、:2097、:2144、:2229、:2305、:2346、:2582。`computer/src/doctor.test.ts` 是 port-partial，droppedImports 只列了 kPaths，但 :193、:227、:304、:332 的 `--migration-details` case 依赖已 drop 的 `lib/migration.ts`。
- 原因：`droppedImports` 只看测试文件的直接 import。`setup.test` 通过 `setup.ts` 间接用到 `adoptLegacy`/`migration`/`diagnosticsPush`，所以没被识别出来。
- 违反：README 规定 Dropped 的 "observable effect is listed in `decisions.md`"。doctor 属于保留命令，但它的 `--migration-details`、setup-blocker 和 regret 细节（`doctor.ts:246-307`、`doctorCli.ts`）被删除，没有任何 D 记录。`setup --machine` 也没有明确写进 README Dropped。GOAL 规定这类情况要交给用户决定。
- 后果：parity 会把这些 case 算作必须移植。要么移植者去实现已删除的迁移逻辑，要么在没有依据的情况下自行豁免。
- 修复：把 `setup.test.ts` 改成 port-partial，并在 `tests-waived.md` 写逐 case 的 dropped 豁免。补一条 D 记录，写明 doctor 迁移相关输出和 `setup --machine` 的可观察效果；或者提交用户决定。

## 应修

### S1. `services/diagnosticsPush.ts` 的 drop 只引用了 research 笔记
- `rules.json` 理由是 "research/computer-core.md §6"。但 rules.json 自己的注释要求 "Every drop cites README Scope or a D-record"。而且 README 明确写 "`lib/api.ts` … and the rest of their closure are ported"，`lib/api.ts:51-54,225,464` 正好导入并暴露了 `diagnosticsPush`。
- 后果：README 保留的 lib/api 闭包被悄悄削掉了一块；`services/diagnosticsPush.test.ts` 的 16 个 case 被整文件豁免，却没有合规依据。
- 修复：补 D 记录（例如 "diagnosticsPush 只服务 Electron 和 setup 迁移路径"），或提交用户决定。

### S2. 未分类的测试文件被 `check.mjs` 静默算作 out-of-scope
- `check.mjs:146` 用 `scope.tests[c.file]?.status ?? "out-of-scope"`。未分类的文件：`daemon/src/testing/seaHostHarness.sea-suite.ts`，`oar/sea-trial/vendor/{claude,codex,pi}.vendor.test.ts`，`oar/apps/coxswain/test/*.test.ts`（共 7 个）。
- `seaHostHarness.sea-suite.ts` 由 `vitest.sea-host.config.ts` 单独运行，所以 oracle 列表里根本没有它。它测的是 D8 保留的 host kind 探测（"the real probe answers sea inside a SEA and node outside one"），这个测试现在完全不可见。
- 修复：在 rules 里显式分类这些文件（sea-suite 要么移植，要么按 packaging/bundling 豁免；coxswain 和 sea-trial 标 out-of-scope 并写理由）；让 check 遇到未分类文件直接报错，不再默认 out-of-scope。

### S3. port-partial 文件还没有逐 case 豁免，部分文件应该整文件豁免
- `tests-waived.md` 只有整文件行，没有任何逐 case 行。
- `installScriptContract.test.ts`（25 case，都是 install.sh/install.ps1 和 K reset）：README 把 "the installers" 列为 Dropped，应整文件按 dropped 豁免，而不是 port-partial。
- `drivers/piToolExecutionRuntimeSession.test.ts`（1 case，Pi SDK）：D17 已 drop pi，应整文件豁免。
- `status.test.ts:313-426` 的 K receipt case，以及 `residentLifecycleBridge.test.ts` 和 `agentProcessManager.builtin.e2e.test.ts:136` 里的 builtin case：需要逐 case 写 dropped 行。同一文件里的通用 handoff case（:196 起）要照常移植。

## 可选

- O1. `internal/h-family-chaos.ts` 在 graph 里除了自己的测试没有任何导入方。它本来就会被判为 out-of-scope，现在却用 README 未列出的理由 "chaos matrix" 算作 dropped 豁免。建议去掉这条 drop，改成 out-of-scope。
- O2. `computerExistingKMacosAcceptance.contract.test.ts` 的豁免理由写的是 "legacy OS supervisor retirement"，实际内容是 existing-K 验收。理由应改为 k-carrier/update。B1 修正后，这个理由也会失效。
- O3. `oar/tests/cli-progress.test.ts` 在 oracle 列表里是 0 个 case，需要确认是不是加载失败被吞掉了。

## 已核实无问题

- shared 符号闭包：对 48 个 out-of-scope shared 文件的导出名在 cli/daemon/computer/trace-client 非测试代码里做了全词匹配，只有 `generated/openapi.ts` 的 `paths` 命中，属于误报。25 个 shared out-of-scope 测试成立。
- oar：daemon 只在 `runtimeAccountUsage/{collector,oarAdapter}.ts` 用 `*Runtime` 和 `utcInstantFromDate`，sessions/projections/observe/registry/oar CLI 的 13 个 out-of-scope 测试成立。
- `agentO11yClient`、`historyFormatting` 在 packages 内除自身测试外没有导入方。
- 根：`index.ts` 动态 `import("./cli.js")`、`cli.ts:894` 动态导入 daemon core、`core.ts:1261` 导入 CLI dist，都已被遍历覆盖；trace-client 29 个 case 在 scope 内。
- D16 的 `agentMigration*`、D18 的 `traceBundleUpload.ts`、D20 的 `macosLoginCarrier.ts` 都是 in-scope/port。

结论：核算不能直接使用。B1–B3 把保留的行为标成了 dropped，或者把 dropped 的功能标成了 port；B4 让大量 dropped case 以 port 身份混在里面，doctor 的迁移输出删除也没有决策依据。这四项修完之后，parity 数字才可信。

## 第二轮复查

对照当前 `rules.json`、`files.json`、`tests-waived.md`、D22 和 `check.mjs` 的输出逐条复核：

- B1 已修：`osSupervisorLifecycle.ts` 现在是 in-scope；`computerExistingKMacosAcceptance.contract.test.ts` 的豁免理由改成了 k-carrier/update（O2 一并解决）。
- B2 已修：`serviceUpgradeStart.ts` 是 in-scope。`service.test.ts` 里的 SEA K coordinator case 按 D13 逐条豁免，非 SEA 的 `K_COORDINATOR_SEA_ONLY` case 继续移植，只用一行 `:: assert` 去掉 readChannelFn 这条断言，这个处理合理。
- B3 已修：`channel.ts` 和 `lib/channelState.ts` 已 dropped，`channel.test.ts` 已按 README Dropped/D13 整文件豁免。
- B4 已修：`setup.test.ts` 和 `doctor.test.ts` 改成 port-partial，逐 case 豁免行分别是 50 行和 10 行。D22 与上游一致：`cli.ts:466-475`（`--migration-details`）、`cli.ts:357-359`（`--machine`、`--fresh`、`--verbose`）、`setup.ts:1337-1342`、`setup.ts:1659-1662` 都核对过；D22 也写明这些选项是用户决定移除的。
- S1 已修：`diagnosticsPush.ts` 回到 in-scope，其测试为 port，D22 写明它因 `lib/api.ts` 而保留。
- S2 已修：`seaHostHarness.sea-suite.ts` 现在有明确分类，按 packaging 豁免（构建 SEA、postject、codesign），属于 §12 的 bundling/packaging 类别。`check.mjs:417` 对没有 waiver 的 out-of-scope 做了处理。
- S3 已修：`installScriptContract.test.ts`、`piToolExecutionRuntimeSession.test.ts` 已整文件豁免；`status.test.ts`、`agentProcessManager.builtin.e2e.test.ts`、`residentLifecycleBridge.test.ts` 都有对应的豁免行。
- O1 已修：`h-family-chaos.ts` 改为 out-of-scope。O3 保持不变：oar 目前有 140 个上游 case，没有发现加载失败。
- 新增的 source-scan 豁免抽查了 `cliLifecycleContract`（restart 那个 case 只对 `cli.ts` 文本做正则匹配，同文件里行为性质的 IPC handoff case 没有豁免）、`cliCommandReferenceContract` 和 `runtimeContract`，都确实在读 TS 源码文本，没有发现范围过宽。
- 当前 parity 输出：cli 918/17 豁免，computer 948 in-scope/429 豁免，daemon 1663/194，shared 300 in-scope，另有 46 个 extra 走白名单。

第二轮结论：没有剩余的阻塞或应修项。
