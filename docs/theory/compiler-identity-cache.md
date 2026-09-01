# Compiler Identity Cache

Rivus 的 lint pass 在一个 rustc 进程内只为一个 crate 服务。`TyCtxt` 及其
所有查询（DefPath、stable crate id、span、expansion data）在 late lint
阶段不可变：同一输入的 identity 构造永远得到同一输出。identity cache
利用这一确定性，在同一 pass 内记忆 `DefId -> def_path`、
`DefId -> impl type identity`、`Span -> source identity` 与
generated-definition 分组结果，避免对同一输入重复遍历、格式化和分配。

约束：

- 生命周期等于一个 lint pass（即一个 crate）。不使用 static、
  thread-local、锁或跨进程复用；pass 结束整体释放。
- 没有失效问题：cache 键是不可变编译器状态的纯函数。不引入 TTL、
  淘汰策略或容量边界；键空间被 crate 的定义数量界定。
- negative 结果同样是确定性纯函数结果，必须与 positive 结果一样缓存
  （dummy span、跨文件 span、非宏生成定义的 `None`）。
- cache 只消除重复计算，不得改变任何输出：def path、identity marker、
  ordinal 分组、artifact JSON、诊断内容与顺序完全不变。
- generated-definition ordinal 分组必须保持 rustc owners 的原始顺序；
  单遍预索引是按序分组的实现手段，不是语义变更。
- cache 属于 driver 桥接层，不进入 artifact schema、caps 层或离线推断；
  下游看不到 cache 的存在。
