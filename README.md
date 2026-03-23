# Cross-market arbitrage monitor (`arbitrage-monitor`)

**English** | [简体中文](README.zh-CN.md)

A **Rust** service that pairs prediction markets between **Polymarket** and **Kalshi** using title similarity, periodically fetches quotes and order books, flags potential cross-venue spreads, and writes logs. **Read-only analysis** — it does not place orders automatically.

## Features

- **Market ingestion**: Open markets via Polymarket Gamma API and Kalshi Trade API (configurable paging and caps).
- **Matching**: TF-IDF-style vectors and cosine similarity; category keywords in `config/categories.toml` for filtering and boosting.
- **Arbitrage checks**: Uses **best ask from the order book** on matched pairs; optional fixed-notional walk (e.g. 100 USDT) for slippage and estimated net PnL.
- **Tracking**: Keeps watching high-similarity pairs with periodic full refresh and incremental updates (see `src/query_params.rs`).
- **Logging**: Runtime logs under `logs/`; unmatched items may go to `logs/unclassified/`.

## Requirements

- **Rust** 1.70+ (stable recommended, `edition = "2021"`)
- Network access to the above APIs (public market data; if Kalshi policy changes, follow their official docs)

## Quick start

```bash
cargo run              # debug
cargo run --release    # recommended for long runs
```

Ensure **`config/categories.toml`** exists before the first run (included in the repo).

## Configuration

| Path | Purpose |
|------|---------|
| `config/categories.toml` | Category names, weights, keywords for classification and matching |
| `src/query_params.rs` | Request pacing, page limits, `SIMILARITY_THRESHOLD`, `SIMILARITY_TOP_K`, `FULL_FETCH_INTERVAL`, `RESOLUTION_HORIZON_DAYS`, etc. |

### Optional environment variables

- **`POLYMARKET_TAG_SLUG`**: When set, Polymarket fetches may be filtered by tag (see `clients.rs`).

## Repository layout (short)

```
src/
  main.rs               entry + main loop
  clients.rs            Polymarket / Kalshi HTTP clients
  market_matcher.rs     matching + index build
  text_vectorizer.rs    text vectorization
  vector_index.rs       vector search
  arbitrage_detector.rs spread + slippage helpers
  query_params.rs       global tuning constants
  validation.rs         validation
  tracking.rs           monitor cycle state
  ...
config/
  categories.toml
docs/
  MATCHING_VERIFICATION.md
```

See **[docs/MATCHING_VERIFICATION.md](docs/MATCHING_VERIFICATION.md)** for matching/index notes.

## Disclaimer

- For **learning and research only** — not investment advice.
- Prediction markets differ in rules, settlement, liquidity, and API latency; shown PnL/slippage are **estimates** and live results may differ.
- Comply with each platform’s terms of service and applicable laws when using third-party APIs.

## License

If no `LICENSE` file is present, all rights are reserved; add a `LICENSE` file if you intend to open-source the project.
