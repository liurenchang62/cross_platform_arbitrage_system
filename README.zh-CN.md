# 跨市场套利监控 (`arbitrage-monitor`)

[English](README.md) | **简体中文**

在 **Polymarket** 与 **Kalshi** 之间，基于标题语义相似度自动配对预测市场，周期性拉取行情与订单簿，检测潜在跨平台套利机会并记录日志的 **Rust 监控程序**（只读分析，不自动下单）。

## 功能概览

- **市场拉取**：通过 Polymarket Gamma API、Kalshi Trade API 获取开放市场列表（可配置分页与上限）。
- **智能匹配**：TF-IDF 风格向量化 + 余弦相似度；支持按 `config/categories.toml` 的类别关键词做约束与加权。
- **套利检测**：在匹配市场对上，以**订单簿最优卖价**为准计算价差；可选按固定本金（如 100 USDT） walk 盘口估算滑点与净收益。
- **状态追踪**：对高相似度配对做持续跟踪，定期全量刷新与增量更新（参数见 `src/query_params.rs`）。
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
