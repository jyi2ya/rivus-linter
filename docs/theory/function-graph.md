# 函数语义图

## 实体

**函数语义图**由一组**函数节点**和**调用边**组成。

- 一个节点代表一个函数、方法或 trait 方法。
- 一条边代表一个函数直接调用另一个函数。
- trait impl 方法具有实现方法路径和 trait 路径两部分身份。artifact 继续使用稳定的 `实现方法路径@trait 路径` 表示，内存分析必须先解析为结构化身份，再生成 impl 聚合键或 trait declaration alias，不能由各分析阶段分别拆解字符串。实现路径还必须保留由 canonical、untrimmed、禁用 visible re-export 的完整 self type 与 trait identity 无损编码得到的内部 impl marker，使 `Worker<u8>` 与 `Worker<u16>` 等文本路径相同的 specialized impl 仍是两个节点；nested definition 继承 enclosing impl 的同一 marker，但只有真正的 associated method 才添加 `@trait` 后缀。不能使用会随 `cfg(test)` 或 target item 顺序变化的 rustc 序号 disambiguator。ADT 的可读路径使用 nominal type 的定义路径而非 impl block 的词法模块，避免不同类型共享 caps key；诊断和 capsmap 对外继续显示不含内部 marker 的可读路径，文本 capsmap key 对同一 nominal method 的所有对应精确实现生效

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

- lint pass 负责从 HIR 收集节点和边；每条已解析调用还记录在当前函数内稳定的调用序号，最终编译用同一 HIR 遍历顺序把离线调用诊断重新锚定到实际 call expression，使 statement/expression 级 `allow`、`expect`、`warn`、`deny` 和 `forbid` 继续由 rustc 解释
- callgraph artifact 负责把这些节点持久化；artifact 使用显式 schema version。schema v9 为每个节点保存可执行入口对应的稳定 crate ID 集合，使同路径的 library 普通函数与 binary 入口可以同时存在。节点身份或 target 角色规则改变时必须提升版本并拒绝旧 versioned cache，避免复用已经合并错误身份的图；没有 envelope 的旧目录图只保留只读兼容
- 每个参与收集的 crate 都必须成功写出自己的 artifact；没有函数的 crate 仍写出合法的空图，只有完全没有 artifact 才表示 wrapper 未执行。任一写入失败都使本次收集失败，不能用其他 crate 的部分图继续分析。项目检查、报告和重命名只收集工作区 crate，第三方依赖通过调用边和 capsmap 表达；只有依赖能力推断与标准库推断才收集依赖 crate。artifact 收集阶段只保留编译错误并静默普通 warning，避免依赖推断泄漏第三方诊断
- 每次命令必须在项目 `target/` 下原子预留自己的 generation。原始 artifact、Cargo target 和测试覆盖选择文件只属于该 generation；并发命令不能清理、读取或复用彼此的 generation。命令只在 Cargo 成功后合并自己的完整 artifact 集，正常结束时只清理自己的 generation，遗留 generation 不属于任何缓存且所有读取器必须忽略
- 标准库函数图缓存是成功 generation 合并后的单个 versioned artifact。`infer-std` 只有在完整收集 `std`、`core`、`alloc`、完成推断并成功写出 caps 后才原子发布该缓存；收集、推断或输出失败必须保留上一个完整缓存。读取器优先读取该合并缓存，并只为已有用户保留旧目录格式的只读兼容
- 源码写回只使用 artifact 记录的路径基准；旧 artifact 没有基准时允许兼容解析，但多个候选都存在则拒绝猜测
- 源码写回的 eligibility 只由 rustc 函数图生成的精确 source plan 决定；rust-analyzer 只把计划中的文件和字节范围解析为语义 rename position，不能按目录或语法标签再次筛选候选
- 同一命令已经确定本地 crate 边界后，callgraph 收集、std cache 选择、报告和缓存过滤都必须接收并复用这一份边界快照，并通过同一个 `LocalScope` 执行 typed path 和字符串 path 的归属判定，不允许各阶段重新构造 prefix 规则或用可选参数在执行中途重新探测项目范围
- `check` 的父进程在启动两个 Cargo 阶段前加载一次项目 caps 快照；第一阶段只收集工作区函数图，第二阶段执行非能力 HIR lint 和合并覆盖诊断且不重新解析 caps，最终离线能力分析必须复用命令开始时的同一份快照
- Cargo target 范围使用具名策略区分 production target 与 test/example/bench target；本地 crate 发现与 Cargo invocation 必须共享同一策略，不能用含义不明的布尔值分别传递
- 一次分析通过共享的 inference preparation 只执行一次 Port scope、能力推断、impl 索引和 synthetic path 识别；本地分析只为具有可写源码位置且启用契约检查的真实图节点生成契约差异，每条差异都携带完整的期望名称和期望能力，不用 `Option` 表示“此节点不检查契约”。synthetic path 和无可写源码的宏生成节点仍参与能力推断，但不产生无法修复的名称契约。synthetic path 只属于推断结果，各输出视图不能分别重建可能漂移的分析上下文

## Lint 分层

源码检查按分析范围分为三个平级类别：

- **node lint**：只依赖当前 HIR 节点、签名、属性或源码范围，直接完成判断
- **body lint**：每个函数体只遍历一次并生成 `BodyFacts`，各规则只解释共享事实，不再自行遍历函数体
- **caps lint**：把签名事实和 `BodyFacts` 投影为函数图事实，再由跨函数、跨 crate 的离线能力引擎统一推断

body collector 必须进入 closure、async block 等嵌套 body，否则嵌套代码中的调用和行为会漏报。进入嵌套 body 是统一 body 遍历设施的职责，不属于任一具体 lint。callgraph、测试调用识别和 body lint 必须消费同一份调用观察；已解析调用直接携带结构化 `DefPath`，各消费者不能重新包装路径字符串，但仍可按各自语义选择 canonical target、源码方法名或 unresolved path。方法解析失败时仍保留独立的 unresolved-method 观察，不能让语法级检查和测试覆盖因类型解析缺失而消失。

普通 HIR 表达式的直接子节点关系只维护一份。inline asm 的 `in`、`out`、`inout`、split-inout 和符号函数操作数属于表达式子节点，不能因操作数形式不同而遗漏其中的调用。block/loop 和 closure 属于带上下文的边界：block 负责 statement、let-else 和尾表达式，closure 通过独立 body 解析并标记已进入嵌套 body，不能被普通子节点遍历扁平化，也不能用固定嵌套层数截断有限 HIR 树。

free function、impl method 和带默认实现的 trait method 共享同一条 body-bearing 处理流水线；各函数来源只提供测试、文档和 Port 等策略差异。无函数体的 required trait method 只投影签名事实，不能用空 body facts 伪装成已观察的函数体。

## 测试覆盖

测试是否覆盖函数，取决于从测试函数出发能否沿调用边到达该函数，而不是调用处书写的别名或是否由测试直接调用。导入重命名不能让真实调用失去测试覆盖，也不能让另一个函数借用同名别名伪造覆盖。artifact 保存测试中的已解析目标，并为无法解析的调用保留独立的方法名回退；同名回退只有在恰好对应一个候选函数时才提供覆盖，不能一次掩盖多个同名函数。

测试覆盖必须在所有 Cargo target 的 artifact 成功收集并合并后判断。production 编译提供候选函数，unit test 和 integration test 编译提供测试调用；只在 test compilation 中存在的 helper 不是生产覆盖候选。覆盖身份由 rustc 的稳定 crate ID 和 `DefPath` 共同组成，因此同名 library/binary target 不会互相借用覆盖；unit-test 编译副本通过相同源码位置映射回 production 身份，无源码节点仅在 production 身份唯一时映射。传递可达性必须沿测试实际编译出的身份和调用边前进，只在标记 production 候选已覆盖时做身份归一化，否则 `cfg(test)` 分支会错误借用 production 调用边。无法解析的调用只有在候选名称唯一时才可回退，已解析为局部 binding 的 callable 不能进入该回退。直接 rustc/UI 模式也必须沿当前 crate 内存图做相同的传递可达性判断，而不是只看测试函数的直接调用。合并结果作为最终 rustc lint 阶段的选择输入，使 `allow`、`expect`、`warn`、`deny` 和 `forbid` 仍由编译器按函数、参数及其父作用域解释；artifact-only 阶段只在内部满足 crate、item、parameter、statement、expression 和 field 作用域中的 Rivus `expect`，避免 crate 级 `forbid(unfulfilled_lint_expectations)` 中断收集。仅为满足同源码 test-compilation 中 `expect` 而添加的别名锚点只在最终 lint level 仍为 `expect` 时发射；`warn`、`deny` 或 `forbid` 不能把 production-only 行为广播为测试变体的真实违规。不能在单次 rustc 编译结束时把尚未看到其他 target 的函数报告为未测试。若任一 target 编译失败，本次全项目覆盖判断不可用，也不输出基于部分图的覆盖结论。

同一 `DefPath` 在不同 Cargo target 中可能由 `cfg` 产生不同函数体或不同入口角色。合并图必须保留每个稳定 crate ID 的签名事实、是否具有函数体、是否是可执行入口、调用边和推断完整度；contract、static/thread-local 和 trait outlier 等离线诊断只能锚定实际具有违规行为且不是可执行入口的 target identity，不能把一个 target 的行为、入口豁免或未知依赖广播到其他同名 target。Trait 投票按 production/test 角色组成 cohort，并按实现路径计数；不同定义 crate 的实现必须进入同一角色投票，测试角色缺少独立编译副本的实现可使用其 production 贡献作为只读回退，不能按各实现自己的 stable crate ID 拆成多个单实现投票。

调用诊断身份同时保存 callee、函数内序号和可用时的精确源码范围。跨 production/test 副本建立 `expect` 别名时优先按源码范围匹配，不能因 `cfg` 插入或删除了更早的调用而把诊断映射到另一个表达式；无源码范围时才允许使用序号回退。artifact 必须保证每个 target 的调用集合和调用点集合一致，并拒绝同一源码范围内重复的调用序号。最终 rustc 阶段若找不到指定调用点，不能回退到函数范围并伪造已匹配确认。

聚合型 incomplete-knowledge 警告可以共享摘要文本，但必须为每个受影响 caller 保留独立函数锚点，使 `allow`、`expect`、`warn`、`deny` 和 `forbid` 仍按 caller 的 rustc 作用域解释；同一 caller 内的多个调用不重复发射相同摘要。

## 推断过程

推断阶段读取整张图，根据统一规则生成独立的派生结果，不把期望能力、期望名称或推断出的外部函数写回事实节点：

- Port 方法对外只有 `P`
- Port 是当前工作区对六边形架构端口的特别约定，只对当前命令认定为本地的 crate 生效。依赖库和标准库中即使 trait 名以 `Repository` 或 `Client` 结尾，也不自动获得 `P`，而是按 capsmap、显式后缀和实际行为处理。
- `A/M/U` 只由签名事实决定，不通过调用传播
- `B/I/P/S/T` 从被调用方传播到调用方
- 外部函数通过 capsmap 补全能力
- 普通非 Port trait 方法的公开能力由各 impl 的传播能力做“至少半数”聚合；声明自身写的能力后缀只在没有 impl 可聚合时作为回退。两个 impl 中只要一个具备某传播能力，该能力就会出现在 trait 方法的聚合结果中。这是典型行为的经验性折中，不是严格多数投票，也不是完整的 over-approximation。
- Trait 投票必须保留参与实现数、阈值和逐能力票数。完整、可修改且拥有未入选传播能力的本地实现作为非典型实现产生设计反馈，但不改变公开投票结果。
- Port trait 方法例外：公开能力固定为 `P`，不受 impl 的实际 I/O、副作用或阻塞行为影响。

## 差异

能力诊断、annotate、why、report 不再各自发明一套解释，而是基于同一张图做不同视图。lint pass 只从 HIR 收集能力事实；能力契约、后缀、静态状态和调用边诊断统一由离线能力引擎计算。各视图复用同一套本地分类策略；离线诊断在遍历真实节点时构造一次名称上下文供各诊断规则消费。由 rustc 选中的可执行入口、测试和无可写源码的生成节点不生成一般源码诊断；trait impl 不生成名称契约，其中 Port impl 仍检查后缀、静态状态和调用边。synthetic 推断路径没有真实节点，也不参与节点诊断。跨 Cargo target 合并丢弃同一源码 test-compilation 副本的重复行为事实，但保留其 coverage 身份、调用边和源码映射；若 production 和 test-compilation 副本都没有源码位置，则 production 行为优先，测试副本仍只贡献 coverage metadata。同名普通函数和入口并存时保留普通函数用于契约，并独立保留入口的直接调用用于依赖推断；同一源码同时承担普通函数和生产入口时拒绝猜测。不同 production target 中相同路径、兼容角色的不同源码定义以事实与调用边的并集做保守能力分析，同时保留每个定义独立的覆盖 eligibility、report eligibility、函数数量和有效行数；Port、test 或 trait-impl 分类不一致时拒绝合并。直接 rustc/UI 模式使用当前 crate 的内存图，`cargo rivus check` 使用合并后的全项目图。

各视图共享函数的本地范围、入口点、测试、trait impl、Port、源码和生成代码分类，但保留具名的视图策略；contract、offline、report 和 rename 不得因复用分类而被压成同一套筛选条件。

- **lint**：收集事实，并把统一能力引擎的当前 crate 诊断映射为 rustc lint
- **annotate**：把期望名字写回源码
- **why**：展示节点能力和边上的来源
- **report**：从 fresh function graph 的 report metadata 聚合能力分布和 contract mismatch 摘要；只在测试编译中存在的 helper 不进入生产能力分布，相同路径的不同 production 源码定义仍分别计数
