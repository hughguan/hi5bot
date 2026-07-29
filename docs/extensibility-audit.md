# Hi5bot Extensibility Audit — Hardcoded Assumptions

> **Date**: 2026-07-31  
> **Scope**: 全代码库排查定投数额、资金池参数、ETF 组合、账户数量等维度的硬编码耦合  
> **Method**: `rg` 全文搜索 + 逐行追溯数据流

---

## 1. ETF 组合可替换性 — 🔴 最差

**当前 5 只 ETF 在编译时被硬编码为 Rust enum。** 更换/增加/减少 ETF 必须修改代码并重新编译。

### 1.1 硬编码的根：`Ticker` enum

```rust
// types.rs:25-37
pub enum Ticker {
    Iwy, Spmo, Rsp, Pff, Vnq,   // ← 5 个变体是编译器已知的
}
```

这个 enum 驱动了以下所有位置的固化：

| 位置 | 代码 | 影响 |
|------|------|------|
| `types.rs:37` | `pub const ALL: [Ticker; 5]` | 固定数组大小 `5` |
| `types.rs:45-61` | `as_str()` / `parse()` | 每个 ticker 需要手动添加 match arm |
| `types.rs:84-116` | `pub const PORTFOLIO: &[AssetMeta]` | 5 个元素，权重固定 20% |
| `types.rs:81` | `pub const TARGET_WEIGHT = 0.20` | 等权假设 |
| `config.rs:76` | `for ticker in Ticker::ALL` | 验证逻辑遍历固定集合 |
| `config.rs:127-136` | sample TOML `[symbol_ids]` | 固定 5 个 key |

### 1.2 `buffer_pool.rs` — 遍历 `PORTFOLIO`，正确

```rust
// buffer_pool.rs:62
for meta in PORTFOLIO { ... }
```
`PORTFOLIO` 是 `&[AssetMeta]` 切片，遍历模式正确。如果 `PORTFOLIO` 能从配置文件加载而非编译时常量，此代码无需改动。

### 1.3 `backtest.rs` — 🔴 严重硬编码

```rust
// backtest.rs:113
struct PortfolioSnapshot {
    shares: [u64; 5],     // ← 固定 5
    cash: Decimal,
    sgov_pool: Decimal,
}

// backtest.rs:138-146
fn prices_from_day(day: &BacktestDay) -> [Decimal; 5] {
    [day.iwy, day.spmo, day.rsp, day.pff, day.vnq]  // ← 逐个列名
}

// backtest.rs:44-49
pub struct BacktestDay {
    pub iwy: f64,    // ← 字段名硬编码为 ticker
    pub spmo: f64,
    pub rsp: f64,
    pub pff: f64,
    pub vnq: f64,
}

// backtest.rs:482, 543, 560
for i in 0..5 { ... }   // ← 三处 `0..5` 硬编码
```

**这是整个系统最脆弱的部分。** `PortfolioSnapshot.shares: [u64; 5]` 意味着 ETF 数量是编译时数组大小，`BacktestDay` 的字段名直接用了 ticker 名称。要换成 6 只或 4 只 ETF，需要改动 ~25 行代码。

### 1.4 `engine.rs` — 遍历正确，但查询 ID 时引用 `Ticker::ALL`

```rust
// engine.rs:191
let ids: Vec<u64> = Ticker::ALL.iter()
    .map(|t| settings.symbol_id(*t))
    .collect::<Result<Vec<_>>>()?;
```
遍历方式正确。但如果增加 ETF，`settings.symbol_id()` 会因新 ticker 不在 `Ticker::ALL` 中而静默失败。实际上需要通过 `Settings` 动态发现 ticker 列表。

### 1.5 `web.rs` — `GET /api/overview` 遍历 `PORTFOLIO`

```rust
// web.rs:98
let positions: Vec<PortfolioPosition> = PORTFOLIO.iter().map(...).collect();
```
跟 `buffer_pool` 一样，遍历切片，模式正确。

---

## 2. 定投数额 — 🟡 部分可配

### 2.1 回测中完全可配

```rust
// backtest.rs:67-68
pub struct BacktestRequest {
    pub monthly_contribution: Option<f64>, // 用户通过 API 传入，默认 $1000
}
```
`POST /api/backtest` 的 JSON body 中可以指定任意值。✅

### 2.2 实盘引擎中无"定投数额"概念

实盘引擎 (`engine.rs`) 不处理定投——它从 Questrade 实时读取账户 USD 现金余额，用 `available_per_trade(cash, M)` 动态计算可部署量。这本身是正确的设计：真实的 RESP/TFSA 账户中的现金余额由用户外部存入，引擎不应假设每月新增金额。

**问题**：如果将来需要模拟"每月仅定投 $X"的约束（比如 TFSA 有年度供款上限），引擎目前没有这个限制。

### 2.3 Hi5e 对比 Hi5 的预算语义仅存在于回测

实盘引擎通过 `hi5e_dynamic_budget(base_budget, zone)` 缩放的是 **buffer pool budget**（`cash / M`），而不是"定投金额"。这在实盘场景下语义略有不同：
- 回测：`$1000 → Normal: $500 deploy, $500 SGOV`
- 实盘：`cash=$50,000, M=10 → base=$5,000 → Normal: $2,500 deploy`

这意味着实盘的 Hi5e 逻辑是在**大资金池**上做比例调节，而非在**月度定投**上做。两种语义都能工作，但需要在文档中明确区分。

---

## 3. 资金池参数 — 🟢 良好

### 3.1 Safety Buffer M

```rust
// config.rs:25-27
#[serde(default = "default_m")]
pub safety_buffer_m: u32,    // ← TOML 可配 ✅
```

`config.toml` 中修改 `safety_buffer_m` 即可调整每笔交易占总现金的比例上限。验证逻辑仅拒绝 `0`。

### 3.2 SGOV 蓄水池

SGOV 蓄水池仅存在于回测 (`backtest.rs`) 的 `PortfolioSnapshot.sgov_pool` 字段。实盘引擎不管理 SGOV——它假设用户在账户外自行持有 SGOV，或通过外部机制管理。这是一个设计选择而非缺陷，但使得实盘 Hi5e 的"蓄水池"逻辑与回测不完全一致。

### 3.3 极端区倍率

```rust
// radar.rs:194-200
pub fn hi5e_dynamic_budget(base: Decimal, zone: ExtremeZone) -> Decimal {
    match zone {
        Normal/Caution  => base * 0.5,   // ← 硬编码
        Panic           => base * 2.0,
        ExtremePanic    => base * 3.0,
    }
}
```

倍率 (0.5×, 2×, 3×) 是硬编码的。如果用户想自定义（例如 Panic 时只打 1.5×），目前无法配置。建议加入 `config.toml`：

```toml
[hi5e_multipliers]
normal = 0.5
caution = 0.5
panic = 2.0
extreme_panic = 3.0
```

---

## 4. 账户数量 — 🟡 硬编码为 2

### 4.1 Settings 结构体

```rust
// config.rs:15-17
pub struct Settings {
    pub resp_account: String,   // ← 命名字段
    pub tfsa_account: String,
    // ...
}

// config.rs:96-97
pub fn accounts(&self) -> [&str; 2] {   // ← 固定 2 个
    [self.resp_account.as_str(), self.tfsa_account.as_str()]
}
```

要增加第三个账户（如 RRSP、Margin），必须：
1. 在 `Settings` 中加新字段
2. 修改 `accounts()` 返回类型
3. 修改 `evaluate_account` 循环逻辑
4. 更新 `config.toml` 示例

### 4.2 state_store 按 account string 存储

`MonthlyStateStore` 使用 `HashMap<String, AccountEntry>`，key 为 `account.to_string()`。这意味着**账户数量本身不受限**，只是 `Settings` 的命名字段方式限制了发现新账户。

### 4.3 已修复 — Questrade API 联动账户发现

`config.rs` 中 `resp_account`/`tfsa_account` 命名字段已替换为 `account_types: Vec<String>`。
启动时通过 `accounts::discover()` 调用 `GET v1/accounts`，按类型过滤并锁定可用账户。

```toml
# config.toml — 新格式
account_types = ["RESP", "TFSA", "RRSP", "Margin"]
```

实现(`src/accounts.rs`)：
- 调用 Questrade `GET v1/accounts` 获取全部账户
- 过滤 `status == "Active"` 且 `kind` 在 `account_types` 白名单中
- Stable sort: primary first, then type, then number
- 无匹配账户时返回 `Error::ConfigParse`

改动量：`config.rs`（移除 2 个命名字段+ `accounts()` 方法）、新增 `accounts.rs`（~100 行）、`main.rs`（添加发现步骤）、`engine.rs`（`run_tick` 签名增加 `&[DiscoveredAccount]` 参数）。

---

## 5. 数据源基础设施 — 🟢 良好

### 5.1 市场数据

| 数据 | 来源 | 可配性 |
|------|------|--------|
| ETF 报价 | Questrade `v1/markets/quotes` | `symbol_ids` in TOML ✅ |
| RSP K 线 | Questrade `v1/markets/candles` | `symbol_ids` in TOML ✅ |
| VIX | Questrade `v1/markets/quotes` | `vix_symbol_id` in TOML ✅ |
| AAII 情绪 | Web scrape | URL 硬编码 ❌ |
| NAAIM 暴露度 | Web scrape | URL 硬编码 ❌ |
| 市场广度 | 未实现 | — |

AAII/NAAIM URL 在 `fetcher.rs` 中硬编码。如果要切到不同的数据源或使用付费 API，需要修改代码。

### 5.2 通知通道

```rust
// notify.rs
pub async fn notify(webhook: &str, reason: &str) {
    // 仅支持 webhook (Slack/Discord generic webhook)
}
```

Spec 中提到的 Telegram/微信/Email 均未实现。通知模块目前只有一个 HTTP POST 通路。

---

## 6. 修改成本矩阵

| 变更 | 涉及文件数 | 代码行数 | 难度 |
|------|-----------|---------|------|
| 修改 M (safety_buffer) | 0（仅 TOML）| 0 | 🟢 无需改码 |
| 修改定投额（回测） | 0（API 参数）| 0 | 🟢 无需改码 |
| 修改账户类型白名单 | 0（仅 TOML）| 0 | 🟢 无需改码 |
| 修改 Hi5e 倍率 | 1 (`radar.rs`) | 4 | 🟢 5 分钟 |
| 替换/增减 ETF（保持5只） | 3 (`types`, `backtest`, `config`) | ~25 | 🟡 30 分钟 + 回归 |
| 增加第二种资产类别 | 全模块 | ~200 | 🔴 需架构重构 |
| 切换数据源 | 1 (`fetcher.rs`) | ~20 | 🟡 30 分钟 |
| 增加 Telegram 通知 | 1 (`notify.rs`) | ~30 | 🟡 1 小时 |

---

## 7. 推荐的重构路线图

### 7.1 短期（不改架构，纯配置化）

1. **Hi5e 倍率移至 `config.toml`** — 4 行改动，最大 ROI
2. **账户改为 `Vec<AccountConfig>`** — 解除 2 账户限制
3. **`BacktestDay` 改为 `HashMap<String, f64>`** — 解除 5 ETF 字段硬编码

### 7.2 中期（数据结构升级）

4. **`PortfolioSnapshot.shares: [u64; 5]` → `HashMap<Ticker, u64>`**  
   使回测引擎支持任意数量 ETF。这是最大的单点改动，影响回测全部 `for i in 0..5` 循环。

5. **`Ticker` enum → trait + `TickerId` newtype**  
   ```rust
   // 当前
   pub enum Ticker { Iwy, Spmo, Rsp, Pff, Vnq }
   
   // 建议
   pub struct TickerId(String);  // 运行时从配置发现
   impl TickerId {
       pub fn as_str(&self) -> &str { &self.0 }
   }
   ```
   这将使 ETF 列表完全由 `config.toml` 的 `symbol_ids` keys 驱动，新 ETF 只需加一行配置。

### 7.3 长期（架构层）

6. **策略模式抽象**  
   将 `strategy.rs` 的 5-signal 状态机和 `radar.rs` 的 3-pillar 分类都抽象为 trait，支持用户自定义信号逻辑。

7. **多资产类别支持**  
   `AssetMeta` 增加 `asset_class`、`currency`、`exchange` 字段，`PortfolioState` 支持多币种余额。

---

## 8. 总结

| 维度 | 可扩展性 | 评级 |
|------|---------|------|
| Safety Buffer M | TOML 配置 | 🟢 |
| 回测定投额 | API 参数 | 🟢 |
| Hi5e 倍率 | 硬编码 | 🟡 |
| 账户数量 | Questrade API 动态发现 | 🟢 |
| ETF 数量 | 编译时 enum (Hi5=5只, 约束) | 🟡 |
| ETF 权重 | 已支持自定义（`AssetMeta.target_weight`）但实际全部 `TARGET_WEIGHT` | 🟡 |
| 数据源 URL | 硬编码 | 🟡 |
| 通知通道 | 仅 webhook | 🟡 |
| 策略逻辑 | 硬编码 | 🔴 |

**核心瓶颈**：`Ticker` enum 和 `[u64; 5]` 数组大小是系统最大的灵活性天花板。一旦 ETF 组合从 5 只变为 6 只，需要改遍类型系统、回测引擎和配置验证。
