# Cargo Rivus 工具手册

`cargo rivus` 是基于 rustc-driver 的 lint 插件，通过编译器的 `LateLintPass` 在 HIR 层面分析代码。

项目 crate 名为 `rivus-linter`，安装后的二进制名为 `cargo-rivus`，作为 cargo 子命令使用：`cargo rivus <subcommand>`。

**`cargo rivus`（无子命令）默认运行 `check`。**

---

## 命令一览

| 命令 | 用途 |
|------|------|
| `cargo rivus check` | 检查 `rvs_` 函数调用链能力合规性（默认） |
| `cargo rivus report` | 统计项目能力分布和契约不一致摘要，输出好函数率 |
| `cargo rivus infer-capsmap` | 从项目 `caps/` 推断 direct external deps，并写到 `-o` 指定路径 |
| `cargo rivus infer-std` | 推断标准库函数能力标注并写到 `-o` 指定路径（需 nightly） |
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

基于 rustc-driver 的 HIR 分析。第一阶段运行非能力 HIR lint，第二阶段收集全项目函数图，并由统一的离线能力引擎检查命名契约、后缀、静态状态和调用链合规性。

```bash
cargo rivus check                    # 使用项目 caps/（若存在）
cargo rivus check -- --features foo  # 传递额外 cargo check 参数
```

注意：capsmap 只从项目 `caps/` 目录加载。CLI 不再支持 `-m/--capsmap`，也不再读取或生成 `target/rivus-std-capsmap.txt`、`target/rivus-inferred-capsmap.txt`、`target/rivus-deps-capsmap.txt`、`target/rivus-effective-capsmap/`。目录不存在时统一能力引擎使用空 capsmap。caps 目录使用统一的层级加载器（`CapsMap::rvs_load_dir_BIS`），按 `std → deps → seed → suppress → ext → 其余字母序` 的固定顺序合并。


注意：`check` 默认编译 `--all-targets`，因此测试、示例和 benchmark 中的函数也会被分析。`infer-capsmap` 和 `infer-std` 只编译 production targets。

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

**以下函数被排除在统计之外**：`#[test]` 函数，以及 `#[allow(dead_code)]` 或 `#[allow(unused)]` 标记的函数。

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

推断分两步：首先对不在种子中的函数，直接从行为特征推断能力（`async fn` → A、`unsafe fn` → U、`&mut` 参数 → M、`static` 引用 → S、`static mut` 引用 → S+U、`thread_local!` 引用 → S+T）；然后通过固定点迭代，将所有被调用方的能力沿调用图向上传播。若同一函数同时被识别为普通 `static` 引用和 `thread_local!` 引用，结果会合并为 `S+T`（幂等）。种子中的条目作为推断的起点（下界），传播可能在其基础上累加更多能力。

对于本地非 Port trait 方法，公开能力由各 impl 按能力逐项做 at-least-half vote（阈值为 `ceil(n/2)`）决定；trait 声明自身写的后缀只在没有 impl 可聚合时作为回退。因此 2 个 impl 中 1 个带能力会被抬升，3 个 impl 中仅 1 个带能力不会被抬升。Port trait 方法例外：公开能力固定为 `P`，不受 impl 实际行为影响。这一规则同样会影响 `annotate` 和 `why` 的显示结果。

```bash
cargo rivus infer-capsmap -o caps/deps       # 从项目 caps/ 推断，并把 direct external deps 写到指定文件
```

选项：
- `-o, --output <PATH>` — **必填**。direct external deps capsmap 输出路径；相对路径按目标项目目录解析。通常写到 `caps/deps`。命令只写这个显式输出，不再写入 `target/rivus-inferred-capsmap.txt` 或 `target/rivus-deps-capsmap.txt`。

注意：种子始终从项目 `caps/` 加载（排除 `deps` 层，避免旧 deps 干扰重新推断）。


---

## `cargo rivus infer-std [OPTIONS] [PATH]`

通过 `-Zbuild-std` 编译 std/core/alloc，推断标准库函数的能力标注。需要 nightly Rust；命令实际会设置 `RUSTUP_TOOLCHAIN=nightly` 并运行 `cargo check -Zbuild-std=std,core,alloc`，如果本机没有可用的 nightly toolchain 会直接失败。`PATH` 必须是一个有效的本地 crate 项目；仅含 `[workspace]` 的虚拟根目录不受支持。

注意：该命令只会从 `PATH/caps` 加载 `seed` 和 `suppress` 文件（不加载 `std`/`deps`/`ext`，因为那些是上一次生成的结果，会干扰重新生成），并在其基础上推断标准库条目。

```bash
cargo rivus infer-std -o caps/std        # 将 std caps 写到指定文件（通常 caps/std）
```

选项：
- `-o, --output <PATH>` — **必填**。std capsmap 输出路径；相对路径按目标项目目录解析。通常写到 `caps/std`。命令只写这个显式输出，不再写入 `target/rivus-std-capsmap.txt`。


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
- root `main`、测试函数、trait impl 方法、synthetic 节点、宏展开或其他没有真实源码位置的函数不会作为直接 annotate 候选；trait impl 方法可能会随 trait 声明或调用点的语义重命名被间接更新
- 本地非 Port trait 方法的能力由各 impl 按 at-least-half vote（`ceil(n/2)`）聚合；声明后缀只在没有 impl 可聚合时作为回退。Port 方法固定为 `P`
- annotate 后 `#[serde(default = "...")]` 等字符串字面量中的函数引用不会自动更新，需要手动修复
- annotate 会删除 `target/rivus-callgraph` 和 `target/rivus-callgraph-std` 缓存（函数名已变，旧缓存失效）

---

## capsmap

为非 `rvs_` 函数声明能力。**只支持项目根目录下的 `caps/` 目录**：

```
caps/
├── seed      # 手动维护的底层基线（分配、I/O 内部、编译器内部、async 展开等）
├── std       # std/core/alloc 的全量条目（`cargo rivus infer-std -o caps/std`）
├── deps      # 第三方依赖条目（`cargo rivus infer-capsmap -o caps/deps`）
├── suppress  # 修正条目（覆盖 std/deps 中过宽的能力标记）
└── ext       # 项目特定条目（标准层中最高优先级）
```

目录内的文件按固定层级顺序加载（后加载的覆盖先加载的）：
`std` → `deps` → `seed` → `suppress` → `ext` → 其余文件按字母序。
因此 `ext` 是固定标准层中的最高优先级；若目录中还有额外文件，额外文件会在 `ext` 之后按文件名顺序加载并可覆盖前面的条目。

每行一个条目，格式：

```
完整函数路径=能力字母 # 可选注释
```

示例：

```
std::fs::read_to_string=BI     # 阻塞+I/O（失败由 Result 表达）
std::collections::HashMap::new=  # 纯函数，无能力
std::process::exit=S           # 副作用：终止进程
```

- linter 对 capsmap 中的键做精确匹配（全限定路径完全一致）。不支持后缀匹配——caps 文件中的键必须使用 rustc 给出的 def_path
- 如果 linter 报告某函数"既非 rvs_-prefixed nor in capsmap"，你需要补全 capsmap。方法优先级：检查源码 > 编写测试验证行为 > 合理猜测
- caps 文件中的条目使用 rustc-driver 解析出的全限定路径（如 `core::result::impl::expect=`），而非源码中的短名
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
| `NonRvsFnWarning` | 函数缺少 `rvs_` 前缀（`#[test]` 函数、`main`、`new`、`go`、`wblk`、trait impl 方法除外） |
| `MissingDocWarning` | `rvs_` 开头的 pub 函数/方法缺少 `///` 文档注释 |
| `DenyWarningsWarning` | crate 级 `#![deny(warnings)]` 反模式——应改用具名 lint |
| `WildcardImportWarning` | `use xxx::*;` 通配导入（`super::*` 和 `*::prelude::*` 除外） |
| `MissingSafetyDocWarning` | `unsafe fn` 缺少 `/// # Safety` 文档段 |
| `BorrowedParamWarning` | 参数或结构体字段使用 `&String`/`&Vec<T>`/`&Box<T>`——应改用 `&str`/`&[T]`/`&T` |
| `MissingDebugWarning` | struct/enum 缺少 `#[derive(Debug)]` |
| `IntoImplWarning` | 直接实现 `Into`——应实现 `From`，`Into` 会自动提供 |
| `ConsumedArgOnErrorWarning` | 函数返回 `Result<(), E>` 时消费了 owned 参数但错误类型中未保留该参数。注意：仅检查错误类型名称中是否包含参数类型标识符（如 `RunError<Cli>` 包含 `Cli`），无法深入检查错误枚举的变体字段——如果参数确实被保留在变体中（如 `AppError::Failed { cli: Box<Cli> }`），属于误报 |
| `DerefPolymorphismWarning` | 实现了 `Deref`——可能用 Deref 模拟继承，应改用组合 |
| `ReflectionUsageWarning` | 使用了 `std::any::Any`/`type_name`/`type_id`——应改用 trait 分发 |
| `TodoCommentWarning` | 代码中包含 `// TODO` 或 `// FIXME` 注释（含 `/* */` 块注释，仅检测以 `//` 或 `/*` 开头的行） |
| `UntestedGoodFnWarning` | good 函数（能力 ≤ ABM）未被任何测试调用 |
| `UntestedOkFnWarning` | ok 函数（能力 ≤ ABMP）未被任何测试调用 |
| `ErrorSwallowWarning` | 调用 `.ok()` 或 `.unwrap_or_default()` 静默吞掉错误 |
| `CatchUnwindWarning` | 使用 `catch_unwind`——应修 panic 源头而非捕获 |
| `CatchAllErrorVariantWarning` | 错误枚举含 `Unknown`/`Other`/`UnknownError`/`OtherError` 兜底变体 |
| `MissingTestOutputWarning` | `#[test]` 函数缺少对应的 `test_out/{name}.out` 快照文件（仅当 `test_out/` 目录存在时检查） |
| `ValidateReturnsUnitWarning` | 名为 `validate`/`check`/`verify` 的函数返回 `Result<(), E>`——应改用 `TryFrom` 返回 `Result<Target, Error>`（parse instead of validate） |
| `SpawnWarning` | 函数调用了非结构化 spawn（`tokio::spawn`、`std::thread::spawn` 等）——应改用结构化并发原语 |
| `ContractMismatchWarning` | 函数名与推断出的公开契约不一致；当前主要用于 Port trait 方法应公开为 `_P` 但名称缺失或后缀错误的情况 |

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

1. **写代码时**：确保每个 `rvs_` 函数名的后缀与其实际行为一致
2. **交付前必跑**（全部通过才算交付完成）：
   ```bash
   cargo fmt            # 格式化代码
   cargo build          # 编译通过
   cargo clippy         # 无警告
   cargo test           # 测试通过
   cargo rivus check    # 能力合规检查无违规
   ```
3. **遇到 unknown callee warning 时**：linter 输出的 `Warning` 表示某个函数调用既非 `rvs_` 前缀也不在 capsmap 中。补全 capsmap 即可消除
4. **遇到其他 warning 时**：根据警告类型分别处理——缺少断言就加 `debug_assert!`，缺少文档就补 `///`，等等
5. **遇到 violation 时**：调用链能力冲突。要么修改调用方的标记（可能级联影响），要么重构代码避免不合规的调用
6. **遇到推断提示时**：推断性提示——函数的实际行为暗示应有某能力但名字里没写。审查后决定：补上能力标记（注意级联影响），或确认是误判则忽略

---

## spawn 函数的识别

linter 内置了一个 spawn 函数路径列表（`tokio::spawn`、`std::thread::spawn` 等），在 HIR 分析时自动识别这些调用并发出 `SpawnWarning`。spawn 函数的能力标注由 `infer-capsmap` 通过 callgraph 自动推断，不需要手动在 `caps/seed` 中注入条目。
