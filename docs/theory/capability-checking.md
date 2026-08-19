# 能力检查

## 实体与事实源

**函数**是基本实体。函数的**语义能力集合**（`ABCIMPSTU` 的子集）唯一来自调用图：函数签名与函数体事实、直接调用边、capsmap 精确条目、trait 实现投票和 Port 结构规则共同推断出每个节点的 semantic caps。函数**名字不是能力来源**。

名字中的能力后缀只是 semantic caps 的**只读视图**：把 semantic caps 投影到 `B/I/M/P/S/T` 并按字母序排列，方便人类和大模型即时阅读。后缀与投影不一致时产生 naming view 诊断；后缀永不参与能力推断、传播、调用检查、报告统计或覆盖分类。修改函数名（含删除或伪造后缀）不会改变任何语义分析结果。

`A/C/U` 永不进入视图：A 来自 `async fn`、C 来自 `const fn`、U 来自 `unsafe fn` 或 `static mut` 访问，这些信息从签名与函数体直接可见。

## 能力

能力是函数的运行时性质，共有九种：

| 能力 | 含义 | 参与调用规则 | 推断方式 |
|------|------|------------|---------|
| A | 异步函数声明 | 否（自身签名） | `async fn` |
| B | 可能阻塞线程 | 是 | 传播 |
| C | 常量求值函数声明 | 否（自身签名） | `const fn` |
| I | 执行 I/O 操作 | 是 | 传播 |
| M | 接受可变状态 | 否（自身签名） | `&mut` 参数 |
| P | World 端口操作 | 是 | 本地 trait 结构推断 |
| S | 有副作用（读写全局、环境变量、随机等） | 是 | 传播 |
| T | 依赖线程局部状态 | 是 | 传播 |
| U | 不安全函数或 `static mut` 访问 | 否（自身签名/函数体） | `unsafe fn` / `static mut` |

### 函数级别

| 级别 | 能力范围 | 含义 | 测试策略 |
|------|---------|------|---------|
| pure | 空集 | 确定性、无副作用 | 直接单元测试 |
| good | A/B/C/M 子集 | 无 I/O、无副作用，可单元测试 | 直接单元测试 |
| ok | A/B/C/M/P 子集 | good 的超集；不含 I/S/T/U 的 P 操作可替换解释器 | fake World 测试 |

good 和 ok 的统计集合可以重叠，但未测试诊断采用互斥分类：good 函数只报告 good，只有属于 ok 且不属于 good 的函数才报告 ok。

## 命名视图

函数名以 `rvs_` 开头。名字的最后一节（以下划线分隔）若全由大写 ASCII 字母组成，则视为能力后缀视图：

- 期望视图 = `project_BIMPST(semantic caps)`，按字母序排列
- 完整知识下，actual 后缀必须精确等于期望视图
- 后缀含 A/C/U、未知字母、重复字母或乱序是**视图结构错误**
- 缺失或多出 B/I/M/P/S/T 是**视图与语义不一致**

执法位置：普通调用边不再产生 call violation——caller 的能力就是传播闭包，resolved 调用链自洽。能力执法由 error 级 naming 诊断承接：缺失字母报对应 Missing\* contract 错误；多出字母或名不副实报 NameMismatch 错误（actual 后缀字母超出期望视图时必须报告，仅缺失字母时由 Missing\* 承接、NameMismatch 沉默；乱序、重复、未知字母等结构缺陷交给各自的专门诊断）。rustc 输出层把所有字母类 contract kind 统一映射到 `rvs_contract_mismatch`（Deny），具体 kind 保留在消息与离线报告 code 中；offline Error 严重级一律对应 Deny lint，Warn 对应 Warn lint。唯一例外是缺 `rvs_` 前缀：它是命名约定而非能力谎言，保持 Warning 并可按 crate 豁免。唯一保留调用边执法的是 World Port 实现体：impl body 与默认 trait body 按 voted 契约向下检查，执行契约未声明的能力报 `port_effect_violation` 错误；Port 实现内的 unknown callee 同样必须浮出，不得被 Port 分支吞掉。

例：
- `rvs_add` → 期望视图为空（semantic caps 为空集时）
- `rvs_write_db_BIS` → semantic caps 含 B/I/S 时期望视图为 `BIS`（A/C/U 由签名与函数体测量，永不进入名字）
- 纯函数错误写 `_I` → semantic caps 仍为空集，只报 extra-view 诊断，调用方仍按纯函数检查

### P（Port）能力

P 是特殊能力：普通函数可以通过 `_P` 视图声明自己依赖端口；World Port 的操作由 trait 结构获得 P，并通过实现投票获得完整的 `B/I/S/T` 契约。

本地 trait 声明一个非泛型 associated type `World`，至少包含一个无 `self` receiver 的操作，并且每个操作都显式接收 `&Self::World` 或 `&mut Self::World` 时，该 trait 被标记为 **Port**。额外 associated type 表示长期资源；associated constant、generic World、receiver 方法或缺少 World 参数的操作都会使整个 trait 按普通 trait 处理。trait 名不参与判断。

Port 操作自动获得 P；各 impl 的 `B/I/S/T` 能力按普通 trait 的至少半数规则投票，入选能力与操作自身的 `A/C/M/U` 一起构成完整契约，后缀只保留 BIMPST 字母。具体 impl body 与默认 trait body 都按完整契约向下检查，不能执行契约未声明的效果。

P 的语义：调用者通过类型级解释器和显式 World 使用一个可替换端口，而非依赖运行时 Client 对象或具体 adapter。实现可以替换为持有内存状态的 fake World。

### 调用规则中的 P

P 参与调用规则并向上传播：只要被调用方的完整能力包含 P，该调用边的传播需求就是仅 P，同一契约中的 `B/I/S/T` 不再向上传播。没有 P 的函数仍不能调用 Port 操作。

## 调用规则

函数 A 调用函数 B 时，调用边逐字母检查可传播能力：普通调用要求 A 拥有 B 的每个 `B/I/P/S/T` 能力；若 B 包含 P，则该调用边只要求 A 拥有 P。

`A/C/M/U` 不参与调用规则，因此调用方的完整能力集不必是被调用方完整能力集的超集。

**"我有，方可调你。"**

`B/I/P/S/T` 是且仅是五个传播屏障。`A/C/M/U` 不参与调用规则——它们只从函数自身的签名或函数体事实获得，其中 A/C/U 不进入后缀。P 同时是端口边界：包含 P 的被调用方在该调用边只向上传播 P。

`Result` 和 `Option` 在类型系统中表达错误或缺失流程；返回、匹配或用 `?` 传播它们都不会增加能力字母。

## 处理流程

1. rustc lint pass 从 HIR 收集函数签名、静态状态、源码位置和直接调用边
2. 将各 crate 的事实合并为函数语义图
3. 从签名/函数体事实、Port 规则、trait 投票和 capsmap 精确条目推断 semantic caps（函数名不参与）
4. 在函数图上传播可传播能力到固定点
5. 使用同一个离线能力引擎检查调用关系、静态状态和视图一致性
6. 直接 rustc/UI 模式把当前 crate 的诊断映射为 rustc lint；`cargo rivus check` 输出全项目诊断

推断与消费的每一层都不读名字：resolver 的能力来源只有 Port 结构、capsmap 精确条目、bodyless 签名+投票、有 body 节点的推断结果和 trait 多数投票；固定点 seed 只来自 capsmap。direct rustc 模式的覆盖分类使用签名/函数体事实加结构性 P（传播闭包属于离线引擎）；静态状态检查只对 World Port 实现体与默认 trait body 对照 voted 契约执行——普通函数的同类缺陷由 naming view 的 Contract 诊断唯一承接，不再重复报告。未测试诊断的分类标签（good/ok）由离线引擎按 semantic caps 判定并随 selection 传递，发射编译不再从签名事实重分类；incomplete lower bound 不构成分类证明，其覆盖候选被跳过。

命名契约不一致只有一套语义分类。离线报告直接携带推断阶段产生的 contract kind，并由输出层分别映射为稳定诊断 code 或 rustc lint；输出层不得复制另一套 `MissingBlocking`、`MissingIo` 等分类枚举。诊断与报告统计共用同一个 kind 选择规则：仅缺字母时由 Missing\* 承接、NameMismatch 沉默；期望视图含 P 或 actual 后缀带多余字母时 NameMismatch 参与。

不完整知识下的视图比较规则：known lower bound 中存在而视图缺失的能力可以报 missing；视图中存在而 lower bound 未证明的能力不能报 extra——它可能来自尚未证明的传播。A/C/U、未知字母、重复、乱序不依赖 completeness，始终可报。bodyless 函数没有函数体、impl 投票或 capsmap 条目时保持 unknown，即使名字写了 `_BI` 也不从名字补全。

能力集合和能力知识是不同概念。调用检查消费能力集合；解释和设计反馈还消费能力来源、完整度与 trait 投票证据。旧记录迁移不得伪造已经丢失的证据。详见 `capability-knowledge.md`。

## 目标身份与诊断归属

同一路径的函数可以同时出现在 production、test、不同 feature 或不同 crate target 中。扁平合并图按 `DefPath` 合并所有 Cargo target 的 artifact，各节点的行为、调用边、事实、角色和源码取保守并集。任何消费者需要 target 身份、调用、事实、body、角色、归属或源码时都读取同一合并节点。

诊断锚定实际具有违规行为的合并节点。能力和完整度都是单调增长的有限状态，使用受影响节点工作队列传播。诊断阶段遍历一次，输出使用有序集合保证可复现。

## 错误返回与所有权

`Result<(), E>` 的所有权诊断只看签名。只要 `E` 可构造，就保守认为函数可能失败；owned、非 Copy 输入必须由错误类型保留。即使当前函数体只返回 `Ok(())` 也不做控制流证明。引用、Copy 输入和不可构造的错误类型不触发该规则。

## 依赖能力生成

生成 `caps/deps` 时，已有 `deps` 是待替换的输出，不是推断输入。推断读取其他 caps 层作为已知事实，因此旧 `deps` 即使损坏也不能阻止重新生成；其他输入层损坏时仍须在收集函数图前失败。

`infer-capsmap` 和 `infer-std` 都只写调用者显式指定的输出。active `caps/` 目录中的自动输出只能分别是规范层 `deps` 和 `std`；自定义输出必须位于该目录之外，避免加载顺序更靠后的自定义层覆盖人工 `ext` 修正。两者必须在验证项目和 caps 输入之后、收集函数图之前解析并校验输出路径；非法目录、符号链接、错误层角色或路径穿越不能触发普通 callgraph 编译或 nightly build-std。输出 preflight 必须把父目录 descriptor 作为 publication authority 传入整个长时推断；发布、exchange、rollback 和 cleanup 都相对该 descriptor 完成，不能在推断结束后重新打开词法路径。
