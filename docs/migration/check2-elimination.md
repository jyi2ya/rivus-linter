# 消除 cargo check #2：迁移工作文档

> 每次变更之前先更新本文档（已完成工作 + 接下来计划），再实现。
> 阶段定义与动机见会话决策；本文件是唯一进度权威。

## 目标

`cargo rivus check` 从两次 Cargo 编译迁移为一次：

1. 直接 rustc lint（node/body lint、测试命名、快照检查等）在采集编译（#1）内执行，保留 crate-root Rivus lint level 与 `-D warnings` 语义。
2. 图诊断（需要合并调用图的诊断）由离线引擎计算，severity 固定（Error/Warning），不受 Rust lint 属性影响。
3. #1 非零退出（编译错误或 deny 级直接 lint）时不合并图、不推断、不渲染图诊断，原样返回失败。
4. 第二次 Cargo 编译、offline emissions JSON、ack 回执、`RIVUS_OFFLINE_*` 有效传输**已删除**（阶段 5 完成）；`RIVUS_UNTESTED_PATHS` 的 payload/传输已在阶段 3 删除。旧变量名继续作为非法 driver authority 被拒绝并由父进程清理。图诊断由父进程渲染适配器从 artifact source 解析位置并渲染。

## 原则

- 每个阶段独立通过全部门禁（fmt/build/clippy/test/rivus check/git diff --check），可独立提交、可回滚。
- 不让现有 UI 快照大规模失效：ProjectCaps 直连模式保留为兼容/UI harness，不属于 `cargo rivus check` 的最终生产路径；真实 check pipeline 行为变化集中在阶段 2 与最终切换阶段。
- 图诊断固定等级是契约；迁移期内回放阶段暂时仍走 rustc lint 管线（见决策记录 D1）。
- 不新增布尔执行标志；执行策略用 enum 表达（见决策记录 D2）。

## 阶段清单与状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 固定设计边界：诊断类别、固定等级、短路规则写入理论文档与手册 | **完成** |
| 1 | 用 `LintExecutionMode` enum 替换 pass 内布尔三元组（行为保持） | **完成** |
| 2 | 直接 lint 移入采集编译；check #1 失败短路；#2 只回放图诊断 | **完成** |
| 3 | untested selection 在中间直接构造为图诊断，删除 #2 的 good/ok 重登记 | **完成** |
| 4 | #2 缩成纯 anchor replay（迁移期优化，不再继续投入） | **完成（临时路径，已在阶段 5 删除）** |
| 5 | 修正 artifact source resolver，直接切换父进程渲染并删除 #2 | **完成（2026-09-01）** |

## 阶段 2 补充（评审发现，完成）

- [x] **Issue 1（回归）**：覆盖候选登记丢失 Production scope 门控，UI/直连模式对 `mod tests` 内未调用 helper 误报 `rvs_untested_good_fn`。修复：pipeline 调用处加 `scope == DiagnosticScope::Production`；新增 UI fixture `437_test_module_helper_not_coverage_candidate_20260831.rs`（先复现失败，修复后 check-pass）。
- [x] **Issue 2（测试补齐）**：新增 `test_20260831_check_graph_error_fails_replay_phase`（static 读取 → 契约缺 S → #2 回放 Deny → Err(101)）；`测试策略` 中 `-D warnings` 项已标注按 D1 推迟。
- [x] **Issue 3（文案）**：短路消息按错误类别区分——`ExitCode(n)` 报"fix the diagnostics above"，spawn/Message 类失败报 "offline caps check unavailable"；顺带删除仅剩测试引用的 `rvs_callgraph_failure_exit_code` 及其旧测试（退出码透传已由 20260831 端到端测试覆盖）。
- 门禁：fmt/build/clippy(基线 6)/test 全过/`cargo rivus check` ok/git diff --check ok。

## 阶段 3 计划（已完成，2026-09-01）

目标：untested 选择不再经 `RIVUS_UNTESTED_PATHS`/JSON 传输与 #2 重登记，而是由中间直接构造为图诊断 emissions，与 contract/incomplete 等同通道回放。

- [x] `offline_caps`：`OfflineCapsLint` 增加 `UntestedGoodFn`/`UntestedOkFn`；新增 `rvs_untested_emissions(uncovered) -> Vec<OfflineCapsEmission>`（identity 锚点、消息格式与现有 `"{label} fn '{name}' not called by any test"` 完全一致，name 取 `def_path.rvs_fn_name_str()`）；`lints/mod.rs` 的 lint 映射补两臂。
- [x] 中间管线：`rvs_run_cargo_check_at_BIST` 把 untested emissions 追加到 report emissions 之后；`rvs_run_project_lints_BIST` 删除 uncovered 参数——check 管线**恒为 Offline 模式**（干净项目也是：emissions 可为空数组，避免退回 per-crate ProjectCaps 直连分析）；`CargoLintInput::ProjectCaps` 变体删除（`RunGenerationAnalysisMode::ProjectCaps` 保留，UI harness 仍用）。
- [x] marker：`RunGenerationAnalysisMode::Offline` 收为无字段变体（driver 强制要求 `RIVUS_OFFLINE_EMISSIONS` 路径）；schema 5→6；`tests/ui_tests.rs` 常量同步。
- [x] 传输删除：`RivusOfflineDriverInput.untested_paths`、`RIVUS_UNTESTED_PATHS` 作为有效输入的路径校验与消费、`rvs_write_untested_selection_BIST`、`artifacts.rs` 的 selection 序列化/解析与 `UntestedSelectionEntry`（`CoverageLabel` 保留）、`lint_driver.rs` 加载函数、`RivusLintConfig.untested_functions`、pass 字段与 check_crate 错误转发、`test_quality` 的 selected 分支。closed protocol 对旧变量的显式拒绝及父进程 `env_remove` 清洁保留。
- [x] 登记门控简化：`rvs_registers_coverage_candidates` 访问器删除，pipeline 调用处直接 `mode.rvs_is_caps_report() && scope == Production`（ReplayDiagnostics 不再登记）。
- [x] theory 文档"测试覆盖"一节：选择输入改为"转换为图诊断 emissions 在最终 rustc 阶段回放"。
- [x] 回归：UI untested fixtures（41/183/425/430 等）stderr 不变；20260831 端到端快照复核（warning-pipeline 项目含 untested good fn → #2 图 warning，退出 0 不变）。

- [x] 评审修复（2026-09-01）：schema 常量实际补齐 5→6（workspace.rs + ui_tests.rs，此前记录声称未执行）；三处过时注释更正（ports.rs ReplayDiagnostics 登记、rvs_register_coverage_candidate_BM 文档、offline_caps 前缀过滤理由）。
## 阶段 4 计划（已完成，2026-09-01）

目标：ReplayDiagnostics 进程只保留 emissions 锚定所需工作；其余收集职责删除。锚点编号与节点构建共享同一条 body 遍历与循环，不建立第二套遍历（避免编号漂移触发 D6 硬失败）。

- [x] `LintExecutionMode` 增加访问器 `rvs_collects_graph_nodes()`（仅 ReplayDiagnostics 为 false）。
- [x] `rvs_collect_callgraph_for_item_BMS` / `rvs_collect_callgraph_for_signature_BMS` 接收 mode：锚点路径（caller identity + 调用点 occurrence→span）恒执行；节点构建（facts、行数统计、unresolved test calls、coverage 标志、`rvs_merge_node_M`）仅当 collects_graph_nodes。
- [x] `check_crate`：~~test-harness 预扫描（test_fn_names）~~ **保留全模式运行**（D7：harness 剥掉 `#[test]` 属性，预扫描是 is_test 图事实的唯一可靠来源，不能门控）；`test_outputs` 目录加载收窄到 `rvs_should_emit_lints()` 模式（移至 lint_driver prepare）。
- [x] `check_crate_post`：ReplayDiagnostics 跳过 test_quality（其四个职责在 replay 全为 no-op：test_names 空、coverage 关、World 无输出）。
- [x] theory 构建过程补一句：回放阶段复用同一遍历收集锚点、不构建节点。
- [x] 回归：20260831 三个端到端测试（依赖 replay 锚定的 untested/contract emission ack 闭环即验证锚点正确性）+ UI 全套 + 下游 fixture。

教训见 D7（test-harness 预扫描是图事实来源，不是 lint 附属）——初版把预扫描门控在 should_emit_lints 上，破坏了采集编译的 is_test 事实与覆盖可达性，被 test_20260715_specialized_impls_keep_distinct_callgraph_identity 抓回，已撤销门控并加注释说明。
## 阶段 5 计划（最终切换，已完成 2026-09-01）

目标：不再制造新的迁移期协议。修正当前未提交的 resolver 草稿后，直接让父进程消费内存中的图诊断、解析 artifact source、渲染并决定退出码；同一交付中删除第二次 Cargo 编译及其 replay 协议。

### 完成记录

- [x] **resolver 回归测试先行**：`test_20260901_node_anchor_requires_exact_identity`（crate ID 不匹配先复现失败再修复）、`test_20260901_canonical_duplicate_sources_deduplicate`（绝对路径与 base+relative 规范化重复先复现再修复）；保留节点缺失、无 base、call-site 无 source 用例。
- [x] **修正 resolver**：节点锚点按完整 `FunctionIdentity` 验证（`node.crate_id` 精确匹配，mismatch 无位置）；规范化后经 `BTreeSet<DiagnosticLocation>` 去重排序；call-site 只用自带 source 不回退。emission 锚点 identity 全部来自合并节点 `crate_id`，exact-match 自洽。
- [x] **emission 携带 severity**：`OfflineCapsEmission` 增加 `severity` 字段（diagnostic.severity / untested 恒 Warning）；`rvs_emissions`、`rvs_untested_emissions` 同步。
- [x] **父进程渲染器**：新增 `src/environment/graph_render.rs`——`rvs_render_graph_emissions_BIS` 逐 emission 渲染 `{severity}[{code}]: {message}`，每锚点输出 `--> file:line:col (bytes s..e)` + 源码行 + caret（列号按字节计，非 ASCII 前缀行与 rustc 字符列可能有显示差异）；无位置按 def_path 呈现不丢弃；文件读取失败/范围越界降级为纯字节范围行。
- [x] **固定 severity 与退出码**：任一图 Error → `rivus check failed: N error(s), M warning(s)` + exit 101；纯 Warning → 成功并打印 `Offline Caps Check: ok`。`RUSTFLAGS=-Dwarnings` 无法升降图诊断（`test_20260901_check_graph_warning_not_escalated_by_deny_warnings`）。
- [x] **删除第二次编译与协议**：`rvs_run_project_lints_BIST`、`rvs_write_offline_emissions_BIST`、`rvs_verify_offline_emission_acks_BIS`、`rvs_merge_lint_results`、`CargoCheckMode::Lint`/`CargoLintInput`/`OfflineLintInput`/`OfflineEmissionInput`、`RivusDriverMode::Offline`/`RivusOfflineDriverInput`、`RunGenerationAnalysisMode::Offline`（marker schema 6→7，ui_tests 常量同步）、driver 协议 Offline arm、emissions JSON 序列化/解析/校验、ack 基础设施与 `LintEnvironment::rvs_acknowledge_offline_emission_P`。
- [x] **折叠临时 gate**：`ReplayDiagnostics` 变体、`rvs_collect_caps_facts`（恒 true）、`rvs_collects_graph_nodes`、`rvs_runs_test_quality` 一并删除；callgraph 收集器不再接收 mode 参数，`CargoCheckMode` 收敛为单变体 Callgraph。
- [x] **保留兼容 harness**：ProjectCaps/UI 直连模式与全部 UI 快照不变（`rvs_emit_offline_caps_diagnostics_S` 仅由 caps-report 模式调用）。
- [x] **收口 active 协议**：`RIVUS_OFFLINE_CAPS`/`RIVUS_OFFLINE_EMISSIONS`/`RIVUS_OFFLINE_EMISSIONS_ACK_DIR`/`RIVUS_UNTESTED_PATHS` 不再被设置或消费，仅保留在父进程 env_remove 清洁、closed-protocol 拒绝列表与 `--config` 危险键列表中（"拒绝旧输入"，非"仍在传输"）。
- [x] **验收测试**：
  - 单编译证明 `test_20260901_check_runs_single_cargo_compile`：build.rs 计数文件在完整 check 后恰为 1。
  - 图 Error `test_20260901_check_graph_error_renders_in_parent_fails_101`：采集成功、父进程渲染 `error[contract_mismatch]`（含期望名）、exit 101；取代原 replay-phase 测试。
  - 图 Warning 不升级（见上）；deny 短路、warning 完整管线既有测试保持。
  - resolver 单测 5 项 + 渲染快照（node 锚点、def_path fallback、line/col 纯函数）+ 下游 fixture 端到端（call-site 锚定、多锚点 emission、incomplete/unknown 诊断渲染正常）。
  - 测试调整：sanitize-env/absolute-paths/target-scope/typed-modes 改用 Callgraph 模式；删除 ack 必需性、emissions JSON round-trip、empty-anchor 拒绝、merge-lint-priority、defers-caps-to-parent 等仅服务已删机制的测试。
- [x] **文档**：theory 构建过程（单编译 + 父进程渲染）、渲染职责（exact identity、去重、def_path fallback）、测试覆盖通道、manual check 段落同步。
- 期间发现的测试基建事实：`cargo test --bin` 不重建 `target/debug/cargo-rivus` 主二进制，测试 spawn 的 wrapper 陈旧时会产生大范围假失败（marker schema 不匹配）；schema 变更后必须先 `cargo build`。

### 非目标（维持不动）

- 不为宏展开函数新增诊断 source，不提升 artifact schema（callgraph 仍为 v18；提升的只是 run-generation marker schema）。无 `FnSource` 的诊断按 def_path 展示；只有真实用户反馈证明该降级不可接受时再设计 expansion callsite。
- 不做 source digest。它只防止采集结束到渲染之间文件被并发修改的短窗口，后续按实际问题补充。
- 不预先设计多 target 候选选择策略。渲染所有去重后的精确候选；出现重复噪音后再基于证据收窄。
- 不实现 shadow mode；用回归、快照和真实 Cargo 端到端测试完成切换验证。

## 已完成记录

### 阶段 4（2026-09-01）

- 本阶段只优化迁移期 #2，不是最终架构依赖；其分支与门控已在阶段 5 随 #2 整体删除。
- `LintExecutionMode::rvs_collects_graph_nodes()`（仅 ReplayDiagnostics 为 false）；两个 callgraph 收集器接收 mode：锚点路径（caller identity + 调用点 occurrence→span，与节点路径共享同一条循环，无第二套编号）恒执行，节点构建（facts、行数统计、unresolved test calls、coverage 标志、`rvs_merge_node_M`）仅节点收集模式执行。
- `check_crate_post`：ReplayDiagnostics 跳过 test_quality（四个职责在 replay 全为 no-op）；`test_outputs` 目录加载收窄到 lint 发射模式（lint_driver prepare）。
- 预扫描保留全模式运行（D7）。
- theory 构建过程更新为"纯锚点回放"描述；下游 fixture 输出与阶段 3 完全一致。
- 门禁：fmt/build/clippy(基线 6)/test 全过/`cargo rivus check` ok/git diff --check ok。

### 阶段 3（2026-09-01）

- `offline_caps`：`OfflineCapsLint` 增 `UntestedGoodFn`/`UntestedOkFn`；`rvs_untested_emissions` 把 uncovered 选择转为 emissions（消息与历史格式逐字一致）；lint 映射补两臂；新增 converter 快照单测。
- 中间管线：untested emissions 追加到 report emissions 之后；`rvs_run_project_lints_BIST` 收窄为 emissions-only 且 **check 恒为 Offline 模式**（干净项目也回放空 emissions，避免退回 per-crate 直连分析）；`CargoLintInput::ProjectCaps` 编排变体删除（`RunGenerationAnalysisMode::ProjectCaps` 保留供 UI harness）。
- marker：`Offline` 变体收为无字段（driver 强制要求 emissions/ack 路径）；schema 5→6；ui_tests 常量同步。
- 传输删除：`RivusOfflineDriverInput.untested_paths`、`RIVUS_UNTESTED_PATHS` 设置（拒绝与 env_remove 清洁保留）、selection JSON 序列化/解析与 `UntestedSelectionEntry`、`RivusLintConfig.untested_functions`、pass 字段与错误转发、`test_quality` selected 分支；登记门控收敛为 `mode.rvs_is_caps_report()`。
- 测试调整：删除 direct-driver coverage 测试（UI 套件覆盖同一行为）；caps 校验三连改为 `rvs_run_cargo_check_at_BIST` 快速失败断言；target-scope/sanitize-env/absolute-paths 改用 Offline 模式（绝对路径断言从 RIVUS_CAPSMAP 改为 RIVUS_OFFLINE_EMISSIONS）；typed-modes 协议快照去掉 project_lint 行与 untested 列。
- 文档：theory 构建过程（lint-bearing 采集 + 回放阶段）与测试覆盖（选择→emissions）两处更新；manual check 段落同步。
- 端到端验证：本仓库与下游 fixture 输出与阶段 2 完全一致（直接 lint 在 #1、图诊断在 #2）；lib 与 lib-test 双单元的同名诊断由 cargo 既有去重处理（与改造前一致）。
- 门禁：fmt/build/clippy(基线 6)/test 全过/`cargo rivus check` ok/git diff --check ok。

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

- **D1（等级迁移缺口）**：图诊断固定等级自阶段 0 起是 `cargo rivus check` 的文档契约；但阶段 2 到最终切换前仍通过 rustc replay lint 管线发射（受 `-D warnings` 影响），阶段 5 的父进程渲染必须完全落地固定等级。ProjectCaps/UI 直连模式作为显式兼容 harness 保留，不是生产 check 路径。
- **D2（enum 切分）**：`LintExecutionMode` 的 `CheckAndCollect` 变体在阶段 2 接线时才引入（阶段 1 仅收拢现有三种模式），避免 dead variant。collection generation 的"目的"enum（检查采集 vs 静默分析采集）同样推迟到阶段 2，因为它属于 marker schema 变更。
- **D3（短路范围）**：#1 任何非零退出都短路（不区分 deny 与编译错误），现有 collection 失败时的 ProjectCaps fallback 在阶段 2 取消。
- **D4（登记简化）**：`rvs_collect_caps_facts` 收拢为恒 true——原 Offline 模式在 emissions 为空时跳过图收集的分支被删除；该差异仅影响进程内无用的工作量，不产生输出差异（artifact 写出仅在配置了 callgraph_output 时生效，#2 无输出目录）。该恒 true 包装不单独清理，阶段 5 删除 ReplayDiagnostics 时一起折叠。
- **D5（迁移期 builtin warning 重复，已知回归）**：#1（Check 采集）与 #2（回放）都是 Normal lint level、独立 target 目录各自全新编译，rustc 内建 warning（如 unused variable）会在两遍各出现一次。Rivus 自身诊断无重复（模式门控）。不为迁移期增加 `-Awarnings` 或 dcx workaround；阶段 5 删除 #2 时自然消除。
- **D6（历史 replay fail-closed）**：untested 转为 emissions 后，锚定 identity 在回放编译中找不到匹配时经 ack 校验使整个 check 失败（旧路径静默跳过无匹配的选择项）。阶段 5 删除 replay/ack 后不保留这一临时协议语义：resolver 必须 exact-match identity；无法解析位置的有效图诊断按 def_path 渲染，不得静默丢弃，也不因缺少源码位置把语义诊断改成工具失败。
- **D7（test-harness 预扫描是图事实来源）**：rustc test harness 会剥掉 `#[test]` 属性并改写为 `RustcTestMarker`，因此 `test_fn_names` 预扫描是测试身份（`node.is_test` → 覆盖可达性种子）的可靠来源，与 lint 发射无关，任何执行模式都不能跳过。阶段 4 曾误将其门控在 lint 发射上，导致采集图覆盖判定全断。
- **D8（锚点范围的有意差异）**：#2 的 rustc 锚定取函数整条 span（`subject.span`），artifact `FnSource` 记录函数名 ident 范围（写回源码需要）。外部渲染器将锚定名称范围（对高亮更精确）；artifact smoke test 对照源码 ground truth（文件字节与函数名文本），不声称与 rustc span 逐字节一致。

## 测试策略

1. 阶段 0–1 已通过现有全部测试与快照验证行为保持。
2. 阶段 2 已增加 deny 短路、warning 不短路和图 Error 退出码 fixture；`-D warnings` 不升级图 Warning 推迟到阶段 5 的固定 severity 渲染验证。
3. 阶段 3 已通过 converter 快照与真实 check pipeline 验证 untested 进入统一 emissions；旧变量拒绝测试继续覆盖 closed protocol，不把它误记为仍在传输 payload。
4. 阶段 4 只由现有 replay ack 端到端路径覆盖，不再新增专用测试，因为该路径将在阶段 5 删除。
5. 阶段 5 已按上文"完成记录"一次完成 resolver 修正、外部渲染、单 Cargo 编译证明、固定 severity 和 replay 协议删除；ProjectCaps/UI harness 保持，未做全量快照迁移工程。
