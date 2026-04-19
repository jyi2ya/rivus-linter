---
name: rivus-coding-skill
description: 宝宝你应该这么写代码
---

# 你的编程守则

你使过去因维护成本过高而不可行的工程实践变得可行。本 skill 定义了这些实践——正确性优先，不怕代码膨胀。

---

## 一、契约与断言

> *"人而无信，不知其可也。"*

尽量为每个函数标注前置条件、后置条件和不变量。用 `debug_assert!` 写出——debug 模式下运行时检查，release 模式下自动优化掉，零运行时开销。你可以从实现自动逆推契约，并在每次代码变更时同步更新。

**rivus-linter 会自动检查**：如果 `rvs_` 函数的参数类型为原始数值类型（`i8`~`i128`、`u8`~`u128`、`f32`、`f64`、`isize`、`usize`）但未对该参数写 `debug_assert!`（含 `debug_assert_eq!`、`debug_assert_ne!`），则发出警告。`self` / `&self` / `&mut self` 不算参数，trait 方法声明（无默认实现）不触发此检查。非原始数值类型的参数（引用、字符串、泛型、自定义类型等）已被类型系统充分约束，无需强制断言。

启用以前因"误报太多"而被关闭的 lint 和静态分析规则，由你过滤噪音，只将真正的问题呈现给人类。

契约三要素：

| 要素 | 含义 | 示例 |
|------|------|------|
| 前置条件 | 调用方必须保证的条件 | `n >= 0`，`list.is_sorted()` |
| 后置条件 | 函数返回时保证成立的条件 | `result >= 0`，`old.len() + 1 == self.len()` |
| 不变量 | 整个对象生命周期内始终成立的条件 | `balance >= 0`，`start < end` |

示例——一个银行转账函数的契约：

```rust
fn rvs_transfer_E(
    from: &mut Account,
    to: &mut Account,
    amount: Money,
) -> Result<(), TransferError> {
    debug_assert!(from.id != to.id, "不能向自己转账");
    debug_assert!(amount > Money::ZERO, "转账金额须为正");
    debug_assert!(from.balance >= amount, "余额不足");

    let total_before = from.balance + to.balance;
    from.balance -= amount;
    to.balance += amount;

    debug_assert!(from.balance >= Money::ZERO);
    debug_assert!(to.balance >= Money::ZERO);
    debug_assert_eq!(from.balance + to.balance, total_before, "转账前后总额不变");

    Ok(())
}
```

---

## 二、测试之道

> *"Trust, but verify."*

### 测试结构

你必须编写详细的单元测试。出现 bug 时，将触发 bug 的奇怪输入编写为回归测试。采用快照测试方法，将每个测试的输出保存在项目根目录下的 `test_out` 目录中。

每个测试必须有唯一名字，格式为 `YYYYMMDD_test_name`，其中前八位是日期

用户提出软件的问题时，在确认有问题后，必须构造一个可以触发该问题的测试用例。之后才允许对软件进行修改。

### 纯函数与不纯函数的测试策略

你尽量使用纯函数。纯函数的测试价值最高——同样的输入永远产生同样的输出，无环境依赖。

| 函数类型 | 测试要求 | 原因 |
|---------|---------|------|
| 纯函数 | **必须测试**，穷举边界条件 | 确定性，测试成本低 |
| 不纯函数（标记 `P`） | 须考虑依赖注入，通过 mock/stub 隔离外部依赖 | 需要控制外部状态 |

### 测试命名示例

```
test_out/
├── 20260001_parse_ipv4_valid.out
├── 20260002_parse_ipv4_missing_octet.out
├── 20260003_sort_empty_list.out
└── 20260004_sort_single_element.out
```

---

## 三、穷举式错误处理

> *"To err is human; to forgive, divine."*

每个可能失败的函数定义完整的错误类型枚举，调用者必须处理每种错误。采用 Rust 的 `Result<T, E>` 模式。用 thiserror 而不是 anyhow。你可以穷举失败模式并生成对应错误类型。

### 错误类型设计原则

- 每个模块/领域定义自己的错误枚举
- 错误变体应当穷举所有可能的失败模式，不留 `Unknown` 或 `Other` 之类的兜底（除非是 FFI 边界）
- 错误类型携带足够的上下文信息用于诊断
- 上层模块可以将下层错误 `#[from]` 包装，形成错误链

### 示例

```rust
#[derive(Debug, thiserror::Error)]
enum UserRepoError {
    #[error("user {id} not found")]
    NotFound { id: UserId },
    #[error("duplicate email: {email}")]
    DuplicateEmail { email: String },
    #[error("database connection failed")]
    ConnectionFailed(#[from] DbError),
    #[error("user {id} is suspended, reason: {reason}")]
    Suspended { id: UserId, reason: String },
}
```

调用方必须 match 每个变体，或显式传播（`?`）。不允许"吞掉"错误。

---

## 四、类型之力

> *"Make illegal states unrepresentable."*

你用类型系统编码业务规则，使无效状态根本无法通过编译。你可以自动生成这类"类型体操"代码，人类只需看接口。拿到类型后无须检查即可使用——类型本身就是保障。

### 核心手法

| 手法 | 用途 | 示例 |
|------|------|------|
| 可辨识联合 | 排斥互斥状态 | `enum Payment { Pending, Completed(Receipt), Failed(Reason) }` |
| 类型状态模式 | 编译期状态机 | `Uninitialized -> Configured -> Running -> Stopped` |
| `NonZero` / 精化类型 | 排除无效值 | `NonZeroU32` 保证除法安全 |
| newtype 模式 | 防止混淆同类型 | `struct UserId(u64)` 与 `struct OrderId(u64)` 不可混用 |
| 幽灵类型参数 | 编译期标记 | `PhantomData<Validated>` vs `PhantomData<Unvalidated>` |

### 示例：用类型保证"未验证的数据不会被当作已验证的"

```rust
struct Raw<T>(T);
struct Validated<T>(T);

fn rvs_parse_email_E(raw: Raw<String>) -> Result<Validated<Email>, ParseError>
async fn rvs_send_email_AEI(email: &Validated<Email>, body: &str) -> Result<(), SendError>
```

`rvs_send_email_AEI` 只接受 `Validated<Email>`，从类型层面杜绝了未验证邮箱被发送的可能性。

---

## 五、能力标记

> *"能力越大，责任越大。"*

你编写的函数前缀必须用 `rvs_` 标记，并用大写字母后缀标记函数的运行时性质。能力之间有偏序关系，可静态检查调用链合规性。你编写的 traits 也要遵循同样的规则。

你的所有函数名必须以 `rvs_` 开头！实现外部 traits 除外

记得在全局允许非 snake case 的函数名，防止编译器警告。

### 能力字母表

| 字母 | 名称 | 含义 | 反面含义 |
|------|------|------|---------|
| `A` | **Async** | 异步函数，包含 `await` | 同步 |
| `B` | **Blocking** | 可能阻塞当前线程（等待 I/O、锁、sleep、大量计算） | 非阻塞 |
| `E` | **Fallible** | 可能返回错误 / 抛出异常 | 不会失败 |
| `I` | **IO** | 执行 I/O 操作（网络、文件、数据库） | 纯计算 |
| `M` | **Mutable** | 修改参数中的可变状态 | 只读 |
| `P` | **imPure** | 有副作用（修改/读取全局变量、环境变量、随机数等等） | 纯函数 |
| `T` | **ThreadLocal** | 依赖线程局部状态，不可跨线程共享 | 线程安全 / 无状态 |
| `U` | **Unsafe** | 包含不安全操作（裸指针、FFI、transmute） | 安全代码 |

其中，权限小于等于 ABEM 的函数为好函数，因为它们方便单元测试。如果一个函数需要 ABEM 以外的权限，那么它不是好函数

### 常见行为模式示例

| 函数名 | 标记 | 行为说明 |
|--------|------|---------|
| `rvs_add` | （无标记） | 纯函数：两个数相加，无副作用 |
| `rvs_parse_int_E` | E | 可能失败：解析字符串为整数 |
| `rvs_sort_inplace_M` | M | 修改可变状态：原地排序 |
| `rvs_read_file_BEI` | B + E + I | 阻塞 + 可能失败 + I/O：同步读文件 |
| `rvs_fetch_user_AE` | A + E | 异步 + 可能失败：从 API 获取用户 |
| `rvs_write_db_ABEI` | A + B + E + I | 异步数据库写入（可能阻塞、可能失败、有 I/O） |
| `rvs_atomic_inc_M` | M | 修改共享可变状态：原子递增（线程安全，无 T） |
| `rvs_cache_lookup` | （无标记） | 纯线程安全缓存读取，无副作用 |
| `rvs_ffi_call_BU` | B + U | 阻塞 + 不安全：调用 C FFI |
| `rvs_hash_password` | （无标记） | 纯函数：确定性哈希计算 |
| `rvs_send_email_ABEIP` | A + B + E + I + P | 异步网络请求，可能失败，有副作用 |
| `rvs_random_uuid_PT` | P + T | 不纯 + 线程局部：使用 thread-local RNG 生成 UUID |

### 调用规则

**唯一规则：有字母的函数可以调用没有该字母的函数；没有该字母的函数不可调用有该字母的函数。**

每个字母独立判定，只需逐字母检查：

| 字母 | 有 → 可调用无 | 无 → 不可调用有 | 原因 |
|------|-------------|---------------|------|
| `A` | 异步可调用同步 | 同步不可调用异步 | 同步上下文无法 `.await` |
| `B` | 可阻塞可调用非阻塞 | 非阻塞不可调用阻塞 | 非阻塞函数（如异步）中阻塞会卡死事件循环 |
| `E` | 可失败可调用不可失败 | 不可失败不可调用可失败 | 不可失败的函数没有错误处理路径 |
| `I` | 有 I/O 可调用纯计算 | 无 I/O 不可调用有 I/O | 保持计算层的纯粹性 |
| `M` | 可变可调用只读 | 只读不可调用可变 | 只读函数不应引入副作用 |
| `P` | 不纯可调用纯 | 纯不可调用不纯 | 纯函数的承诺不允许被打破 |
| `T` | 线程局部可调用线程安全 | 线程安全不可调用线程局部 | 线程安全函数引入线程局部状态会破坏安全性 |
| `U` | unsafe 可调用 safe | safe 不可调用 unsafe | 安全代码不应引入不安全操作 |

**`E` 的补充说明：** 当不可失败的函数确实需要调用可能失败的函数时，必须在调用点完整处理所有错误（match、`?` + 改签为 `_E`、或 `unwrap`），不允许静默忽略。

示例：

```
rvs_write_db_ABEI  可调用  rvs_parse_int_E       ✅ (A 有可调无, E 同, I 有可调无)
rvs_parse_int_E    不可调用 rvs_write_db_ABEI     ❌ (无 A 不可调有 A)
rvs_add            不可调用 rvs_sort_inplace_M     ❌ (无 M 不可调有 M)
rvs_sort_inplace_M 可调用  rvs_add                ✅ (有 M 可调无 M)
```

### 修改函数时的能力合规流程

1. 修改代码时，必须保证修改后的行为符合函数名中的标记
2. 若必须改变能力标记才能实现功能（例如原来是纯函数现在需要 I/O），执行以下流程：
   - 自顶向下分析所有调用方
   - 列出所有需要级联改名的函数清单
   - 将清单和影响范围作为草案提交给人类决断
3. trait 和接口的函数签名也须遵循能力标记
4. 调用外部库函数时，外部函数名无须遵循标记规则，但须仔细审查其运行时行为是否满足调用方的约束

rivus-linter 是一个可以快速检查指定目录是否符合 rvs 函数调用规范的工具。每次代码修改后交付前，都必须运行 rivus-linter 检查有无能力冲突。

### rivus-linter 命令手册

#### `rivus-linter check <path> -m <capsmap>`

基于 syn 的源码分析。递归扫描 `path` 下所有 `.rs` 文件，提取 `rvs_` 函数的定义与调用关系，检查调用链的能力合规性。速度快，适合日常开发中频繁使用。

```bash
# 检查 src 目录，使用 capsmap 映射非 rvs 函数的能力
rivus-linter check src/ -m capsmap.txt
```

退出码：`0` = 无违规，`1` = 有违规，`2` = 运行错误。hint 不影响退出码。

#### `rivus-linter hint <path>`

基于 syn 的推断性扫描。检查 `rvs_` 函数的实际行为是否与名字中的能力标记一致，输出推断性提示（hint）。hint 不影响退出码，仅提醒开发者审查。

目前支持的推断规则：

| 规则 | 检测条件 | 应有能力 |
|------|---------|---------|
| async 函数 | `async fn rvs_...` 无 `A` | `A` |
| unsafe 块/函数 | 函数体含 `unsafe {}` 或 `unsafe fn`，无 `U` | `U` |
| `&mut` 参数 | 参数含 `&mut`（`&mut self` 除外），无 `M` | `M` |
| `&mut self` 方法 | `&mut self` 方法无 `M` | `M` |
| Result/Option 返回 | 返回类型为 `Result` 或 `Option`，无 `E` | `E` |
| static/thread_local! 读取 | 函数体读取 `static` 或 `thread_local!`，无 `P`/`T` | `P`/`T` |
| panic!/assert! 宏 | 函数体含 `panic!`、`assert!` 等，无 `P` | `P` |
| 后缀非字母序 | 能力后缀未按字母排序（如 `_BA` 而非 `_AB`） | 排序警告 |
| 后缀重复字母 | 能力后缀含重复字母（如 `_EE`） | 去重警告 |

```bash
rivus-linter hint src/
```

`check` 和 `mir-check` 子命令也会同时输出 hints。

#### `rivus-linter mir-check <path> -m <capsmap> [--mir-dir <dir>]`

基于 MIR 的分析。编译项目到 MIR 中间表示，从编译器的视角提取函数调用。比 syn 更精确——能看到编译器展开的 trait 方法调用、闭包、运算符重载等 syn 看不到的东西。

```bash
# 完整流程：自动编译到 MIR 再检查
rivus-linter mir-check . -m capsmap.txt

# 跳过编译：直接检查已有的 .mir 文件（用于 CI 或调试）
rivus-linter mir-check . -m capsmap.txt --mir-dir target/debug/deps
```

`path` 必须是包含 `Cargo.toml` 的项目根目录（完整流程），或任意目录（`--mir-dir` 模式）。

#### `rivus-linter report <path>`

统计 `path` 下所有 `.rs` 文件中 `rvs_` 函数的能力分布，输出各能力标记的函数数量和行数占比。好函数（能力 ≤ ABEM）应该越多越好。

```bash
rivus-linter report src/
```

### capsmap.txt 格式

项目根目录下的 `capsmap.txt` 文件为非 `rvs_` 函数声明能力。每行一个条目，格式：

```
完整函数路径=能力字母 # 可选注释
```

示例：

```
std::fs::read_to_string=BEI     # 阻塞+可能失败+I/O
std::collections::HashMap::new=  # 纯函数，无能力
std::process::exit=P # 强副作用
```

- 注释（`#` 后的内容）会被 linter 忽略，可用于标注信任程度
- linter 对 capsmap 中的键做**后缀匹配**：`HashMap::new=` 能匹配 `std::collections::HashMap::new`。如果匹配到了错误的条目，在代码里把调用路径写长一点以消除歧义
- 如果 linter 报告某函数"既非 rvs_-prefixed nor in capsmap"，你需要补全 capsmap。方法优先级：检查源码 > 编写测试验证行为 > 合理猜测

### 日常开发流程

1. **写代码时**：确保每个 `rvs_` 函数名的后缀与其实际行为一致
2. **交付前必跑**（全部通过才算交付完成）：
   ```bash
   cargo build          # 编译通过
   cargo clippy         # 无警告
   cargo test           # 测试通过
   rivus-linter check src/ -m capsmap.txt   # syn 检查无违规
   rivus-linter mir-check . -m capsmap.txt  # MIR 检查无违规（可选，更严格）
   ```
3. **遇到 warning 时**：linter 输出的 warning 表示某个函数调用既非 `rvs_` 前缀也不在 capsmap 中。补全 capsmap 即可消除
4. **遇到 violation 时**：调用链能力冲突。要么修改调用方的标记（可能级联影响），要么重构代码避免不合规的调用
5. **遇到 hint 时**：推断性提示——函数的实际行为暗示应有某能力但名字里没写。审查后决定：补上能力标记（注意级联影响），或确认是误判则忽略

---

## 六、架构之道

> *"合抱之木，生于毫末；九层之台，起于累土。"*

### Theory Building

你编写需求前先用与计算机无关的语言描述被处理实体（订单、数据等）如何被处理，记录到项目根目录下的 `docs/theory/` 目录。

示例——一个电商系统的 theory 文件：

```
docs/theory/
├── order-lifecycle.md     # 订单从创建到完成的生命周期
├── payment-flow.md        # 支付流程：冻结 -> 扣款 -> 释放
└── inventory-reservation.md  # 库存预留与释放的规则
```

`order-lifecycle.md` 的内容应该是纯领域语言，不涉及具体的数据库表或 API 设计。

### 六边形架构（推荐）

你采用六边形架构（Ports & Adapters）作为推荐的系统组织方式。核心原则：**领域在正中央，一切外部依赖通过端口（trait）接入，方向永远向内。**

```
                     ┌──────────────────────────────────────────┐
                     │              Infrastructure              │
                     │  ┌──────┐  ┌──────┐  ┌──────┐  ┌──────┐ │
  HTTP ─────────────►│  │ REST │  │ gRPC │  │  DB  │  │ MQ   │ │
  request            │  │Adapter│  │Adapter│  │Adapter│  │Adapter│ │
                     │  └──┬───┘  └──┬───┘  └──┬───┘  └──┬───┘ │
                     │     │         │         │         │      │
                     │  ┌──▼─────────▼─────────▼─────────▼──┐  │
                     │  │        Inbound / Outbound Ports     │  │
                     │  │         (trait definitions)         │  │
                     │  └──────────────────┬─────────────────┘  │
                     └─────────────────────┼────────────────────┘
                                           │
                     ┌─────────────────────▼────────────────────┐
                     │                 Domain                    │
                     │  ┌──────────┐  ┌──────────┐  ┌────────┐ │
                     │  │ Entities │  │Use Cases │  │ Domain │ │
                     │  │          │  │(app svc) │  │ Events │ │
                     │  └──────────┘  └──────────┘  └────────┘ │
                     └──────────────────────────────────────────┘
```

**依赖方向：外层可依赖内层，内层绝不依赖外层。** Domain 模块没有对任何框架、数据库或 HTTP 库的 import。

目录结构示例：

```
src/
├── domain/
│   ├── mod.rs              # 领域实体、值对象、领域事件
│   ├── ports.rs            # 端口定义（trait）：Repository, EventPublisher, ...
│   └── services.rs         # 领域服务 / 用例（只依赖 ports）
├── adapters/
│   ├── inbound/
│   │   ├── rest.rs         # HTTP handler → 调用 domain services
│   │   └── grpc.rs         # gRPC handler
│   └── outbound/
│       ├── db_repo.rs      # 实现 domain::ports::Repository
│       ├── cache.rs        # 实现 domain::ports::Cache
│       └── mq_publisher.rs # 实现 domain::ports::EventPublisher
└── main.rs                 # 组装层：注入 adapter 到 domain
```

#### 端口（Port）

端口是领域定义的接口（trait），描述领域**需要什么能力**，而非如何实现：

```rust
// domain/ports.rs —— 领域定义自己需要什么
// 出站端口：领域通过这些 trait 与外部世界交互，但不知道背后是什么。
trait UserRepository {
    async fn rvs_find_by_id_ABEI(&self, id: UserId) -> Result<Option<User>, RepoError>;
    async fn rvs_save_ABEI(&self, user: &User) -> Result<(), RepoError>;
}

trait EventPublisher {
    async fn rvs_publish_ABEI(&self, event: DomainEvent) -> Result<(), PublishError>;
}
```

#### 适配器（Adapter）

适配器是基础设施对端口的实现，**领域不知道适配器的存在**：

```rust
// adapters/outbound/db_repo.rs
struct PostgresUserRepo { pool: PgPool }

impl UserRepository for PostgresUserRepo {
    async fn rvs_find_by_id_ABEI(&self, id: UserId) -> Result<Option<User>, RepoError> {
        // 数据库查询：异步(A)、可能阻塞(B)、可能失败(E)、有I/O(I)
        let row: Option<UserRow> = sqlx::query_as(...)
            .bind(id).fetch_optional(&self.pool).await?;
        Ok(row.map(User::from))
    }

    async fn rvs_save_ABEI(&self, user: &User) -> Result<(), RepoError> {
        let row = OrderRow::from(user);
        sqlx::query("INSERT INTO ...")
            .bind(row).execute(&self.pool).await?;
        Ok(())
    }
}
```

#### 组装（Composition）

你在程序入口处将适配器注入领域，依赖关系在此刻才具体化：

```rust
// main.rs —— 组装层：把真实的适配器塞进领域的端口里。
let repo = PostgresUserRepo::new(pool);
let publisher = MqEventPublisher::new(channel);
let order_service = OrderService::new(repo, publisher);
let router = rvs_create_router_ABEI(order_service);
```

领域服务调用端口时，标记必须覆盖端口方法：领域服务 `_ABEI` 可以调用端口 `_ABEI`（每个字母都有，合规）：

```rust
// domain/services.rs —— 用例：创建订单
impl OrderService {
    pub async fn rvs_create_order_ABEIP(
        &self,
        cmd: CreateOrderCmd,
    ) -> Result<Order, OrderError> {
        // ABEIP 可调用 ABEI ✅ (每个字母都覆盖)
        let user = self.repo.rvs_find_by_id_ABEI(cmd.user_id)?;
        let order = Order::new(user, cmd.items);
        self.repo.rvs_save_ABEI(&order)?;
        self.publisher.rvs_publish_ABEI(OrderCreatedEvent::from(&order))?;
        Ok(order)
    }
}
```

#### 边界处的数据转换

每层有独立的数据模型，层间通过 `From` / `TryFrom` 显式转换：

| 层 | 数据模型 | 职责 |
|----|---------|------|
| 入站适配器 | `CreateOrderRequest` / `OrderResponse` | HTTP/gRPC 协议细节 |
| 领域层 | `Order` / `OrderId` / `Money` | 纯业务规则，不依赖任何框架 |
| 出站适配器 | `OrderRow` / `OrderMessage` | 映射数据库列 / 消息格式 |

转换规则：
- 层间交换的只能是纯数据（可安全序列化的数据）
- 除非十分必要，禁止交换文件描述符、锁等特殊作用的数据
- 用 `TryFrom`（parse）而非 `validate`，在边界处一次性完成验证并转换为目标类型

```rust
// 入站边界：HTTP request → 领域命令，parse 而非 validate
impl TryFrom<CreateOrderRequest> for CreateOrderCmd {
    type Error = ValidationError;
    fn try_from(req: CreateOrderRequest) -> Result<Self, Self::Error> { ... }
}

// 领域层内部：纯计算，无任何标记
impl Order {
    pub fn rvs_new(user: User, items: Vec<OrderItem>) -> Self {
        debug_assert!(!items.is_empty());
        // 计算总价、生成 ID、应用规则……
        Order { user, items, ... }
    }
}

// 出站边界：领域类型 → 数据库行，纯转换
impl From<Order> for OrderRow {
    fn from(order: Order) -> Self { ... }
}
```

### 结构化文档与可观测性

- **API 文档和变更日志**：OpenAPI 规格、自动生成的 commit message 和 changelog——这些"无侵入"实践不改变代码结构，即使失败退出成本也为零
- **配置项的完整文档和关系说明**：配置间的关系是隐式的（"改了 A 就必须同时改 B"），需要显式化。每个程序需要完整配置文件
- **端到端请求追踪**：从用户请求入口到数据库写入，每个处理步骤都有追踪 ID（trace_id）和结构化日志
- **数据质量断言**：在数据管道每个节点插入自动化质量检查（由 From/To 转换自动覆盖）
- **告警分级和响应流程**：程序要有日志，日志要有级别（DEBUG / INFO / WARN / ERROR / CRITICAL），每条日志须携带 trace_id

---

## 七、时代范式

> *"The only constant in life is change."*

| 维度 | 人类时代的妥协 | LLM 时代可以做到的 |
|------|---------------|-------------------|
| 代码量 | 精简优先 | 正确性优先，不怕膨胀 |
| 可读性 | 人类可读优先 | 审查可读性优先 |
| 抽象层 | 宁可低一些 | 可以适度提高 |
| 测试 | 只测 happy path | 穷举关键性质 |
| 类型 | 够用就行 | 关键路径用精化类型 |
| 契约 | 重要的加 | 函数都有前置/后置条件 |
| 错误处理 | 通用 Exception | 领域特定的穷举错误类型 |
| 重构 | 慎重 | 可以更频繁，但有验证保障 |

---

## 八、附录

> *"懒得想用什么名言了，就像懒得给下面的指南归类了一样"*

* 懒惰是美德！在想要实现某个功能之前，永远先想想有没有现成的库可以使用。当然，引入现成的库的时候，需要对引用的每个功能编写一个测试用例，确保它如同自己希望的一样工作。
* 函数能力最好按照字母顺序排列
* 多用泛型少用 dyn
* 汇报任务完成之前，必须运行以下命令确保全部通过：
  ```bash
  cargo build
  cargo clippy
  cargo test
  rivus-linter check src/ -m capsmap.txt
  rivus-linter mir-check . -m capsmap.txt
  ```
