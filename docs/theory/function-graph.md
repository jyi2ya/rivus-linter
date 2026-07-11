# 函数语义图

## 实体

**函数语义图**由一组**函数节点**和**调用边**组成。

- 一个节点代表一个函数、方法或 trait 方法。
- 一条边代表一个函数直接调用另一个函数。

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
- 可写回源码的函数名范围；相对文件名同时记录编译器解析它时使用的精确基准目录

推断包括：

- 对外公开能力集合
- 调用传播后得到的能力集合

## 构建过程

构建图时，只记录**事实**，不做规则裁决。

- lint pass 负责从 HIR 收集节点和边
- callgraph artifact 负责把这些节点持久化；artifact 使用显式 schema version，读取时同时兼容当前版本之前没有 envelope 的旧图
- 每个参与收集的 crate 都必须成功写出自己的 artifact；任一写入失败都使本次收集失败，不能用其他 crate 的部分图继续分析
- 源码写回只使用 artifact 记录的路径基准；旧 artifact 没有基准时允许兼容解析，但多个候选都存在则拒绝猜测
- 同一命令已经确定本地 crate 边界后，后续报告和缓存过滤复用这一份边界快照，不在执行中途重新解释项目范围
- 一次本地分析只执行一次 Port scope、能力推断、impl 索引和契约差异构建；各输出视图复用同一份派生结果，不能分别重建可能漂移的分析上下文

## Lint 分层

源码检查按分析范围分为三个平级类别：

- **node lint**：只依赖当前 HIR 节点、签名、属性或源码范围，直接完成判断
- **body lint**：每个函数体只遍历一次并生成 `BodyFacts`，各规则只解释共享事实，不再自行遍历函数体
- **caps lint**：把签名事实和 `BodyFacts` 投影为函数图事实，再由跨函数、跨 crate 的离线能力引擎统一推断

body collector 必须进入 closure、async block 等嵌套 body，否则嵌套代码中的调用和行为会漏报。进入嵌套 body 是统一 body 遍历设施的职责，不属于任一具体 lint。callgraph、测试调用识别和 body lint 必须消费同一份调用观察，但可以按各自语义选择 canonical target、源码方法名或 unresolved path。方法解析失败时仍保留独立的 unresolved-method 观察，不能让语法级检查和测试覆盖因类型解析缺失而消失。

普通 HIR 表达式的直接子节点关系只维护一份。block/loop 和 closure 属于带上下文的边界：block 负责 statement、let-else 和尾表达式，closure 通过独立 body 解析并增加嵌套深度，不能被普通子节点遍历扁平化。

free function、impl method 和带默认实现的 trait method 共享同一条 body-bearing 处理流水线；各函数来源只提供测试、文档和 Port 等策略差异。无函数体的 required trait method 只投影签名事实，不能用空 body facts 伪装成已观察的函数体。

## 测试覆盖

测试是否覆盖函数，取决于调用实际指向的函数，而不是调用处书写的别名。导入重命名不能让真实调用失去测试覆盖，也不能让另一个函数借用同名别名伪造覆盖。方法调用在无法唯一确定动态目标时仍按方法名匹配。

## 推断过程

推断阶段读取整张图，根据统一规则生成独立的派生结果，不把期望能力、期望名称或推断出的外部函数写回事实节点：

- Port 方法对外只有 `P`
- Port 是当前工作区对六边形架构端口的特别约定，只对当前命令认定为本地的 crate 生效。依赖库和标准库中即使 trait 名以 `Repository` 或 `Client` 结尾，也不自动获得 `P`，而是按 capsmap、显式后缀和实际行为处理。
- `A/M/U` 只由签名事实决定，不通过调用传播
- `B/I/P/S/T` 从被调用方传播到调用方
- 外部函数通过 capsmap 补全能力
- 本地非 Port trait 方法的公开能力由各 impl 的传播能力做“至少半数”聚合；声明自身写的能力后缀只在没有 impl 可聚合时作为回退。两个 impl 中只要一个具备某传播能力，该能力就会出现在 trait 方法的聚合结果中。这是经验性折中，不是严格多数投票，也不是完整的 over-approximation。
- Port trait 方法例外：公开能力固定为 `P`，不受 impl 的实际 I/O、副作用或阻塞行为影响。

## 差异

能力诊断、annotate、why、report 不再各自发明一套解释，而是基于同一张图做不同视图。lint pass 只从 HIR 收集能力事实；能力契约、后缀、静态状态和调用边诊断统一由离线能力引擎计算。直接 rustc/UI 模式使用当前 crate 的内存图，`cargo rivus check` 使用合并后的全项目图。

各视图共享函数的本地范围、入口点、测试、trait impl、Port、源码和生成代码分类，但保留具名的视图策略；contract、offline、report 和 rename 不得因复用分类而被压成同一套筛选条件。

- **lint**：收集事实，并把统一能力引擎的当前 crate 诊断映射为 rustc lint
- **annotate**：把期望名字写回源码
- **why**：展示节点能力和边上的来源
- **report**：从 fresh function graph 的 report metadata 聚合能力分布和 contract mismatch 摘要
