# Setup 所有权

## 实体

一个项目已有的策略和配置属于项目维护者。Setup 只能修改自己能够明确证明拥有的内容，不能把“文件名是已知名称”当作整份文件的所有权。

Setup 有两个输出：项目策略文档 `AGENTS.md` 中的一个受管理区域，以及 manifest `Cargo.toml` 中原先缺失的 clippy lint key。`clippy.toml`、`.cargo/config`、`.cargo/config.toml`、`rustfmt.toml`、贡献指南和其他策略文件都不是输出。

## 策略区域

策略区域由两个各自独占一行的 marker 界定：

```text
<!-- BEGIN RIVUS MANAGED SECTION: cargo-rivus setup -->
<!-- END RIVUS MANAGED SECTION: cargo-rivus setup -->
```

marker 外的字节始终属于项目维护者，必须无损保留。没有 marker 时可以在已有内容之后增加一个区域；恰好一对且顺序正确时只能替换区域内部。缺失、重复、倒序或嵌入其他文本的 marker 不能唯一证明所有权，必须在任何写入前作为冲突拒绝。没有显式 force 授权时，未标记内容不能被替换。

受管理区域来自独立的公开消费者模板。仓库自身的维护者流程、LLM 操作要求和问题记录不是消费者项目规则，不能通过 setup 发布。

## Manifest 合并

Manifest 是结构化文档，必须按 TOML 结构合并。Setup 只增加缺失的 clippy lint key；已有 key 及其值属于项目维护者，不能因值不同而替换。无关 key、顺序和注释应在解析器能够保留时保持不变。

普通 table 和 inline table 都有明确结构，可以合并。标量或数组不能冒充 lint table，应作为冲突拒绝。`workspace = true` 把 lint 所有权交给 workspace root，不能再注入 package-local lint key；维护者必须在 workspace 配置规则或显式停止继承。

## 发布

Setup 在写入前完成所有解析、marker 和所有权检查，并再次确认目标内容没有变化。目标必须是 regular file 或尚不存在，不能跟随 symlink。每份新内容先写入同目录临时文件，再原子替换目标。删除被替换内容或回滚生成内容时，操作只接收已打开的拥有者句柄：先把待删除的文件原子领取到独立 tombstone，再以该句柄重新领取并确认 identity；不能把一个已经验证的 pathname 交给普通 unlink。领取后发现对象变化时保留变化内容，不能删除也不能报告成功。清理失败时，要么安全回滚交换，要么报告 replacement 已提交但 cleanup 仍待处理。

两个输出不能由一次文件系统 rename 共同提交。若第一份输出已经发布而第二份失败，必须恢复第一份原内容；恢复失败时同时报告原始失败和恢复失败，不能声称项目未改变。相同输入重复运行必须得到逐字节相同的结果。
