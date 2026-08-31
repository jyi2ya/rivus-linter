# 消除 cargo check #2：迁移工作文档

> 每次变更之前先更新本文档（已完成工作 + 接下来计划），再实现。
> 阶段定义与动机见会话决策；本文件是唯一进度权威。

## 目标

`cargo rivus check` 从两次 Cargo 编译迁移为一次：

1. 直接 rustc lint（node/body lint、测试命名、快照检查等）在采集编译（#1）内执行，保留 crate-root Rivus lint level 与 `-D warnings` 语义。
2. 图诊断（需要合并调用图的诊断）由离线引擎计算，severity 固定（Error/Warning），不受 Rust lint 属性影响。
3. #1 非零退出（编译错误或 deny 级直接 lint）时不合并图、不推断、不进入回放阶段，原样返回失败。
4. 第二次 Cargo 编译、offline emissions JSON、ack 回执、`RIVUS_OFFLINE_*`/`RIVUS_UNTESTED_PATHS` 传输最终删除，图诊断由外部渲染器（miette 类）从 artifact source 解析位置并渲染。

## 原则

- 每个阶段独立通过全部门禁（fmt/build/clippy/test/rivus check/git diff --check），可独立提交、可回滚。
- 不让现有 UI 快照大规模失效：阶段 0–4 不改变 ProjectCaps 直连模式的输出；真实 pipeline 行为变化集中在阶段 2 与阶段 7。
- 图诊断固定等级是契约；迁移期内回放阶段暂时仍走 rustc lint 管线（见决策记录 D1）。
- 不新增布尔执行标志；执行策略用 enum 表达（见决策记录 D2）。

## 阶段清单与状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 固定设计边界：诊断类别、固定等级、短路规则写入理论文档与手册 | **完成** |
| 1 | 用 `LintExecutionMode` enum 替换 pass 内布尔三元组（行为保持） | **完成** |
| 2 | 直接 lint 移入采集编译；check #1 失败短路；#2 只回放图诊断 | **完成** |
| 3 | untested selection 在中间直接构造为图诊断，删除 #2 的 good/ok 重登记 | 未开始 |
| 4 | #2 缩成纯 anchor replay（identity→Span 收集器），删除其余收集职责 | 未开始 |
| 5 | 建立 artifact `DiagnosticSource` resolver + 一致性对照测试 | 未开始 |
| 6 | 外部渲染器端口 + shadow mode 输出比对 | 未开始 |
| 7 | 切换渲染器，删除第二次 Cargo 编译与 emissions/ack 协议 | 未开始 |

## 阶段 2 补充（评审发现，完成）

- [x] **Issue 1（回归）**：覆盖候选登记丢失 Production scope 门控，UI/直连模式对 `mod tests` 内未调用 helper 误报 `rvs_untested_good_fn`。修复：pipeline 调用处加 `scope == DiagnosticScope::Production`；新增 UI fixture `437_test_module_helper_not_coverage_candidate_20260831.rs`（先复现失败，修复后 check-pass）。
- [x] **Issue 2（测试补齐）**：新增 `test_20260831_check_graph_error_fails_replay_phase`（static 读取 → 契约缺 S → #2 回放 Deny → Err(101)）；`测试策略` 中 `-D warnings` 项已标注按 D1 推迟。
- [x] **Issue 3（文案）**：短路消息按错误类别区分——`ExitCode(n)` 报"fix the diagnostics above"，spawn/Message 类失败报 "offline caps check unavailable"；顺带删除仅剩测试引用的 `rvs_callgraph_failure_exit_code` 及其旧测试（退出码透传已由 20260831 端到端测试覆盖）。
- 门禁：fmt/build/clippy(基线 6)/test 全过/`cargo rivus check` ok/git diff --check ok。

## 阶段 3 计划（接下来）

- [ ] 中间在计算 `rvs_uncovered_test_functions` 后，把 uncovered 直接转换为 `OfflineCapsEmission`（identity 锚点，lint=UntestedGoodFn/UntestedOkFn，severity 固定 Warning），与 contract/incomplete 等 emissions 合并后由 #2 回放。
- [ ] 删除 `RIVUS_UNTESTED_PATHS` 传输与 `untested selection` JSON 写入/读取；`RivusOfflineDriverInput.untested_paths` 移除（marker Analysis::Offline 的 `has_untested_selection` 字段随 schema 一并清理或保留至阶段 4 统一删除——倾向现在删）。
- [ ] `#2` 的 good/ok 登记（`rvs_register_coverage_candidate_BM`）与 `rvs_check_selected_untested_fns_S` 停用；`test_quality` 相应收窄。
- [ ] untested 发射锚定依赖 #2 收集 walk 产生的 `diagnostic_spans`（identity→Span），消息格式与现快照保持一致；UI 直连模式不受影响（其 local coverage 走 `check_local_coverage`）。
- [ ] 回归：untested fixture（41/183/425/430 等）stderr 不变；workspace 快照更新。

## 已完成记录

### 阶段 0（2026-08-31）

- `docs/theory/function-graph.md`：新增"诊断类别与等级"一节（直接 lint vs 图诊断、固定 Error/Warning、采集编译非零短路、渲染属环境适配、无源码锚点按 def_path 呈现不丢弃）。
- `docs/theory/environment-boundary.md`：图诊断渲染归环境层，核心只产结构化诊断。
- `src/rivus-manual.md`：`输出分类` 增加两类诊断的等级语义与短路说明。
- 门禁：fmt/build/clippy(基线 6)/test 全过（546+）、`cargo rivus check` ok、git diff --check ok。无代码变更。

### 阶段 2（2026-08-31）

- `src/lints/ports.rs`：新增 `CheckAndCollect`；`ReplayDiagnostics.rvs_should_emit_lints()` 改为 false；新增 `rvs_registers_coverage_candidates()`。
- `src/lints/mod.rs`：覆盖候选登记从 `rvs_run_fn_checks_S` 提取为 `rvs_register_coverage_candidate_BM`（pipeline 按 mode 调用）；`rvs_effective_caps` 提取共享。
- `src/environment/workspace.rs`：`CollectionLints` enum（Silent/Check）；marker Collection 变体增加 `lints` 字段，schema 4→5；`RivusDriverMode::Callgraph{output,lints}`；`rvs_run_cargo_check_at_BIST` 提取（可传项目路径）；#1 使用 `Check` 目的；cargo 失败透传退出码；**删除 ProjectCaps fallback**。
- `src/environment/lint_driver.rs`：Callgraph{lints} → CheckAndCollect / CollectOnly。
- `src/main.rs`：`rvs_callgraph_lint_mode` 三态——仅 Silent 采集加 `-Awarnings`/ForceWarn，Check 采集保持正常 lint level。
- `tests/ui_tests.rs`：schema 常量 5。
- 新增回归测试（真实 cargo 临时项目）：`test_20260831_check_collection_deny_short_circuits_graph_analysis`（stub deny → Err(101) 短路）、`test_20260831_check_warning_project_completes_full_pipeline`（warning → Ok(())）。
- 端到端验证：本仓库与下游 fixture——直接 lint（missing doc、test name format 等）全部出现在 #1，图诊断（make_seed missing prefix）只在 #2，无重复；report（silent 采集）不变。
- 门禁：fmt/build/clippy(基线 6)/test 全过（548+UI 3）/`cargo rivus check` ok/git diff --check ok。

### 阶段 1（2026-08-31）

- `src/lints/ports.rs`：新增 `LintExecutionMode`（CollectOnly/ReplayDiagnostics/ProjectCapsCompatibility）+ `rvs_should_emit_lints`/`rvs_collect_caps_facts`/`rvs_is_caps_report` 访问器；`RivusLintConfig` 以 `mode` 替换 `collect_callgraph`+`should_emit_caps_report`。
- `src/lints/ctx.rs`：`FnCheckData` 以 `mode` 替换两个布尔。
- `src/lints/mod.rs`：`RivusLintPass` 三布尔收拢为 `mode`；删除 `should_emit_caps_report` 失败清零的可变标志（改为 crate_post 的 `capsmap.is_some()` 守卫）；全部 `data.should_emit_lints`/`data.collect_caps_facts` 消费点改经访问器。
- `src/environment/lint_driver.rs`：`rvs_prepare_lint_config_BIS` 单点映射 driver mode → execution mode。
- 行为保持：全量 test、UI 套件、clippy 基线 6、`cargo rivus check` ok、git diff --check ok 均与改前一致。

## 决策记录

- **D1（等级迁移缺口）**：图诊断固定等级自阶段 0 起是文档契约；但回放阶段在阶段 2–6 仍通过 rustc lint 管线发射（受 `-D warnings` 影响），阶段 7 切换外部渲染器时才完全落地。ProjectCaps/UI 直连模式保持 lint 管线直至阶段 7 的 harness 迁移。
- **D2（enum 切分）**：`LintExecutionMode` 的 `CheckAndCollect` 变体在阶段 2 接线时才引入（阶段 1 仅收拢现有三种模式），避免 dead variant。collection generation 的"目的"enum（检查采集 vs 静默分析采集）同样推迟到阶段 2，因为它属于 marker schema 变更。
- **D3（短路范围）**：#1 任何非零退出都短路（不区分 deny 与编译错误），现有 collection 失败时的 ProjectCaps fallback 在阶段 2 取消。
- **D4（登记简化）**：`rvs_collect_caps_facts` 收拢为恒 true——原 Offline 模式在 emissions 为空时跳过图收集的分支被删除；该差异仅影响进程内无用的工作量，不产生输出差异（artifact 写出仅在配置了 callgraph_output 时生效，#2 无输出目录）。
- **D5（迁移期 builtin warning 重复，已知回归）**：#1（Check 采集）与 #2（回放）都是 Normal lint level、独立 target 目录各自全新编译，rustc 内建 warning（如 unused variable）会在两遍各出现一次。Rivus 自身诊断无重复（模式门控）。备选方案（#2 加 `-Awarnings` 或把回放改为 dcx 固定等级发射）都会改变图诊断等级语义，属阶段 5–7 的工作；作为迁移期已知噪音接受，阶段 7 消除。

## 测试策略

1. 阶段 0–1：现有全部测试与快照不变。
2. 阶段 2：新增端到端 fixtures（deny 短路、warning 不短路、图 Error 退出码）；`-D warnings` 不升级图 warning 一项按决策 D1 推迟至外部渲染器切换时验证（迁移期内图诊断仍走 rustc lint 管线）；现有 workspace 集成快照如有输出位置变化按新行为 rebless。
3. 阶段 3–6：每阶段先加一致性测试（resolver vs #2 anchor collector 输出相同）。
4. 阶段 7：图 UI fixtures 分批迁移到独立 graph-diagnostic snapshot harness；混有 `rvs_untested_good_fn` 的旧快照最后处理。
