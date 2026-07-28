# Cargo Rivus 工具手册

`cargo rivus` 是基于 rustc-driver 的 lint 插件，通过编译器的 `LateLintPass` 在 HIR 层面分析代码。

项目 crate 名为 `rivus-linter`，安装后的二进制名为 `cargo-rivus`，作为 cargo 子命令使用：`cargo rivus <subcommand>`。

**`cargo rivus`（无子命令）默认运行 `check`。**

---

## 开发状态与可疑诊断

`cargo rivus` 仍在积极开发，存在许多已知和未知 bug，也可能误报或漏报。

> **给 LLM 的强制规则**：如果你认为某个诊断、能力传播、推断结果或工具失败可能是 linter 的 bug，立即停止当前工作并向人类汇报，等待进一步决定。不要修改业务代码来迎合可疑诊断，也不要通过改名、添加能力后缀、修改 capsmap、添加 `allow`/`suppress`、降低检查级别、重构或其他 workaround 绕过它。

---

## 命令一览

| 命令 | 用途 |
|------|------|
| `cargo rivus check` | 检查 `rvs_` 函数调用链能力合规性（默认） |
| `cargo rivus report` | 统计项目能力分布和契约不一致摘要，输出好函数率 |
| `cargo rivus infer-capsmap` | 从项目 `caps/` 推断 direct external deps，并写到 `-o` 指定路径 |
| `cargo rivus infer-std` | 推断标准库函数能力标注并写到 `-o` 指定路径（需 nightly） |
| `cargo rivus migrate-caps` | 将项目 capsmap v1 目录原子迁移为 v2 JSON Lines |
| `cargo rivus setup` | 为新项目注入 AGENTS.md 和 clippy lint |
| `cargo rivus strip` | 移除所有 `rvs_` 前缀和能力后缀 |
| `cargo rivus annotate` | 推断能力并添加 `rvs_` 前缀和后缀 |
| `cargo rivus why` | 显示某函数为何具有当前能力（列出被调用方及其能力） |
| `cargo rivus usage` | 显示本手册 |

---

## 开发工作流

使用 `annotate` 和 `strip`，你可以采用以下工作流：

```
编程 → annotate → 重构 → strip → 提交
```

1. **编程**：按 Rivus Style 编写代码，暂时不写 `rvs_` 前缀和能力后缀
2. **annotate**：运行 `cargo rivus annotate`，工具自动推断能力并添加 `rvs_` 前缀和后缀
3. **重构**：在能力标记的辅助下重构——标记会暴露调用链中的能力冲突，帮助你分离纯函数和副作用
4. **strip**：重构完成后运行 `cargo rivus strip`，移除所有 `rvs_` 前缀和后缀
5. **提交**：提交干净的代码

这样，能力标记只在重构过程中作为临时的"脚手架"存在，不会留在最终代码中。

---

## `cargo rivus check [OPTIONS] [ARGS]`

基于 rustc-driver 的 HIR 分析。第一阶段只为工作区 crate 收集全项目函数图，第二阶段运行非能力 HIR lint 并按合并后的覆盖结果发出测试质量诊断，最后由统一的离线能力引擎检查命名契约、后缀、静态状态和调用链合规性。第三方依赖不重新启用其编译 warning，而是通过本地调用边和项目 `caps/` 参与分析。

```bash
cargo rivus check                    # 使用项目 caps/（若存在）
cargo rivus check -- --features foo  # 传递额外 cargo check 参数
```

`check` 必须从待分析 package 的目录运行，不接受 `--workspace`、`--all`、`--package`/`-p` 或 `--exclude` 等 workspace package 选择参数；否则本地 crate 分类会与 Cargo 实际选择范围不一致。`--target-dir` 也不能透传，因为每次命令都会在项目 `target/.rivus-runs/` 下预留自己的隔离 target 和 artifact generation，并发命令不会清理或读取彼此的中间文件。其他不会覆盖 driver 环境或项目路径的 Cargo 参数可继续透传。

注意：capsmap 只从项目 `caps/` 目录加载，且每个 layer 必须使用带 `# rivus-caps-v2` 版本头的 v2 JSON Lines。CLI 不再支持 `-m/--capsmap`，也不再读取或生成 `target/rivus-std-capsmap.txt`、`target/rivus-inferred-capsmap.txt`、`target/rivus-deps-capsmap.txt`、`target/rivus-effective-capsmap/`。目录不存在时统一能力引擎使用空 capsmap。caps 目录使用统一的层级加载器（`CapsMap::rvs_load_dir_BIS`），按 `std → deps → seed → suppress → ext → 其余字母序` 的固定顺序合并；原子写入遗留的 `.层名.PID.序号.tmp` 临时文件会被忽略。


注意：`check` 默认编译 `--all-targets`，因此测试、示例和 benchmark 中的函数也会被分析。未测试 good/ok 函数在所有 target 的 callgraph 合并后统一判断；从 unit test 或 integration test 沿测试编译实际产生的调用边可达即视为覆盖，不要求测试直接调用每个 helper，`cfg(test)` 改写函数体时也不会误用 production 调用边。覆盖身份同时包含稳定 crate ID 和 `DefPath`，因此同名 library/binary target 不会互相借用覆盖；可执行入口身份也按稳定 crate ID 保存，因此 library 中与 binary `main` 共享 `DefPath` 的普通函数仍接受一般契约检查，而 binary 入口继续获得入口豁免。无法解析的同名调用只有在候选唯一时才作为回退，局部 closure 或函数指针 binding 不参与该回退。不同本地 target 若产生相同 `DefPath`，能力分析对兼容角色的事实和调用边取保守并集，报告仍按不同源码定义分别统计函数数和有效行数；Port/普通函数、test/production 或 trait impl/普通函数等不兼容角色不会被静默合并。测试覆盖候选和 report 排除项按每个 target 定义记录，不会因另一个同路径定义带有 `allow(dead_code)` 而整体消失。结果在最终 rustc lint 阶段发出，因此函数、参数、statement、expression、field、模块和 crate 上的 `allow`/`expect`/`deny` 等级仍然生效；中间 artifact 收集不会因任一 HIR 作用域中的 Rivus expectation 或 `forbid(unfulfilled_lint_expectations)` 提前失败。任一 target 编译失败时不会根据部分图输出覆盖结论。直接 rustc/UI 模式使用当前 crate 内存图做传递可达性判断，并采用相同的稳定 crate ID 和唯一名称回退规则。`infer-capsmap` 和 `infer-std` 只编译 production targets。

退出码：`check` 成功时返回 `0`；失败时透传底层 `cargo check` 的退出码。其他子命令成功时返回 `0`，工具自身运行失败时返回 `2`。warning 不影响退出码。

---

## `cargo rivus report [PATH]`

对 `path` 指定的 Cargo 项目运行 `cargo check`，统计编译过程中发现的所有 `rvs_` 函数的能力分布，输出各能力标记的函数数量和行数占比。`good`（能力集合是 `{A,B,M}` 的子集，包括纯函数）和 `ok`（能力集合是 `{A,B,M,P}` 的子集）应尽量占比高。

能力分布和末尾的 `Contract Mismatches` / `Sample Mismatches` 都来自同一次 fresh callgraph 收集；callgraph 构建失败时 report 整体失败。

`PATH` 最好直接指向目标 Cargo 项目的根目录；如果它不是包含 `Cargo.toml` 的项目根目录，命令会失败。

注意：`report` 产生的中间输出目录按当前工作目录与 `PATH` 组合计算；使用绝对路径时应特别小心，最好在目标项目目录中运行或直接传 `.`。

```bash
cargo rivus report           # 当前目录
cargo rivus report /path/to  # 指定目录
```

**报告中的百分比和柱状图均基于行数占比，而非函数数量占比。** 行数统计只计入函数体内部的有效代码行（去除函数签名、大括号、空行和注释），因此更能反映真实的代码逻辑量。这里的"有效代码行"基于源代码片段扫描得到，是一种近似统计而非语义级精确计数。优化方向是减少非好函数的代码行数——将逻辑从高能力函数抽出到低能力/纯函数中。

**严禁注水**：为了提高好函数率而注入无实际业务价值的纯函数是被禁止的。好函数率的提升必须来自有意义的重构。

**以下函数被排除在统计之外**：`#[test]` 函数、只在 test compilation 中存在的 helper，以及 `#[allow(dead_code)]` 或 `#[allow(unused)]` 标记的函数。

契约不一致摘要使用 callgraph 和本地 crate 前缀过滤，不参与能力分布的行数统计。

示例输出：

```
Capability Report
------------------------------------------------------------
Total: 42 functions, 890 lines
------------------------------------------------------------
  (good)          30 fns    650 lines  73.0% |██████████████████████░░░░░░░░|
  (ok)            30 fns    650 lines  73.0% |██████████████████████░░░░░░░░|
  (pure)          12 fns    200 lines  22.5% |██████████░░░░░░░░░░░░░░░░░░░░|
  M(Mutable)      10 fns    300 lines  33.7% |█████████████░░░░░░░░░░░░░░░░░|
```

---

## `cargo rivus infer-capsmap [OPTIONS] [PATH]`

收集调用图并从种子标注自底向上推断 capsmap。对每个 `rvs_` 函数，聚合其所有被调用方的能力，得到推断结果。`PATH` 必须是一个可成功执行 `cargo check` 的本地 crate 项目；仅含 `[workspace]` 的虚拟根目录不受支持。

推断分两步：首先对不在 capsmap 精确边界中的函数，直接从行为特征推断能力（`async fn` → A、`unsafe fn` → U、`&mut` 参数 → M、`static` 引用 → S、`static mut` 引用 → S+U、`thread_local!` 引用 → S+T）；然后通过固定点迭代，将所有被调用方的传播能力沿调用图向上传播。若同一函数同时被识别为普通 `static` 引用和 `thread_local!` 引用，结果会合并为 `S+T`（幂等）。capsmap 精确条目是冻结的权威边界，不继续吸收其内部调用的能力。

对于普通非 Port trait 方法，公开能力由各 impl 按传播能力逐项做 at-least-half vote（阈值为 `ceil(n/2)`）决定；trait 声明自身写的后缀只在没有 impl 可聚合时作为回退。因此 2 个 impl 中 1 个带能力会被抬升，3 个 impl 中仅 1 个带能力不会被抬升。Port trait 方法例外：公开能力固定为 `P`，不受 impl 实际行为影响。这一规则同样会影响 `annotate` 和 `why` 的显示结果。

投票会保留参与实现数、阈值和逐能力票数。完整、可修改的本地实现若拥有投票未选中的传播能力，会产生 `TraitImplOutlierWarning`；该 warning 不改变投票结果和 capability totals。`why` 会显示 trait vote 详情和具体实现的 contribution/outlier caps，report 会列出最多十个本地 outlier 样本。

```bash
cargo rivus infer-capsmap -o caps/deps       # 从项目 caps/ 推断，并把 direct external deps 写到指定文件
```

选项：
- `-o, --output <PATH>` — **必填**。direct external deps capsmap 输出路径；相对路径按目标项目目录解析。通常写到 `caps/deps`。命令只写这个显式输出，不再写入 `target/rivus-inferred-capsmap.txt` 或 `target/rivus-deps-capsmap.txt`。

注意：种子始终从项目 `caps/` 加载（排除 `deps` 层，避免旧 deps 干扰重新推断）。如果 `-o` 指向 `caps/` 下的其他自定义层，该输出文件也会从本次种子中排除，避免旧输出影响重新生成。命令不会允许输出覆盖 `std`、`seed`、`suppress` 或 `ext` 等其他保留层。首次运行时允许 `caps/` 不存在，按空种子推断并创建 `-o` 指定输出的父目录。


---

## `cargo rivus infer-std [OPTIONS] [PATH]`

通过 `-Zbuild-std` 编译 std/core/alloc，推断标准库函数的能力标注。需要 nightly Rust；命令实际会设置 `RUSTUP_TOOLCHAIN=nightly` 并运行 `cargo check -Zbuild-std=std,core,alloc`，如果本机没有可用的 nightly toolchain 会直接失败。`PATH` 必须是一个有效的本地 crate 项目；仅含 `[workspace]` 的虚拟根目录不受支持。

注意：该命令只会从 `PATH/caps` 加载 `seed` 和 `suppress` 文件（不加载 `std`/`deps`/`ext`，因为那些是上一次生成的结果，会干扰重新生成），并在其基础上推断标准库条目。命令不会允许输出覆盖 `deps`、`seed`、`suppress` 或 `ext` 等其他保留层；如果没有完整收集到非本地 `std`、`core`、`alloc` 三个 crate，也会报错并保留原输出。成功写出 caps 后，命令会把本次完整合并的 versioned 函数图原子发布到 `target/rivus-callgraph-std.json`，供后续 std `why` 使用；任一前置步骤失败都保留上一个缓存。旧版 `target/rivus-callgraph-std/` 目录仍可只读加载。

```bash
cargo rivus infer-std -o caps/std        # 将 std caps 写到指定文件（通常 caps/std）
```

选项：
- `-o, --output <PATH>` — **必填**。std capsmap 输出路径；相对路径按目标项目目录解析。通常写到 `caps/std`。命令只写这个显式输出，不再写入 `target/rivus-std-capsmap.txt`。


---

## `cargo rivus migrate-caps [PATH]`

将 `PATH/caps` 中的旧 v1 `path=CAPS` layer 转换为 capsmap v2 JSON Lines。普通 loader 不支持 v1；只有该显式迁移命令包含隔离的旧格式 reader。

```bash
cargo rivus migrate-caps .
```

迁移先读取并转换全部 layer，在 `caps` 的同级 staging 目录写出 v2 文件，并通过不跟随 symlink 的文件描述符把原目录及每个同名 layer 的权限复制到 v2 对象，再用生产 v2 loader 验证每个文件和最终有效能力映射。确认迁移前后路径、能力和层级覆盖结果一致后，原子交换 `caps` 与 staging。交换出的 v1 树会先同步每个 layer 和目录本身，再改名为 `caps.v1-backup` 并完成父目录持久化；中断恢复无论从 staging 还是已发布 backup 继续，也会先完成同样的同步。备份发布失败时会尝试原子交换回原目录；如果该二次交换也失败，错误会报告仍保存原 v1 目录的 staging 路径，而已发布的 v2 目录保留在 `caps`。

迁移要求平台和底层文件系统支持原子目录交换以及 no-replace rename。不支持时命令会报错、清理 staging 并保留原 `caps`。

同一项目的迁移、`infer-std` 和 `infer-capsmap` 通过项目根目录下的 `.rivus-caps.lock` 串行化。该 regular file 持久保留；进程内 registry 排斥同进程线程，POSIX record lock 排斥其他进程且不会被 fork 后尚未 exec 的子进程继承。迁移在交换前还会重新读取 v1 layers，检测未遵守该锁的并发修改；维护者仍不应在迁移期间手工编辑 `caps/`。若进程在目录交换后中断，下次迁移会读取 active v2 目录中的 transaction marker，并验证 marker 指向的 v1 staging 或已经发布的 `caps.v1-backup` 与 active v2 的每个 layer 语义一致，再完成备份发布或清除 marker。staging 与 backup 同时存在、同时缺失或内容不匹配时拒绝猜测。

- `caps` 必须是真实目录，不能是 symlink
- `caps.v1-backup` 已存在时拒绝覆盖；仅当 active v2 的 transaction marker 能证明它就是当前迁移已发布且逐层语义一致的原 v1 目录时，重试会清除 marker 并成功结束
- `caps/` 中每个 layer 必须是 regular file；symlink、子目录和其他非文件条目会被拒绝，避免迁移时静默丢失目录内容
- 任一 v1 layer 无效时不修改原目录
- 成功后保留 `caps.v1-backup`，由维护者确认后自行处理

---

## `cargo rivus setup <path>`

将 `rivus.md` 复制为目标项目的 `AGENTS.md`，并在 `Cargo.toml` 中注入 clippy lint 规则。`<path>` 应当是一个包含 `Cargo.toml` 且至少定义了一个本地 crate target 的现有目录。

注意：命令会先确认目标目录包含可读取的 `Cargo.toml`，然后再覆盖写入 `AGENTS.md` 并修改 `Cargo.toml`。

- 如果目标项目已有部分 clippy lint，只注入不存在的条目
- 已存在的 lint 值不会被覆盖
- `AGENTS.md` 每次覆盖写入（确保与最新 `rivus.md` 同步）

注入的 clippy lint 分为以下几类：
- **防 panic**：`string_slice`、`indexing_slicing`、`unwrap_used`、`panic`、`todo` 等
- **防静默故障**：`let_underscore_future`、`let_underscore_must_use`、`unused_result_ok`、`map_err_ignore` 等
- **async 安全**：`await_holding_lock`、`await_holding_refcell_ref`、`large_futures`
- **内存安全**：`mem_forget`、`undocumented_unsafe_blocks`、`multiple_unsafe_ops_per_block` 等
- **数值正确性**：`float_cmp`、`float_cmp_const`、`cast_sign_loss`、`invalid_upcast_comparisons` 等
- **杂项**：`rc_mutex`、`debug_assert_with_mut_call`、`dbg_macro`、`allow_attributes` 等
- **spawn 的识别**：linter 通过内置的 spawn 函数路径列表自动识别 spawn 调用并发出 `SpawnWarning`。spawn 函数的能力由 callgraph 推断，不需要手动注入 capsmap

```bash
cargo rivus setup .           # 当前目录
cargo rivus setup /path/to/project  # 指定目录
```

---

## `cargo rivus strip [PATH]`

使用 rust-analyzer 的语义分析引擎，将项目中所有 `rvs_` 函数的 `rvs_` 前缀和能力后缀移除。正确更新所有引用点（包括 trait 定义、impl 块、调用点等）。

```bash
cargo rivus strip           # 当前目录
cargo rivus strip /path/to  # 指定目录
```

示例：`rvs_write_db_ABI` → `write_db`，`rvs_add` → `add`

注意：
- 需要项目能成功 `cargo check`（ra 需要加载完整 workspace）
- strip 只把普通 package target 的源码作为直接重命名候选；`tests/`、`examples/`、`benches/` 下的独立 Cargo target 暂不作为直接候选处理，以避免 integration test 多 crate 复用同一文件时产生 partial rename
- 如果 strip 后产生同名冲突（如 `rvs_add_M` 和 `rvs_add_ABIS` 都变成 `add`），rename 可能失败并输出警告
- 宏展开中的引用处理为 best-effort，建议 strip 后运行 `cargo check` 验证

---

## `cargo rivus annotate [PATH]`

对项目中所有可定位、受契约检查的本地函数进行能力推断，然后添加 `rvs_` 前缀和能力后缀。使用 rust-analyzer 的语义分析引擎进行重命名。`PATH` 必须指向本地 crate 项目；仅含 `[workspace]` 的虚拟根目录不受支持。

```bash
cargo rivus annotate           # 当前目录
cargo rivus annotate /path/to  # 指定目录
```

注意：
- 需要项目能成功 `cargo check`
- annotate 只基于普通 `cargo check` 范围收集 callgraph 和候选；`tests/`、`examples/`、`benches/` 下的独立 Cargo target 暂不参与能力推断和直接重命名。`src/` 中的单元测试源码不按路径排除，但是否进入候选取决于普通 `cargo check` 是否编译到对应代码
- rustc 选中的可执行入口、测试函数、trait impl 方法、synthetic 节点、宏展开或其他没有真实源码位置的函数不会作为直接 annotate 候选；库 crate 中普通的根级 `main` 仍按一般函数处理，trait impl 方法可能会随 trait 声明或调用点的语义重命名被间接更新
- 本地非 Port trait 方法的能力由各 impl 按 at-least-half vote（`ceil(n/2)`）聚合；声明后缀只在没有 impl 可聚合时作为回退。Port 方法固定为 `P`
- annotate 后 `#[serde(default = "...")]` 等字符串字面量中的函数引用不会自动更新，需要手动修复
- annotate 会删除 `target/rivus-callgraph-std.json` 及旧版 `target/rivus-callgraph`、`target/rivus-callgraph-std` 缓存（函数名已变，旧缓存失效），不会删除其他正在运行命令的 `target/.rivus-runs/` generation

---

## capsmap

为非 `rvs_` 函数声明能力。**只支持项目根目录下的 `caps/` 目录和 capsmap v2 JSON Lines**：

```
caps/
├── seed      # 手动维护的底层基线（分配、I/O 内部、编译器内部、async 展开等）
├── std       # std/core/alloc 的全量条目（`cargo rivus infer-std -o caps/std`）
├── deps      # 第三方依赖条目（`cargo rivus infer-capsmap -o caps/deps`）
├── suppress  # 修正条目（覆盖 std/deps 中过宽的能力标记）
└── ext       # 手工修正或无法自动推导的精确条目（标准层中最高优先级）
```

目录内的文件按固定层级顺序加载（后加载的覆盖先加载的）：
`std` → `deps` → `seed` → `suppress` → `ext` → 其余文件按字母序。
因此 `ext` 是固定标准层中的最高优先级；若目录中还有额外文件，额外文件会在 `ext` 之后按文件名顺序加载并可覆盖前面的条目。

每个 layer 的第一条有效行必须是版本头，之后每行一个 JSON record：

```text
# rivus-caps-v2
{"path":"std::fs::read_to_string","caps":"BI","basis":{"kind":"explicit"},"completeness":"complete"}
{"path":"std::collections::HashMap::new","caps":"","basis":{"kind":"explicit"},"completeness":"complete"}
```

`caps` 是按字母序排列的 `ABIMPSTU` 字符串，空字符串表示 pure。`basis.kind` 当前包括 `explicit`、`inferred`、`trait_vote`、`port` 和 `migrated_v1`。`completeness` 为 `complete`、`incomplete` 或 `unknown`。`trait_vote` basis 还保存 `implementations`、`threshold` 和逐能力 `votes`。

- linter 对 capsmap 中的键做 def_path 精确匹配，不支持后缀匹配。specialized impl 会先匹配带内部 identity marker 的精确路径，再回退到诊断中显示的无 marker 可读 def_path；因此一个可读路径条目默认作用于该方法的所有 specialization
- 如果 linter 报告某函数"既非 rvs_-prefixed nor in capsmap"，你需要补全 capsmap。方法优先级：检查源码 > 编写测试验证行为 > 合理猜测
- caps record 的 `path` 使用 rustc-driver 解析出的全限定路径（如 `core::result::impl::expect`），而非源码中的短名
- 空行和版本头之后以 `#` 开头的完整注释行会被忽略；JSON record 不支持行尾注释
- v1 `path=BI` 文件必须先运行 `cargo rivus migrate-caps`
- capsmap 只支持 `caps/` 目录，不支持单文件 capsmap，也不支持 CLI `-m/--capsmap`
- 不再读取或写入 `target/rivus-std-capsmap.txt`、`target/rivus-inferred-capsmap.txt`、`target/rivus-deps-capsmap.txt`、`target/rivus-effective-capsmap/`
- 更新 `std` / `deps` 必须显式：
  ```bash
  cargo rivus infer-std -o caps/std
  cargo rivus infer-capsmap -o caps/deps
  ```

---

## 输出分类

`check` 输出两类结果：

| 类别 | rustc 前缀 | 含义 | 影响退出码 |
|------|-----------|------|-----------|
| 违规 | `error` | 调用链能力冲突、stub 宏、空函数体 | 是 |
| 警告 | `warning` | 各种代码质量问题、推断提示 | 否 |

## 违规类型

| 类型 | 含义 |
|------|------|
| `Call` | 函数调用了自身能力不允许的函数 |
| `StaticRef` | 函数引用了 `static` 或 `thread_local!` 变量但缺少相应能力（`static` 不可变引用需要 `S`，`static mut` 引用需要 `S` + `U`，`thread_local!` 引用需要 `S` + `T`） |
| `StubMacro` | 函数体包含 `todo!()` 或 `unimplemented!()`——未实现的存根 |
| `EmptyFn` | 函数体无任何逻辑（空函数体，或仅含 `debug_assert!`/`debug_assert_eq!`/`debug_assert_ne!`） |

## 警告类型

| 警告 | 含义 |
|------|------|
| `Warning` | 调用了既非 `rvs_` 前缀也不在 capsmap 中的函数 |
| `MissingAssertWarning` | `rvs_` 函数有原始数值类型参数却未写 `debug_assert!` |
| `DeadCodeWarning` | `rvs_` 函数被 `#[allow(dead_code)]` 或 `#[allow(unused)]` 标记 |
| `MissingAllowWarning` | `rvs_` 函数有大写后缀但未被 `#[allow(non_snake_case)]` 或 `#[expect(non_snake_case)]` 覆盖 |
| `TestNameFormatWarning` | `#[test]` 函数名不匹配 `^test_\d{8}_\w+$` 格式 |
| `DuplicateTestWarning` | 同名测试函数出现多次（跨文件检测） |
| `BannedImportWarning` | 导入了被禁 crate（`thiserror`、`anyhow`、`eyre`、`color_eyre`） |
| `NonRvsFnWarning` | 函数缺少 `rvs_` 前缀（`#[test]` 函数、rustc 选中的可执行入口、trait impl 方法和没有可写源码位置的生成函数除外） |
| `MissingDocWarning` | `rvs_` 开头的 pub 函数/方法缺少 `///` 文档注释 |
| `DenyWarningsWarning` | crate 级 `#![deny(warnings)]` 反模式——应改用具名 lint |
| `WildcardImportWarning` | `use xxx::*;` 通配导入（`super::*` 和 `*::prelude::*` 除外） |
| `MissingSafetyDocWarning` | `unsafe fn` 缺少 `/// # Safety` 文档段 |
| `BorrowedParamWarning` | 参数或结构体字段使用 `&String`/`&Vec<T>`/`&Box<T>`——应改用 `&str`/`&[T]`/`&T` |
| `MissingDebugWarning` | public struct/enum 缺少 `#[derive(Debug)]` |
| `IntoImplWarning` | 直接实现 `Into`——应实现 `From`，`Into` 会自动提供 |
| `ConsumedArgOnErrorWarning` | 函数返回标准库 `Result<(), E>` 时消费了非 `Copy` owned 参数但错误类型中未保留该参数。检查会规范化 rustc 类型，并判断错误类型结构是否包含参数类型（例如 `RunError<Cli>` 的泛型参数包含 `Cli`）；不会递归检查 ADT 的枚举变体或结构体字段，因此 `AppError::Failed { cli: Box<Cli> }` 仍可能误报 |
| `DerefPolymorphismWarning` | 实现了 `Deref`——可能用 Deref 模拟继承，应改用组合 |
| `ReflectionUsageWarning` | 使用了 `std::any::Any`/`type_name`/`type_id`——应改用 trait 分发 |
| `TodoCommentWarning` | 代码中包含 `// TODO` 或 `// FIXME` 注释（含 `/* */` 块注释，仅检测以 `//` 或 `/*` 开头的行） |
| `UntestedGoodFnWarning` | good 函数（能力 ≤ ABM）未被任何测试调用 |
| `UntestedOkFnWarning` | ok 函数（能力 ≤ ABMP）未被任何测试调用 |
| `ErrorSwallowWarning` | 对标准库 `Result` 调用 `.ok()`、`.unwrap_or_default()` 或 `drop(Result)`，在未处理错误信息的情况下丢弃结果；`Option::unwrap_or_default()` 和同名自定义方法不适用 |
| `CatchUnwindWarning` | 使用 `catch_unwind`——应修 panic 源头而非捕获 |
| `CatchAllErrorVariantWarning` | 错误枚举含 `Unknown`/`Other`/`UnknownError`/`OtherError` 兜底变体 |
| `MissingTestOutputWarning` | `#[test]` 函数缺少对应的 `test_out/{name}.out` 快照文件（仅当 `test_out/` 目录存在时检查） |
| `ValidateReturnsUnitWarning` | 名为 `validate`/`check`/`verify` 的函数返回 `Result<(), E>`——应改用 `TryFrom` 返回 `Result<Target, Error>`（parse instead of validate） |
| `SpawnWarning` | 函数调用了非结构化 spawn（`tokio::spawn`、`std::thread::spawn` 等）——应改用结构化并发原语 |
| `ContractMismatchWarning` | 函数名与推断出的公开契约不一致；当前主要用于 Port trait 方法应公开为 `_P` 但名称缺失或后缀错误的情况 |
| `TraitImplOutlierWarning` | 完整、可修改的普通 trait 实现拥有投票未选中的传播能力；不改变 trait 投票和 capability totals |

## 推断提示

所有推断提示均以 `warning:` 前缀输出，不影响退出码。

| InferenceKind | 含义 |
|---------------|------|
| `MissingAsync` | 函数声明为 `async fn` 但后缀缺少 `A` |
| `MissingUnsafe` | 函数声明为 `unsafe fn` 但后缀缺少 `U` |
| `MissingMutable` | 函数有 `&mut` 参数（含 `&mut self`）但后缀缺少 `M` |
| `MissingSideEffect` | 函数读取了 `static` 变量但后缀缺少 `S` |
| `MissingThreadLocal` | 函数读取了 `thread_local!` 变量但后缀缺少 `T`（同时需要 `S`，参见 `StaticRef`） |
| `NonAlphabeticalSuffix` | 能力后缀字母未按字母序排列 |
| `DuplicateSuffixLetter` | 能力后缀中有重复字母 |
| `UnknownSuffixLetter` | 能力后缀包含不在 `ABIMPSTU` 中的字母——已知字母仍正常提取，未知字母仅报告提示 |

---

## 日常开发流程

测试快照需要有意更新时，只使用 `RUSTC_BLESS=1 cargo test`（UI 测试可加 `--test ui_tests`）；libtest 不支持 `cargo test -- --bless`。

1. **写代码时**：确保每个 `rvs_` 函数名的后缀与其实际行为一致
2. **交付前必跑**（全部通过才算交付完成）：
   ```bash
   cargo fmt            # 格式化代码
   cargo build          # 编译通过
   cargo clippy         # 无警告
   cargo test           # 测试通过
   cargo rivus check    # 能力合规检查无违规
   ```
3. **遇到 unknown callee warning 时**：linter 输出的 `Warning` 表示某个函数调用既非 `rvs_` 前缀也不在 capsmap 中。标准库路径运行 `cargo rivus infer-std -o caps/std`；若该推断命令自身报告未知前置函数，将精确 `def_path` 写入 `caps/seed`，因为 `infer-std` 只读取 `seed` 和 `suppress`。第三方依赖运行 `cargo rivus infer-capsmap -o caps/deps`。仅需修正当前项目普通检查结果时，将精确 `def_path` 写入 `caps/ext`
4. **遇到 incomplete caps knowledge warning 时**：调用检查只使用已知能力下界，不能把记录当作 pure。`std` 层运行 `cargo rivus infer-std -o caps/std`，其他自动生成层重新运行对应推断命令；人工确认的修正写入 `caps/ext`
5. **遇到其他 warning 时**：根据警告类型分别处理——缺少断言就加 `debug_assert!`，缺少文档就补 `///`，等等
6. **遇到 violation 时**：调用链能力冲突。要么修改调用方的标记（可能级联影响），要么重构代码避免不合规的调用
7. **遇到推断提示时**：推断性提示——函数的实际行为暗示应有某能力但名字里没写。审查后决定：补上能力标记（注意级联影响），或确认是误判则忽略

---

## spawn 函数的识别

linter 内置了一个 spawn 函数路径列表（`tokio::spawn`、`std::thread::spawn`、`kovi::task::spawn` 等），在 HIR 分析时自动识别这些调用并发出 `SpawnWarning`。spawn 函数的能力标注由 `infer-capsmap` 通过 callgraph 自动推断，不需要手动在 `caps/seed` 中注入条目。
