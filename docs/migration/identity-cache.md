# 采集 pass identity memoization

工作文档。每次代码变更前先在此更新已完成工作与下一步计划，再开始实现。

## 背景与目标

`infer-std` / `infer-capsmap` 的 collection 编译阶段占 ~22m50s（分析仅 ~10s）。perf 采样显示 rustc 进程内 Rivus lint pass 占 ~40%，热点全部是 identity 字符串构造（`rvs_def_path_B`、`rvs_impl_type_identity_B`、`rvs_span_source_identity`、generated-definition identity/marker），且 `rvs_generated_definition_repetition_ordinal_B` 对每个宏生成定义遍历全 crate owners，是 O(generated × owners) 二次开销。详见 `~/var/linter-issues/2026-08-27-infer-collection-pass-structural-slowness.md`。

目标：在 lint pass 内为确定性 identity 构造加 memoization，消除二次遍历，使 infer 命令的编译期开销显著下降。

### 关键约束

- `TyCtxt` 在 LateLintPass 期间不可变，rustc 进程 = 单 crate；cache 生命周期 = 一个 `RivusLintPass`，无失效问题。
- 不使用 static/thread_local/Arc/Mutex；不跨 crate、跨进程复用。
- negative 结果（dummy span、跨文件、非 generated）同样缓存。
- artifact schema v18、run-generation marker schema 7 均不变；输出（FnGraph JSON、诊断、UI 快照）语义完全不变。
- 不做 TTL/LRU/淘汰；键空间被 crate 定义数量天然界定。

### 库选型结论（调查已完成）

不引入 moka/mini-moka/lru/cached/quick_cache：并发、淘汰、TTL、static memoization 均与"单 owner `&mut self`、有限键空间、随 pass 释放"不匹配。采用 `rustc-hash` 2.x（lockfile 已传递包含 2.1.2），直接依赖声明即可；多个 map 封装进领域结构 `IdentityCache`。

## 阶段计划与状态

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 理论文档 + 性能基线 | **完成** |
| 1 | `IdentityCache` + ordinal 单遍预索引 + 调用链接入（合并实现） | **完成** |
| 2 | 正确性测试（分组纯函数、输出逐字节不变） | **完成** |
| 3 | 性能对比 + 完整门禁 + 汇报 | **完成（2026-09-01）** |

### 性能结果

| 指标 | before | after |
|---|---|---|
| `infer-capsmap` wall time（本仓库，release） | 11m16.463s | **57.937s（11.7×）** |
| CPU user | 15m34s | 1m58s（~8×） |
| 输出 caps/deps | — | **与 before 逐字节一致** |

下游 fixture `cargo rivus check` 正常（ok，exit 0）。收益来源与 2026-08-27 perf 调查的根因吻合：宏生成 item 极多的依赖 crate（ra_ap_*）上，per-definition 的全 crate ordinal 重扫与重复 identity 字符串构造是支配成本。

### 阶段重排说明

原计划的"阶段 1 ordinal 先行、阶段 2 再建 cache"不可行——ordinal 预索引需要跨调用存活的状态，其宿主就是 `IdentityCache`；而接入调用链的签名改造是一次性成本，分两批做会重复改动同一批函数。故合并为阶段 1。

### 非目标

- 不做跨运行的 Cargo/build-std 持久缓存：Cargo fresh 命中不调用 wrapper、无本轮 artifact，需要独立的 compilation-unit key 与 freshness 设计，单独立项。
- 不做字符串 interning（纯计算缓存已足够）。
- 不改 caps 层、推断引擎、渲染器。

## 阶段 0 记录（完成）

- 理论：`docs/theory/compiler-identity-cache.md`（生命周期、确定性、negative caching、无失效的依据）。
- 基线：release 构建，本仓库 `cargo rivus infer-capsmap -o /tmp/rivus-perf/before`（AllCrates 模式，编全部依赖含 ra_ap_*）：**11m16.463s**，输出留存作逐字节对比基准。
- 历史参照（loopline-ng，2026-08-27）：infer-std ~23m，编译段占 ~22m50s。

## 阶段 1 记录（完成）

- 新增 `src/lints/identity_cache.rs`：`IdentityCache`（`def_paths` / `impl_types` / `span_sources` / `generated_bases` FxHashMap + 惰性 `generated_ordinals` 索引）、`ImplTypeIdentity` 值类型、纯函数 `rvs_assign_repetition_ordinals`（按 (kind, base) 分组、owner 顺序、组>1 才有 ordinal）。
- `Cargo.toml` 增加 `rustc-hash = "2"`。
- utils.rs：`rvs_def_path_B` → `rvs_compute_def_path_BM` 等 compute 函数接收 `&mut IdentityCache`；删除 `rvs_generated_definition_repetition_ordinal_B`（per-call 全 crate 扫描），由单遍索引替代；`rvs_span_source_identity` → `rvs_compute_span_source_identity`。
- 线程化：`RivusLintPass.identity_cache` 字段；`FnCheckData.identity_cache`；`rvs_collect_body_facts_BM` / `rvs_resolve_call_BM` / `rvs_diagnostic_scope_BM` / `rvs_resolved_target_BM` / callgraph 收集器 +cache 参数。
- **自举抓到 3 处命名不符**（`rvs_span_source_identity_BM`→`_M`、`rvs_assign_repetition_ordinals_M`→无后缀、`rvs_compute_span_source_identity_B`→无后缀），修正后 `cargo rivus check` ok。期间确认：def_path↔impl_type 互调环与 generated-identity 链的 B 由调用边推断支撑，属自洽命名簇。

## 阶段 2 记录（完成）

- `test_20260901_repetition_ordinals_follow_owner_order`（owner 顺序 0/1/2、单例无 ordinal）
- `test_20260901_repetition_ordinals_group_by_full_key`（同 base 不同 kind 不分组）
- 全量 554 单测 + UI 3 + 自检 + 下游 fixture 通过；clippy 基线 6 不变。

## 阶段 3 记录（完成）

- 性能对比见上表；`git diff --check` 通过；全部交付门禁通过。
