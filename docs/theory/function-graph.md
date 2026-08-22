# 函数语义图

## 实体

**函数语义图**由一组**函数节点**和**调用边**组成。

- 一个节点代表一个函数、方法或 trait 方法。
- 一条 strong 边代表一个函数直接调用另一个函数；一条 weak 边代表函数值引用，只作为保守能力依赖，不声称发生了运行时调用。
- trait impl 方法具有实现方法路径和 trait 路径两部分身份。artifact 继续使用稳定的 `实现方法路径@trait 路径` 表示，内存分析必须先解析为结构化身份，再生成 impl 聚合键或 trait declaration alias，不能由各分析阶段分别拆解字符串。实现路径还必须保留由 canonical、untrimmed、禁用 visible re-export 的完整 self type、trait identity 和 canonical impl predicates 无损编码得到的内部 impl marker，使 `Worker<u8>` 与 `Worker<u16>` 以及不同 specialization predicate 的文本路径相同时仍是不同节点。marker 还要按类型系统的结构遍历顺序，分别为 self type、implemented trait 及其参数、specialization predicates 记录所有 nominal type、trait、associated type/alias 和 unevaluated const 的 defining-crate trace；当前 impl crate 内的定义统一编码为 target-independent `local`，外部定义编码为稳定 crate ID，不能只标记根 ADT 或最外层 trait。这样同名依赖不同版本嵌套在 generic argument 或 predicate 中时仍能区分，同时同一源码的 production 与 `cfg(test)` 编译不会因本地 stable crate ID 不同而分裂。nested definition 继承 enclosing impl 的同一 marker，但只有真正的 associated method 才添加 `@trait` 后缀。普通源码定义不能使用会随 `cfg(test)` 或 target item 顺序变化的 rustc 序号 disambiguator。宏展开可能生成多个 canonical type 文本相同的 nominal type；generated impl 把 source-stable expansion identity 纳入 impl marker，宏生成的非 associated definition（包括 impl 内的 local definition）则保留 enclosing impl marker 并额外携带独立的内部 def marker，避免把不同真实定义合并。ADT 的可读路径使用 nominal type 的定义路径而非 impl block 的词法模块，避免不同类型共享 caps key；诊断和 capsmap 对外继续显示不含内部 marker 的可读路径，文本 capsmap key 对同一 nominal method 的所有对应精确实现生效
- 宏生成定义的基础 discriminator 来自定义 span 和完整 expansion chain 的 source identity，包括 remapped source file、source hash、byte range、macro definition 和 call site。同一 `DefKind` 的多个定义若共享这份基础身份，再附加它们在同组 HIR owner visitor order 中的 ordinal；只在组内计数意味着无关的 production-only 或 `cfg(test)` item 不会移动同一源码定义的 ordinal。不能使用完整或 crate-local `DefPathHash` 代替这套 source-group ordinal：前者包含 target-specific stable crate identity，后者仍继承 rustc definition disambiguator 的 item-order 依赖。内部 `{def#...}` marker 只附着在生成的 item segment，不能替换或修饰 crate segment，因此 `std::`、`core::`、`alloc::` 和本地 crate prefix 始终保持 canonical。函数名提取、用户可见路径、capsmap 查询和 prefix 分类统一忽略 `{def#...}` 与 `{impl#...}` marker，但精确图身份继续保留它们

## 节点信息

每个节点分两层信息：

1. **事实**：从源码直接观察到的性质。
2. **推断**：根据规则、调用关系和 capsmap 计算出的期望能力。

事实包括：

- 完整路径
- 直接调用集合
- `async` / `unsafe` / `&mut` / `static` / `thread_local`
- 是否是 Port 方法
- 是否是 trait impl 方法
- 是否是测试
- 每个稳定 crate ID 是否是 rustc 选中的可执行入口
- 可写回源码的函数名范围；相对文件名同时记录编译器解析它时使用的精确基准目录

推断包括：

- 对外公开能力集合
- 调用传播后得到的能力集合

## 构建过程

构建图时，只记录**事实**，不做规则裁决。

- lint pass 负责从 HIR 收集节点和边；每条已解析的显式 `Call` 或 `MethodCall` 还记录在当前函数内稳定的调用序号和源码范围。Rivus lint 属性只在 crate root 生效，支持 `#![allow(...)]`、`#![deny(...)]`、`#![warn(...)]`、`#![expect(...)]` 和 `#![forbid(...)]`（含等效的 crate-root `cfg_attr` 形式）；不允许在 item、impl item、trait item、statement、expression、parameter、field 或 variant 级使用任何这些属性控制 Rivus lint。lint pass 遇到非 crate root 的 Rivus lint 属性时必须报编译错误。收集和锚定都只使用这份 HIR 事实，不建立第二套执行图或 lowering 回配索引
- 项目收集运行单条 Cargo 命令 `cargo check --profile test --all-targets`。`--all-targets` 仍产生多个 rustc 编译单元：被选中 target 的 test-harness 单元在同一本地 crate ID 下包含普通源码 item 和 `cfg(test)` item，其余 lib、bin、test、example、bench 和 build-script 单元保持独立 crate 编译与 artifact。测试身份不依赖编译开关，而是路径约定：`DefPath` 中位于函数名 segment 之前的任意 segment 等于 `tests` 即为测试模块成员。由该判定派生 `is_production`、`is_coverage_candidate` 和 `report_line_count`。可执行入口以 `tcx.entry_fn(())` 命中为权威，扁平图缺少 Cargo target 身份时以"两 segment 且以 `main` 结尾"作为启发式回退；该回退是既定精度边界内的最佳可用规则
- callgraph artifact 负责把这些节点持久化；artifact 使用显式 schema version。schema v18 为每个 `DefPath` 保存一个扁平节点，调用边以 `FunctionIdentity`（稳定 crate ID + `DefPath`）为键区分 strong（直接调用）和 weak（函数引用）；调用点、事实、body/入口/测试/production 角色、源码、报告元数据和 Cargo package provenance 都直接属于该节点。`primary_package` provenance 来自本次 Cargo collection 的 wrapper 边界，依赖 crate 则显式记录为 `dependency`。versioned wire format 不再并列保存按 crate 分片的 map/set 或 per-target 记录。读取器只接受与当前常量完全相等的版本，所有 envelope、node、identity、call-site、call-edge、source、fact 和 provenance 层都是 closed object/value，未知字段或变体必须报错。节点级 `complete` 是读取时从节点来源确定性重建的分析投影。同一 `DefPath` 出现在多个 Cargo target 的 artifact 中时，合并器对各节点做行为、调用边、事实、角色和源码的保守并集；`crate_id` 和 `crate_provenance` 取先写入值（first-writer-wins）。节点身份或归属规则改变时必须提升版本并拒绝旧 versioned cache
- 没有 envelope 的旧目录图是独立的 typed legacy 读取形式。它只保留安全的历史调用、事实和源码作为只读诊断下界。legacy 节点的派生 `complete` 恒为 false，因此不参与当前 local report、源码契约或 target-scoped 诊断，且其不完整性会传播到推断结果。legacy 图即使为空也不能序列化或发布为当前 schema；整个 legacy 图不能与任何 current graph 合并，不以路径是否重叠为条件
- source-less synthetic value 的非零 rustc `DefPath` disambiguator 是本次编译中区分真实节点所必需的内部身份部分，不能在格式化时丢弃。例如用户定义的 crate-root `main` 与测试 harness 注入的 `main#1` 必须保持不同，即使二者共享 stable crate ID；否则 exact target-record 校验会把两个不同函数误判为冲突。source-backed 普通定义仍不能采用会随 target item 顺序变化的 rustc 序号
- 每个参与收集的 crate 都必须成功写出自己的 artifact；没有函数的 crate 仍写出合法的空图，只有完全没有 artifact 才表示 wrapper 未执行。artifact 文件名同时包含 generation identity、crate、writer PID 和进程内单调序号，写入不能替换同名已有文件；同一 rustc 进程多次写入也必须得到不同路径。合并器只接受当前 generation identity 前缀的 JSON，发现外来 JSON 立即报错，不能把并发或遗留结果混入当前图。任一写入失败都使本次收集失败，不能用其他 crate 的部分图继续分析。项目检查、报告和重命名只收集工作区 crate，第三方依赖通过调用边和 capsmap 表达；只有依赖能力推断与标准库推断才收集依赖 crate。artifact 收集阶段只保留编译错误并静默普通 warning，避免依赖推断泄漏第三方诊断
- 每次命令从 canonical project root 在 `target/.rivus-runs/` 下创建唯一的临时 generation 目录。generation identity 编码模式和 nonce，不编码 PID 或 process start time。目录内写入 closed JSON marker 记录 canonical project identity 和 typed mode。原始 artifact、Cargo target、离线诊断输入和 acknowledgement 只属于该 generation。并发命令通过唯一目录名自然隔离，不互相读取、清理或复用。generation 由 RAII guard 管理：正常返回或 panic 展开时自动删除；`kill -9` 或断电可能留下残留，交给 `cargo clean` 或人工清理。工具不做 PID 检测、孤儿扫描或 mtime 过期回收
- rustc driver 环境是 generation capability 的传输层。父命令传递 generation identity 和 canonical generation root；rustc 入口解析一次 closed protocol，验证 marker、typed mode 和 artifact/caps/offline 路径及变量组合后，才把 typed configuration 交给 lint pass。缺失、畸形或矛盾的协议必须在注册 lint 前拒绝
- generation protocol 的路径以实际目录项为准。项目的 `target` 即使是 symlink，创建 `.rivus-runs` 后也必须先 canonicalize，随后 marker、环境变量和清理流程始终传递同一个 canonical generation root
- artifact writer 在 generation 的 artifact 目录中创建不覆盖同名文件的 artifact。合并器只接受当前 generation identity 前缀的 JSON，发现外来 JSON 立即报错
- 标准库公开能力输出和下游查询范围只描述当前工具链和目标平台的 `std`、`core`、`alloc`、`compiler_builtins`，不依赖被检查应用选择了哪些第三方 crate。`-Zbuild-std` 同时编译的 `hashbrown`、`addr2line`、`gimli`、`object` 等实现依赖可以进入本次完整函数图并作为临时推断知识参与传播；只要 resolver 能从函数体、声明、trait 实现或已有能力边界得到可信能力，它们就不是 seed 缺口，也不能因为 crate 前缀不属于公开输出范围而被判为 unknown。只有没有可解析能力知识的 opaque/bodyless 边界才需要分发 seed。函数体内无法解析的间接调用不生成调用边、不传播能力，也不影响图完整度推断；但也不是可由 seed 补全的外部边界，不能使 `infer-std` 要求为 synthetic path 写 caps record。`infer-std` 必须在项目 generation 内创建无依赖、独立 workspace 的最小 probe crate 来触发 `-Zbuild-std`，不能为了收集标准库而编译应用的完整依赖图；probe 的 profile 必须显式关闭 debug assertions，使 debug-only 实现检查不进入公开 caps，同时仍继承项目目录层级中的其他 Cargo 配置，并与自己的 generation 一起清理
- 标准库函数图缓存是成功 generation 合并后的单个 versioned artifact。`infer-std` 只有在完整收集 `std`、`core`、`alloc`、完成推断并成功写出 caps 后才原子替换该缓存；收集、推断、输出或发布失败必须保留上一个完整缓存。缓存必须保留推断时的完整函数图上下文，包括标准库调用的支持 crate 函数，否则后续解释无法展示这些函数的图证据。解释仍使用查询时项目 caps 的最终层级覆盖结果，不能把历史推断上下文置于当前人工覆盖之上。读取器优先读取该合并缓存；旧目录格式只允许在 std-only 查询中独立读取，不能注入 fresh project graph，headerless 内容也不能占用单文件 published-cache 位置
- 源码写回只使用 artifact 记录的路径基准；旧 artifact 没有基准时允许兼容解析，但多个候选都存在则拒绝猜测
- 源码写回的 eligibility 只由 rustc 函数图生成的精确 source plan 决定；rust-analyzer 只把计划中的文件和字节范围解析为语义 rename position，不能按目录或语法标签再次筛选候选
- 同一命令已经确定本地 crate 边界后，callgraph 收集、std cache 选择、报告和缓存过滤都必须接收并复用这一份边界快照，并通过同一个 `LocalScope` 执行归属判定，不允许各阶段重新探测项目范围。crate-name prefix 只用于收集前的函数查询、同名 std cache 防碰撞和 standalone legacy 诊断；versioned graph 中的函数与调用边必须优先按稳定 crate ID 对应的 Cargo primary-package provenance 判定。Cargo 会把所有 build script 命名为 `build_script_build`，因此该名字绝不能单独证明本地归属，也不能单独证明 build-script 身份。build script 是编译期机器代码而非被分析程序的运行时代码，其判定必须到编译单元级：crate name 为 `build_script_build` 且 Cargo 包名（`CARGO_PKG_NAME`）归一化后不同才排除。名为 `build-script-build` 的普通包必须正常分析；lint pass 对真实 build script 的函数不收集节点（artifact 仍写出空图以满足完整性），本地 crate 发现也不把 build-script 排除视为 prefix 语义，build script 函数不参与能力推断、报告、契约诊断、trait 投票或测试覆盖。一个名为 `build-script-build` 的包自身的 build.rs 是该规则的残留歧义，作为精度边界接受
- `check` 的父进程在启动两个 Cargo 阶段前加载一次项目 caps 快照；第一阶段只收集工作区函数图，第二阶段执行非能力 HIR lint 和合并覆盖诊断且不重新解析 caps，最终离线能力分析必须复用命令开始时的同一份快照
- Cargo target 范围使用具名策略区分 production target 与 test/example/bench target；本地 crate 发现与 Cargo invocation 必须共享同一策略，不能用含义不明的布尔值分别传递
- 一次分析通过共享的 inference preparation 只执行一次 Port scope、能力推断、impl 索引和 synthetic path 识别；本地分析只为具有可写源码位置且启用契约检查的真实图节点生成契约差异，每条差异都携带完整的期望名称和期望能力，不用 `Option` 表示“此节点不检查契约”。synthetic path 和无可写源码的宏生成节点仍参与能力推断，但不产生无法修复的名称契约。synthetic path 只属于推断结果，各输出视图不能分别重建可能漂移的分析上下文

## Lint 分层

源码检查按分析范围分为三个平级类别：

- **node lint**：只依赖当前 HIR 节点、签名、属性或源码范围，直接完成判断
- **body lint**：每个函数体只遍历一次并生成 `BodyFacts`，各规则只解释共享事实，不再自行遍历函数体
- **caps lint**：把签名事实和 `BodyFacts` 投影为函数图事实，再由跨函数、跨 crate 的离线能力引擎统一推断

body collector 只遍历 HIR，并进入 closure、async block 等嵌套 body。进入嵌套 body 是统一 HIR 遍历设施的职责，不属于任一具体 lint。callgraph、测试调用识别和 body lint 必须消费同一份 HIR 调用观察；已解析调用直接携带结构化 `DefPath`，各消费者不能重新包装路径字符串。方法解析失败时保留独立的 unresolved-method 观察。

函数图保守记录源码中显式存在的 HIR `Call` 和 `MethodCall`，不尝试证明运行时可达性。因此未调用 closure、未轮询 async block、常量假分支和其他仍存在于 HIR 的显式调用同样形成边。该 over-approximation 用允许误报换取规则简单、稳定和可审查。编译期求值上下文（inline const block `const { ... }`、array repeat 长度表达式、inline asm `const` 操作数和 const generic argument）中的调用不形成运行时调用边——这些调用在编译期完成执行，不产生运行时效果，因此不传播能力、不提供测试覆盖，也不进入函数图。

分析不读取 MIR，也不从编译器 lowering 结果反推调用。函数指针和 callable 值流、隐式 drop、运算符与索引 desugar、Future poll、callback 是否立即执行、活动 enum variant 等不属于受支持的调用恢复范围。代码若依赖这些机制隐藏能力边，已经超出 Rivus 约束的 Rust 子集；工具不能猜测一个具体目标。函数图是 definition-level HIR callgraph，不是 monomorphized instance-level callgraph。通过函数指针参数、闭包变量、运行时读取的 `const`/`static`/associated const `fn()` 值或泛型 `F: Fn()` 参数发起的间接调用无法在 HIR 层解析具体目标，collector 必须为每个此类调用发出 `RVS_UNSUPPORTED_INDIRECT_CALL` warning，不猜测 callee，也不生成虚假调用边。该 warning 只用于诊断反馈，不影响图完整度推断、函数能力传播或 trait 投票；存在间接调用的函数仍可被推断为 complete pure，因为间接调用的目标在 HIR 层不可知。所有 body-bearing 函数（含普通 trait impl 方法）都会收到此 warning，但 warning 不改变 trait 投票结果或函数能力。

用户仓库和 workspace 源码不能定义会隐式执行用户代码、但 HIR 函数图无法恢复调用点的自定义 operator/index（含 `Deref`/`DerefMut`）或 `Drop` impl，也不能使用显式 `Fn`/`FnMut`/`FnOnce` trait 调用和 inline asm。`RVS_UNSUPPORTED_IMPLICIT_EXECUTION` 默认拒绝这些形式，不生成猜测边。依赖或标准库定义的 operator/index 实现仍可正常使用，因此字符串比较、集合索引等普通操作不要求用户代码为依赖实现承担源码禁令。普通 `check` 通过 Cargo `RUSTC_WORKSPACE_WRAPPER` 只对 workspace crate 运行该 HIR lint；第三方依赖不加载 lint driver，因此依赖源码中的同类定义不受此源码子集规则影响。

普通的直接子节点关系只维护一份。inline asm 的 `in`、`out`、`inout`、split-inout、符号函数和 `label` 操作数按源码顺序单次遍历，所有操作数中的显式调用共享同一连续序号流，不因操作数形式不同而遗漏或重排。inline asm `const` 操作数是编译期求值上下文，不进入运行时遍历。调用点 occurrence 完全按统一 HIR 词法遍历顺序分配——direct call、function reference 和 coverage registration 不分批编号。artifact 和诊断锚点复用同一顺序。guard pattern 条件中的调用属于 HIR pattern 的直接子节点，与普通表达式调用共享同一序号流。

free function、impl method 和带默认实现的 trait method 共享同一条 body-bearing 处理流水线；各函数来源只提供测试、文档和 Port 等策略差异。无函数体的 required trait method 只投影签名事实，不能用空 body facts 伪装成已观察的函数体。

## 测试覆盖

测试是否覆盖函数，取决于从测试函数出发能否沿 strong 调用边到达该函数，而不是调用处书写的别名或是否由测试直接调用。weak 边（函数引用）只保守传播能力，不提供测试覆盖。导入重命名不能让真实调用失去测试覆盖，也不能让另一个函数借用同名别名伪造覆盖。artifact 保存测试中的已解析目标，并为无法解析的调用保留独立的方法名回退；同名回退只有在恰好对应一个候选函数时才提供覆盖，不能一次掩盖多个同名函数。

跨进程 UI fixture 实际执行了 linter 代码，但该执行不在 unit-test crate 的运行时图内。此类覆盖只能通过带唯一 rustc diagnostic item 的测试注册 helper 显式声明：helper 接收经过类型检查的 function item，collector 使用精确 `DefId` 把它加入测试可达性；helper 自身必须位于可达测试路径。注册不是普通 callable 执行证据，禁止用 `if false`、`black_box(false)`、未调用 closure 或函数名字符串伪造同一效果。

测试覆盖必须在所有 Cargo target 的 artifact 成功收集并合并后判断。production 编译提供候选函数，unit test 和 integration test 编译提供测试调用；只在 test compilation 中存在的 helper 不是生产覆盖候选。覆盖身份由 rustc 的稳定 crate ID 和 `DefPath` 共同组成。传递可达性必须沿测试实际编译出的身份和调用边前进。无法解析的调用只有在候选名称唯一时才可回退，已解析为局部 binding 的 callable 不能进入该回退。直接 rustc/UI 模式也必须沿当前 crate 内存图做相同的传递可达性判断。合并结果作为最终 rustc lint 阶段的选择输入；Rivus lint 属性只在 crate root 生效。不能在单次 rustc 编译结束时把尚未看到其他 target 的函数报告为未测试。若任一 target 编译失败，本次全项目覆盖判断不可用，也不输出基于部分图的覆盖结论。

同一 `DefPath` 在不同 Cargo target 中可能由 `cfg` 产生不同函数体或不同入口角色。扁平合并图按 `DefPath` 合并所有 Cargo target 的 artifact，各节点的行为、调用边、事实、角色和源码取保守并集。任何消费者需要 target 身份、调用、事实、body、角色、归属或源码时都读取同一合并节点。contract、static/thread-local 和 trait outlier 等离线诊断锚定实际具有违规行为的合并节点。Trait 投票按实现路径计数；不同定义 crate 的实现必须进入同一投票。该聚合是工作区运行时代码模型，不是 Cargo unit graph 的无损表示：同名 lib/bin target 的 target 级精度有意不保证，只有 build-script crate 被整体排除；依赖版本同名冲突只在 all-crates 推断中可能出现，不构成恢复 per-target 项目模型的理由。

调用诊断身份同时保存 callee、函数内序号和可用时的精确源码范围。occurrence 只在单个真实函数身份内有意义。对同一 callee 的多个真实调用以不同 occurrence 完整保留。最终 rustc 阶段若找不到指定调用点，不能回退到函数范围并伪造已匹配确认。

聚合型 incomplete-knowledge 警告为每个受影响 caller 保留独立函数锚点，同一 caller 内的多个调用不重复发射相同摘要；Rivus lint 属性只在 crate root 生效。

## 推断过程

推断阶段读取整张图，根据统一规则生成独立的派生结果，不把期望能力、期望名称或推断出的外部函数写回事实节点。**函数名不参与推断输入**——语义能力只来自签名/函数体事实、调用边、Port 结构、trait 投票和 capsmap 精确条目：

- Port 方法的公开传播能力固定为 `P`，实现的 `B/I/S/T` 是审计信息，不进入契约或后缀
- Port 是当前工作区对 World 解释器的结构约定：本地 trait 必须声明一个非泛型 `World`、至少一个无 receiver 的操作，且每个操作显式接收 World 引用；额外 associated type 表示长期资源。trait 名不参与判断。依赖库和标准库中的相同结构不会自动获得当前项目的 `P`，而是按 capsmap 和实际行为处理。
- `A/C/M/U` 只由函数自身的签名或函数体事实决定，不通过调用传播
- 普通调用把 `B/I/P/S/T` 从被调用方传播到调用方；包含 `P` 的被调用方在该调用边只传播 `P`
- 外部函数通过 capsmap 补全能力；没有函数体、impl 投票或 capsmap 条目的 bodyless 声明保持 unknown，不从名字后缀补全
- trait 方法的完整能力由各 impl 的传播能力做“至少半数”聚合；声明自身写的后缀视图只在没有 impl 可聚合时用于**命名视图比较**，不作为语义回退。两个 impl 中只要一个具备某传播能力，该能力就会出现在 trait 方法的聚合结果中。这是典型行为的经验性折中，不是严格多数投票，也不是完整的 over-approximation。
- Trait 投票必须保留参与实现数、阈值和逐能力票数。完整、可修改且拥有未入选传播能力的本地实现作为非典型实现产生设计反馈，但不改变公开投票结果。
- Port trait 方法例外：公开契约固定为 `P`；实现体的 `B/I/S/T` 效果是适配器审计信息（report/why），不与契约或名称比较，而普通调用方只继承 `P`

## 差异

能力诊断、annotate、why、report 不再各自发明一套解释，而是基于同一张图做不同视图。lint pass 只从 HIR 收集能力事实；能力契约、后缀、静态状态和调用边诊断统一由离线能力引擎计算。各视图复用同一套本地分类策略；离线诊断在遍历真实节点时构造一次名称上下文供各诊断规则消费。由 rustc 选中的可执行入口、测试和无可写源码的生成节点不生成一般源码诊断；trait impl 不生成名称契约。Port impl 的契约固定投影为 `P`，其实现效果只进审计视图；未知调用和 `U` 等安全事实仍报告。synthetic 推断路径没有真实节点，也不参与节点诊断。直接 rustc/UI 模式使用当前 crate 的内存图，`cargo rivus check` 使用合并后的全项目图。

各视图共享函数的本地范围、入口点、测试、trait impl、Port、源码和生成代码分类，但保留具名的视图策略；contract、offline、report 和 rename 不得因复用分类而被压成同一套筛选条件。

- **lint**：收集事实，并把统一能力引擎的当前 crate 诊断映射为 rustc lint
- **annotate**：把期望名字写回源码
- **why**：展示节点能力和边上的来源
- **report**：从 fresh function graph 的 report metadata 聚合能力分布和 contract mismatch 摘要；只在测试编译中存在的 helper 不进入生产能力分布，相同路径的不同 production 源码定义仍分别计数
