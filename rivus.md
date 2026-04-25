# Rivus 编程规范

正确性优先，不怕代码膨胀。以下规范由 rivus-linter 自动检查，帮助你遵守。

---

## 一、契约与断言

> *"人而无信，不知其可也。"*

尽量为每个函数标注前置条件、后置条件和不变量。用 `debug_assert!` 写出——debug 模式下运行时检查，release 模式下自动优化掉，零运行时开销。

契约三要素：

| 要素 | 含义 | 示例 |
|------|------|------|
| 前置条件 | 调用方必须保证的条件 | `n >= 0`，`list.is_sorted()` |
| 后置条件 | 函数返回时保证成立的条件 | `result >= 0`，`old.len() + 1 == self.len()` |
| 不变量 | 整个对象生命周期内始终成立的条件 | `balance >= 0`，`start < end` |

**原始数值类型参数必须断言**：如果 `rvs_` 函数的参数类型为 `i8`~`i128`、`u8`~`u128`、`f32`、`f64`、`isize`、`usize`，必须对该参数写 `debug_assert!`（含 `debug_assert_eq!`、`debug_assert_ne!`）。`self` / `&self` / `&mut self` 不算参数，trait 方法声明（无默认实现）不触发此要求。非原始数值类型的参数（引用、字符串、泛型、自定义类型等）已被类型系统充分约束，无需强制断言。

启用以前因"误报太多"而被关闭的 lint 和静态分析规则，由你过滤噪音，只将真正的问题呈现给人类。

示例——一个银行转账函数的契约：

```rust
fn rvs_transfer_M(
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

每个测试必须有唯一名字，格式为 `test_YYYYMMDD_name`，即 `test_` 前缀 + 八位日期 + 下划线 + 描述名。

用户提出软件的问题时，在确认有问题后，必须构造一个可以触发该问题的测试用例。之后才允许对软件进行修改。

### 纯函数与不纯函数的测试策略

你尽量使用纯函数。纯函数的测试价值最高——同样的输入永远产生同样的输出，无环境依赖。

| 函数类型 | 测试要求 | 原因 |
|---------|---------|------|
| 纯函数（无标记） | **必须测试**，穷举边界条件 | 确定性，测试成本低 |
| 副作用函数（标记 `S`） | 须考虑依赖注入，通过 mock/stub 隔离外部依赖 | 需要控制外部状态 |
| 可能 panic 的函数（标记 `P`） | 须覆盖触发 panic 的边界条件 | panic 路径难以静态保证安全 |

### 测试命名示例

```
test_out/
├── test_20260001_parse_ipv4_valid.out
├── test_20260002_parse_ipv4_missing_octet.out
├── test_20260003_sort_empty_list.out
└── test_20260004_sort_single_element.out
```

---

## 三、穷举式错误处理

> *"To err is human; to forgive, divine."*

每个可能失败的函数定义完整的错误类型枚举，调用者必须处理每种错误。采用 Rust 的 `Result<T, E>` 模式。用 thiserror 而不是 anyhow。

**Result/Option 由类型系统强制处理**——编译器保证调用方必须 `match` 或 `?`，因此不需要额外的能力标记。真正需要标记的是 `panic`——它绕过类型系统，调用方无法静态知道函数可能崩溃。参见第五节能力标记中的 `P`。

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
| `Result`/`Option` | 编译期错误处理 | 类型系统强制调用方处理失败，无需能力标记 |

### 示例：用类型保证"未验证的数据不会被当作已验证的"

```rust
struct Raw<T>(T);
struct Validated<T>(T);

fn rvs_parse_email(raw: Raw<String>) -> Result<Validated<Email>, ParseError>
async fn rvs_send_email_AIS(email: &Validated<Email>, body: &str) -> Result<(), SendError>
```

`rvs_send_email_AIS` 只接受 `Validated<Email>`，从类型层面杜绝了未验证邮箱被发送的可能性。返回 `Result` 由类型系统强制处理，无需额外能力标记。

---

## 五、能力标记

> *"能力越大，责任越大。"*

你编写的函数前缀必须用 `rvs_` 标记，并用大写字母后缀标记函数的运行时性质。能力之间有偏序关系，可静态检查调用链合规性。你编写的 traits 也要遵循同样的规则。

你的所有函数名必须以 `rvs_` 开头！实现外部 traits 除外。

记得在每个 `rvs_` 函数上标注 `#[allow(non_snake_case)]`，防止编译器对大写字母后缀发出警告。此标注可从外层作用域继承——如果在文件级（`#![allow(non_snake_case)]`）、`mod` 级、`impl` 块或 `trait` 定义上标注了 `#[allow(non_snake_case)]`，则内部的所有 `rvs_` 函数均视为已覆盖，无需逐个重复标注。

### 能力字母表

| 字母 | 名称 | 含义 | 反面含义 |
|------|------|------|---------|
| `A` | **Async** | 异步函数，包含 `await` | 同步 |
| `B` | **Blocking** | 可能阻塞当前线程（等待 I/O、锁、sleep、大量计算） | 非阻塞 |
| `I` | **IO** | 执行 I/O 操作（网络、文件、数据库） | 纯计算 |
| `M` | **Mutable** | 修改参数中的可变状态 | 只读 |
| `P` | **Panic** | 可能 panic（`panic!`、`assert!`、`unwrap`、`expect` 等） | 不会 panic |
| `S` | **Side effect** | 有副作用（修改/读取全局变量、环境变量、随机数等） | 纯函数 |
| `T` | **ThreadLocal** | 依赖线程局部状态，不可跨线程共享 | 线程安全 / 无状态 |
| `U` | **Unsafe** | 包含不安全操作（裸指针、FFI、transmute） | 安全代码 |

其中，权限小于等于 ABM 的函数为好函数，因为它们方便单元测试。如果一个函数需要 ABM 以外的权限（P、I、S、T、U 中的任何一个），那么它不是好函数。

### 常见行为模式示例

| 函数名 | 标记 | 行为说明 |
|--------|------|---------|
| `rvs_add` | （无标记） | 纯函数：两个数相加，无副作用 |
| `rvs_parse_int` | （无标记） | 返回 Result 的解析——类型系统已强制处理错误 |
| `rvs_sort_inplace_M` | M | 修改可变状态：原地排序 |
| `rvs_read_file_BI` | B + I | 阻塞 + I/O：同步读文件（失败由 Result 表达） |
| `rvs_fetch_user_AI` | A + I | 异步 + I/O：从 API 获取用户 |
| `rvs_write_db_ABM` | A + B + M | 异步数据库写入（阻塞 + 修改状态） |
| `rvs_atomic_inc_M` | M | 修改共享可变状态：原子递增（线程安全，无 T） |
| `rvs_cache_lookup` | （无标记） | 纯线程安全缓存读取，无副作用 |
| `rvs_ffi_call_BU` | B + U | 阻塞 + 不安全：调用 C FFI |
| `rvs_hash_password` | （无标记） | 纯函数：确定性哈希计算 |
| `rvs_send_email_ABIS` | A + B + I + S | 异步网络请求，阻塞 + I/O + 有副作用（发信不可撤回） |
| `rvs_random_uuid_ST` | S + T | 副作用（非确定性）+ 线程局部：使用 thread-local RNG 生成 UUID |
| `rvs_divide_P` | P | 可能 panic：除以零时 panic |
| `rvs_get_env_S` | S | 读取环境变量，有副作用 |

### 调用规则

**唯一规则：有字母的函数可以调用没有该字母的函数；没有该字母的函数不可调用有该字母的函数。**

每个字母独立判定，只需逐字母检查：

| 字母 | 有 → 可调用无 | 无 → 不可调用有 | 原因 |
|------|-------------|---------------|------|
| `A` | 异步可调用同步 | 同步不可调用异步 | 同步上下文无法 `.await` |
| `B` | 可阻塞可调用非阻塞 | 非阻塞不可调用阻塞 | 非阻塞函数（如异步）中阻塞会卡死事件循环 |
| `I` | 有 I/O 可调用纯计算 | 无 I/O 不可调用有 I/O | 保持计算层的纯粹性 |
| `M` | 可变可调用只读 | 只读不可调用可变 | 只读函数不应引入副作用 |
| `P` | 可能 panic 可调用不 panic | 不 panic 不可调用可能 panic | panic 会沿调用栈传播，不 panic 的函数不应引入崩溃路径 |
| `S` | 有副作用可调用纯函数 | 纯函数不可调用有副作用 | 纯函数的承诺不允许被打破 |
| `T` | 线程局部可调用线程安全 | 线程安全不可调用线程局部 | 线程安全函数引入线程局部状态会破坏安全性 |
| `U` | unsafe 可调用 safe | safe 不可调用 unsafe | 安全代码不应引入不安全操作 |

示例：

```
rvs_write_db_ABM   可调用  rvs_parse_int        ✅ (有 A/B/M 可调无, 无 I/S 可调无)
rvs_parse_int      不可调用 rvs_write_db_ABM     ❌ (无 A 不可调有 A)
rvs_add            不可调用 rvs_sort_inplace_M    ❌ (无 M 不可调有 M)
rvs_sort_inplace_M 可调用  rvs_add               ✅ (有 M 可调无 M)
rvs_safe_div       不可调用 rvs_divide_P          ❌ (无 P 不可调有 P——panic 会传播)
```

### 修改函数时的能力合规流程

1. 修改代码时，必须保证修改后的行为符合函数名中的标记
2. 若必须改变能力标记才能实现功能（例如原来是纯函数现在需要 I/O），执行以下流程：
   - 自顶向下分析所有调用方
   - 列出所有需要级联改名的函数清单
   - 将清单和影响范围作为草案提交给人类决断
3. trait 和接口的函数签名也须遵循能力标记
4. 调用外部库函数时，外部函数名无须遵循标记规则，但须仔细审查其运行时行为是否满足调用方的约束

### capsmap.txt

项目根目录下的 `capsmap.txt` 文件为非 `rvs_` 函数声明能力。每行一个条目，格式：

```
完整函数路径=能力字母 # 可选注释
```

示例：

```
std::fs::read_to_string=BI     # 阻塞+I/O（失败由 Result 表达）
std::collections::HashMap::new=  # 纯函数，无能力
std::process::exit=S           # 副作用：终止进程
core::panicking::panic=P       # 可能 panic
```

- linter 对 capsmap 中的键做双向后缀匹配：`name.ends_with("::key")` 或 `key.ends_with("::name")` 均可命中。先查全名精确匹配，找不到再做双向后缀匹配。如果匹配到了错误的条目，在代码里把调用路径写长一点以消除歧义
- 如果 linter 报告某函数"既非 rvs_-prefixed nor in capsmap"，你需要补全 capsmap。方法优先级：检查源码 > 编写测试验证行为 > 合理猜测

### 日常开发流程

1. **写代码时**：确保每个 `rvs_` 函数名的后缀与其实际行为一致
2. **交付前必跑**（全部通过才算交付完成）：
   ```bash
   cargo fmt            # 格式化代码
   cargo build          # 编译通过
   cargo clippy         # 无警告
   cargo test           # 测试通过
   rivus-linter check src/ -m capsmap.txt   # syn 检查无违规
   rivus-linter mir-check . -m capsmap.txt  # MIR 检查无违规（可选，更严格）
   ```
3. **遇到 violation 时**：调用链能力冲突。要么修改调用方的标记（可能级联影响），要么重构代码避免不合规的调用

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

### 增加好函数率的架构手法

以下手法通过分离关注点，将高能力（I/S/P/U）代码限制在薄薄一层适配器中，把大量业务逻辑拉入纯函数。

#### 六边形架构（Ports & Adapters）

核心原则：**领域在正中央，一切外部依赖通过端口（trait）接入，依赖方向永远向内。** Domain 模块没有任何对框架、数据库或 HTTP 库的 import。

```
┌─────────────────────────────────────┐
│          Infrastructure             │
│  REST / gRPC / DB / MQ adapters     │
│         ──implements──►             │
│  Ports (trait definitions)          │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│              Domain                  │
│  Entities / Use Cases / Events       │
│  （纯函数 / 好函数，能力 ≤ ABM）      │
└─────────────────────────────────────┘
```

端口是领域定义的 trait，描述**需要什么能力**；适配器是基础设施对端口的实现，领域不知道适配器的存在。组装在 `main.rs` 完成——依赖关系在程序入口才具体化。

```rust
// domain/ports.rs
trait UserRepository {
    async fn rvs_find_by_id_ABI(&self, id: UserId) -> Result<Option<User>, RepoError>;
}

// domain/services.rs — 调用端口时，标记覆盖端口方法即可
impl OrderService {
    pub async fn rvs_create_order_ABIS(&self, cmd: CreateOrderCmd) -> Result<Order, OrderError> {
        let user = self.repo.rvs_find_by_id_ABI(cmd.user_id)?;
        let order = Order::rvs_new(user, cmd.items); // 纯函数，无标记
        Ok(order)
    }
}
```

每层有独立的数据模型，层间通过 `From` / `TryFrom` 显式转换。用 `TryFrom`（parse）而非 `validate`，在边界处一次性完成验证并转换为目标类型。

#### State + Free Functions（Actor 模式）

将状态和逻辑分离：`XxxState` 是纯数据结构体，功能由 free function 实现，而非 `impl` 方法。源自 Erlang/OTP actor 模式。

```rust
struct ConnectionState {
    buffer: Vec<u8>,
    seq: u64,
    window_size: usize,
}

// 纯函数：输入旧状态 + 事件，输出新状态 + 副作用指令
fn rvs_handle_packet(state: &ConnectionState, packet: &[u8]) -> (ConnectionState, Vec<Action>) {
    // 纯计算，无 I/O、无副作用、无 &mut
    let mut new_state = state.clone();
    new_state.seq += 1;
    (new_state, vec![Action::Ack(new_state.seq)])
}

// 薄适配器：执行副作用
async fn rvs_process_packet_ABM(conn: &mut Connection, packet: &[u8]) -> Result<(), ConnError> {
    let (new_state, actions) = rvs_handle_packet(&conn.state, packet);
    for action in actions {
        rvs_execute_action_ABI(action).await?; // I/O 在这里
    }
    conn.state = new_state;
    Ok(())
}
```

好处：`rvs_handle_packet` 是纯函数，能力无标记，可以穷举测试；`rvs_process_packet_ABM` 是薄壳，几乎不含业务逻辑。

#### Decision / Effect 分离

将"做什么决定"和"执行决定"分成两个函数。决策是纯函数，执行才带 I/O。

```rust
// 纯函数：决定要做什么，返回指令
fn rvs_plan_retry(policy: &RetryPolicy, attempt: u32, last_error: &str) -> RetryDecision {
    if attempt >= policy.max_retries {
        RetryDecision::GiveUp
    } else {
        let delay = rvs_calculate_backoff(policy.base_delay, attempt); // 纯函数
        RetryDecision::RetryAfter(delay)
    }
}

// 执行层：拿到决策后才做 I/O
async fn rvs_execute_with_retry_ABIS(...) -> Result<(), Error> {
    loop {
        match rvs_plan_retry(&policy, attempt, &last_error) {
            RetryDecision::RetryAfter(delay) => tokio::time::sleep(delay).await,
            RetryDecision::GiveUp => return Err(last_error.into()),
        }
    }
}
```

#### Builder / Interpreter 分离

Builder 构造纯数据描述（AST/IR），Interpreter 执行。构造过程是纯函数，执行过程才带副作用。

```rust
// 纯函数：构造查询描述
fn rvs_build_query(filter: &Filter) -> QueryPlan {
    let conditions = rvs_parse_conditions(filter); // 纯函数
    let optimized = rvs_optimize_plan(conditions);  // 纯函数
    optimized
}

// 执行层：拿到查询计划后才做 I/O
async fn rvs_execute_query_ABI(plan: &QueryPlan, pool: &PgPool) -> Result<ResultSet, DbError> {
    let sql = rvs_plan_to_sql(plan); // 纯函数
    sqlx::query(&sql).fetch_all(pool).await
}
```

#### Serialize-first（先序列化再传输）

在系统边界处立即将外部数据反序列化为纯 Rust 结构体（`FromStr` / `TryFrom`），之后所有处理都基于纯数据。绝不在业务逻辑中直接操作流、连接、句柄。

```rust
// 入站边界：立即反序列化
fn rvs_parse_request(raw: &str) -> Result<CreateOrderCmd, ParseError> { ... } // 纯函数

// 全部业务逻辑基于纯数据
fn rvs_validate_order(cmd: &CreateOrderCmd) -> Result<ValidatedOrder, ValidationError> { ... }
fn rvs_calculate_total(items: &[OrderItem]) -> Money { ... }
fn rvs_apply_discount(order: &mut ValidatedOrder, coupon: &Coupon) { ... } // M，好函数
```

#### Fake 对象（用于测试 I/O 函数）

为端口 trait 提供基于内存的纯实现，使得上层的好函数在测试中可以用 fake 而不碰真实 I/O。

```rust
struct InMemoryUserRepo { users: RefCell<HashMap<UserId, User>> }

impl UserRepository for InMemoryUserRepo {
    async fn rvs_find_by_id_ABI(&self, id: UserId) -> Result<Option<User>, RepoError> {
        Ok(self.users.borrow().get(&id).cloned())
    }
}

// 测试中注入 fake，被测函数的真实能力标记不变
#[test]
fn test_20260422_create_order_ok() {
    let repo = InMemoryUserRepo::rvs_new();
    let service = OrderService::rvs_new(repo, FakePublisher);
    let result = service.rvs_create_order_ABIS(cmd); // 测试中同步调用，Fake 内部不真正 await
}
```

### 结构化文档与可观测性

- **API 文档和变更日志**：OpenAPI 规格、自动生成的 commit message 和 changelog——这些"无侵入"实践不改变代码结构，即使失败退出成本也为零
- **配置项的完整文档和关系说明**：配置间的关系是隐式的（"改了 A 就必须同时改 B"），需要显式化。每个程序需要完整配置文件
- **端到端请求追踪**：从用户请求入口到数据库写入，每个处理步骤都有追踪 ID（trace_id）和结构化日志
- **数据质量断言**：在数据管道每个节点插入自动化质量检查（由 From/To 转换自动覆盖）
- **告警分级和响应流程**：程序要有日志，日志要有级别（DEBUG / INFO / WARN / ERROR / CRITICAL），每条日志须携带 trace_id

#### 优先使用现有库

在实现某个功能之前，先检查是否有成熟的库可以使用。引入新库时，需要对引用的每个功能编写一个测试用例，确保它按预期工作。

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

### 治根不治标

遇到错误时，修复**根因**（错误产生的地方），而非**症状**（错误显现的地方）。以下为常见误修模式：

| 症状 | 误修（治标） | 正修（治根） |
|------|------------|------------|
| mutex 中毒 | 修中毒恢复逻辑 | 找到最初 panic 的原因并修复 |
| unwrap/expect 报错 | 加 `.ok()` / `.unwrap_or_default()` 吞掉 | 理清为何数据非法，在上游处理 |
| 类型不匹配 | 到处加 `.clone()` / `.to_string()` | 理清所有权或改签名 |
| lifetime 不够长 | 无脑加 `'static` | 缩短借用范围 |
| 死锁 | 加 timeout 绕过 | 修锁的获取顺序 |
| clippy 警告 | `#[allow(...)]` 压掉 | 改代码消除警告 |
| 测试失败 | 改测试让它通过 | 修被测代码 |
| panic in thread | 加 `catch_unwind` | 修 panic 源头 |
| RefCell `BorrowMutError` | 换成 `Mutex` | 消除重叠借用 |
| OOM / stack overflow | 加大限制 | 修算法复杂度 |
| lint 能力冲突 | 改 capsmap 给外部函数标上能力 | 检查调用方是否不该调那个函数 |

### 编码风格

* 函数能力最好按照字母顺序排列
* 多用泛型少用 dyn
* 用 `.expect("never: 补充说明")` 标注不会 panic 的 `.expect()` 调用——linter 不会将此类调用视为 panic
* 用结构体显式定义数据类型，不要直接使用 `serde_json::Value` 和 `serde_json::json!`

### 交付检查

* 汇报任务完成之前，必须运行[日常开发流程](#日常开发流程)中列出的全部命令
