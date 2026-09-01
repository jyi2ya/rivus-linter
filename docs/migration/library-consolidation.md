# Library Consolidation — 2026-09-01

**状态：已完成**（门禁全绿：fmt / build / clippy 基线 5 / test 550+UI 3 / rivus check ok / diff --check）

## 结果

用三个成熟外部库替换自维护代码，净 -89 行（+841/-930，含测试与快照）：

1. **annotate-snippets 0.11**（graph_render.rs）
   - 头行 `{severity}[{lint}]: {message}` 原样保留（`Level.title().id()` 原生支持）
   - `(bytes a..b)` 移入 annotation label
   - 新增能力：多行 span 下划线、上下文行、char 列号（旧实现是字节列号，
     多字节行会错位）、warning 用 `---` 下划线
   - 非 UTF-8 / 不可读源文件降级为无 snippet 的定位行（新测试覆盖）
   - 删除手写 line/col 扫描器与 caret 渲染（约 -80 行生产代码）

2. **cargo_metadata 0.21**（cargo_targets.rs + workspace.rs 溯源）
   - 新共享入口 `rvs_cargo_metadata_primary_package_BIS`：一次
     `cargo metadata --no-deps` 同时服务本地前缀检测与 crate 溯源权威文件
   - 删除对 cargo target 自动发现的重实现：manifest 名字表、
     autobins/autotests/autoexamples/autobenches 解析、
     tests/examples/benches/src/bin 目录扫描（cargo_targets.rs 270→175 行）
   - 前缀语义保持：包名恒插入；Production scope 排除 test/example/bench；
     custom-build（build_script_build）恒排除；溯源集合行为不变
   - 行为变化（有意）：manifest 错误消息来自 cargo（更准）；
     infer_std/capsmap 的 "Cargo.toml: Cargo.toml:" 消息叠加 bug 顺带修复；
     零 target 的 manifest 在检测期即被 cargo 拒绝（旧实现会在 cargo check
     期才失败）
   - 已实验确认（2026-09-01 cargo）：lib target 名已规范化（`my-pkg`→`my_pkg`）、
     bin 名不规范化、虚拟 workspace 根不出现在 packages、
     `--no-deps` 不写 Cargo.lock

3. **tempfile::TempDir**（test_support.rs）
   - `TestTempDir` 薄包装（tempfile 3.27 的 TempDir 只有 `AsRef<Path>`，无 Deref），
     Deref 到 Path 保持全部 140 个调用点不变
   - 失败路径自动清理；唯一命名（`rivus-{tag}-` 前缀 + 随机后缀）
   - 能力命名按真实语义更新为 `_BIST`（fastrand 线程局部 RNG 使 T 真实成立）；
     `tempfile::Builder::tempdir` 为结构性 ghost（仅测试代码调用，
     infer-capsmap 只推断 Production targets），按手册在 caps/ext 手工标定
     BIST complete 并注明依据

## 途中决策记录

- 渲染器消息头曾误将 warning 写成 `Level::Error`——测试首跑即捕获，已修复
- clippy 新增 `contains()` 建议（TargetKind 判定）已采纳，回到基线 5 条
- 4 个纯 TOML 解析测试重写为真实 fixture + cargo metadata；5 个 FS 扫描器
  错误路径测试删除（扫描器已不存在，语义归 cargo）
- setup 的项目校验改走 cargo metadata（与编辑用的 toml_edit 模型解耦）

## 观测（与本变更无关，供后续跟进）

- `target/rivus-callgraph-std.json` 在本会话开始前已缺失 → infer-capsmap
  退化到 11 分钟（无缓存时每次 build-std）。如需恢复可运行
  `cargo rivus infer-std -o caps/std`（注意会重新生成 caps/std 内容）
- `target/` 下存在历史遗留 `rivus-relative-project-*` 测试垃圾目录
  （08-30 起累积），值得排查来源

## 非目标（未做）

- petgraph / insta / fs4 / fd-lock（调查报告已记录排除理由）
- 未重生成 caps/std（无 std 层新增 unknown）
