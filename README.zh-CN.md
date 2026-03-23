# 跨市场套利监控 (`arbitrage-monitor`)

[English](README.md) | **简体中文**

在 **Polymarket** 与 **Kalshi** 之间，基于标题语义相似度自动配对预测市场，周期性拉取行情与订单簿，检测潜在跨平台套利机会并记录日志的 **Rust 监控程序**（只读分析，不自动下单）。

## 功能概览

- **市场拉取**：通过 Polymarket Gamma API、Kalshi Trade API 获取开放市场列表（可配置分页与上限）。
- **智能匹配**：TF-IDF 风格向量化 + 余弦相似度；支持按 `config/categories.toml` 的类别关键词做约束与加权。
- **套利检测**（见下节）：对匹配市场对**按真实订单簿多档吃单**，在**每腿固定本金上限**下计算**实际占用资金、手续费、Gas 与净利润**；主结果**不是**“只用第一档最优价”简化出来的。
- **状态追踪**：对高相似度配对做持续跟踪，定期全量刷新与增量更新（参数见 `src/query_params.rs`）。

### 套利利润在代码里到底怎么算

1. **真实盘口**：对每个匹配对，分别请求 Polymarket、Kalshi 的**当前订单簿**（HTTP），解析为按价格排序的卖档 `(价, 量)`。
2. **每腿固定本金上限**：两腿各自用最多 **100 USDT**（`main.rs` 里 `trade_amount`）在簿上**逐档吃单**，得到每腿能买到的份数（`calculate_slippage_with_fixed_usdt`）。
3. **对冲规模**：取 **`min(PM 可买份数, Kalshi 可买份数)`**，保证两腿都能按该份数成交。
4. **该份数下的真实成本**：对上述份数 `n`，在两边订单簿上用 **`cost_for_exact_contracts`** **重新精确扫档** → 得到 **`capital_used`**、加权均价 **`pm_avg_slipped` / `kalshi_avg_slipped`**，再扣手续费与 Gas → **`net_profit_100`**。是否算“有机会”主要看 **`net_profit_100`** 是否超过阈值，**不是**只看第一档价差。
5. **和“最优价”的关系**：代码里的 `pm_optimal` / `kalshi_optimal` 只是**同一本真实订单簿**里**第一档（最低）卖价**，用于和深度加权均价比**滑点百分比**，以及结构体里部分**边际**字段（例如 `total_cost` = 两平台第一档价格之和）。**报告里的 100 USDT 场景净利润以第 4 步的全簿扫档结果为准。**

对应实现：`src/main.rs` 的 `validate_arbitrage_pair`、`src/arbitrage_detector.rs` 的 `calculate_arbitrage_100usdt`。
- **日志**：运行期写入 `logs/`，未匹配项可记入 `logs/unclassified/`。

## 环境要求

- **Rust**：1.70+（建议 stable，`edition = "2021"`）
- 网络可访问上述 API（无需登录即可拉取公开行情；若 Kalshi 策略变更请以官方文档为准）

## 快速开始

```bash
# 调试运行
cargo run

# 发布构建（推荐长时间运行）
cargo run --release
```

首次运行前请确保存在 **`config/categories.toml`**（仓库已带示例配置）。

## 配置说明

| 位置 | 作用 |
|------|------|
| `config/categories.toml` | 类别名称、权重与关键词；用于标题分类与匹配约束 |
| `src/query_params.rs` | 请求间隔、分页上限、相似度阈值 `SIMILARITY_THRESHOLD`、`SIMILARITY_TOP_K`、全量刷新周期 `FULL_FETCH_INTERVAL`、解析日窗口 `RESOLUTION_HORIZON_DAYS` 等 |

### 环境变量（可选）

- **`POLYMARKET_TAG_SLUG`**：若设置，Polymarket 侧可按 tag 过滤拉取市场（见 `clients.rs` 实现）。

## 项目结构（简要）

```
src/
  main.rs              # 入口与主循环
  clients.rs           # Polymarket / Kalshi HTTP 客户端
  market_matcher.rs    # 匹配与索引构建
  text_vectorizer.rs   # 文本向量化
  vector_index.rs      # 向量检索
  arbitrage_detector.rs# 套利与滑点相关计算
  query_params.rs      # 全局查询与匹配参数
  validation.rs        # 校验逻辑
  tracking.rs          # 监控周期状态
  ...
config/
  categories.toml      # 类别配置
docs/
  MATCHING_VERIFICATION.md  # 匹配行为说明与验证笔记
```

更细的匹配与索引行为可参考 **[docs/MATCHING_VERIFICATION.md](docs/MATCHING_VERIFICATION.md)**。

## 免责声明

- 本工具仅供学习与研究，**不构成投资建议**。
- 预测市场存在规则差异、结算风险、流动性不足与 API 延迟；展示的收益与滑点为模型估算，**实盘结果可能不同**。
- 使用第三方 API 时请遵守各平台服务条款与适用法规。

## 许可证

未随仓库附带许可证文件时，默认保留所有权利；如需开源请自行添加 `LICENSE`。
