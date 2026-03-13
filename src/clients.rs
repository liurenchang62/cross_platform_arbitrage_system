use crate::event::{Event, MarketPrices};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};  // 移除 FixedOffset
use reqwest::Client;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

struct PriceCacheEntry {
    prices: MarketPrices,
    timestamp: Instant,
}

struct PriceCache {
    entries: Arc<RwLock<std::collections::HashMap<String, PriceCacheEntry>>>,
    ttl: Duration,
}

impl PriceCache {
    fn new(ttl_secs: u64) -> Self {
        Self {
            entries: Arc::new(RwLock::new(std::collections::HashMap::new())),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    async fn get(&self, key: &str) -> Option<MarketPrices> {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(key) {
            if entry.timestamp.elapsed() < self.ttl {
                return Some(entry.prices.clone());
            }
        }
        None
    }

    async fn set(&self, key: String, prices: MarketPrices) {
        let mut entries = self.entries.write().await;
        entries.insert(key, PriceCacheEntry {
            prices,
            timestamp: Instant::now(),
        });
    }
}

#[derive(Clone)]
pub struct PolymarketClient {
    http_client: Client,
    base_url: String,
    price_cache: Arc<PriceCache>,
}

impl PolymarketClient {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new());
        
        Self {
            http_client,
            base_url: "https://gamma-api.polymarket.com".to_string(),
            price_cache: Arc::new(PriceCache::new(60)),
        }
    }

    const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        let tag_slug = std::env::var("POLYMARKET_TAG_SLUG").ok();
        let tag_slug = tag_slug.as_deref().filter(|s| !s.is_empty());
        self.fetch_events_from_gamma(tag_slug, 500).await
    }













    pub async fn fetch_events_from_gamma(
        &self,
        tag_slug: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Event>> {
        let limit = limit.min(200);  //规模
        let limit_str = limit.to_string();
        let mut query = vec![
            ("active", "true"),
            ("closed", "false"),
            ("limit", &limit_str),
        ];

        if let Some(t) = tag_slug {
            if !t.is_empty() {
                query.push(("tag_slug", t));
            }
        }

        let url = format!("{}/events", Self::GAMMA_API_BASE);
        let response = self
            .http_client
            .get(&url)
            .query(&query)
            .send()
            .await
            .context("Failed to fetch Polymarket events from Gamma API")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Gamma API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let data: Vec<serde_json::Value> = response
            .json()
            .await
            .context("Failed to parse Gamma API response")?;

        let mut events = Vec::new();

        for event_data in data {
            let tags: Vec<String> = event_data["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t["slug"].as_str().or_else(|| t["label"].as_str()))
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            let category = if let Some(cat) = event_data["category"].as_str() {
                    Some(cat.to_string())
                } else {
                    None
                };

            if let Some(markets) = event_data["markets"].as_array() {
                for market in markets {
                    // 使用字段进行筛选：未关闭且未结算
                    let is_closed = market["closed"].as_bool().unwrap_or(true);
                    let is_resolved = market["umaResolutionStatus"].as_str() == Some("resolved");
                    
                    if is_closed || is_resolved {
                        continue;
                    }

                    let event_id = market["id"].as_str().unwrap_or_default().to_string();
                    let question = market["question"].as_str().unwrap_or_default().to_string();
                    
                    let mut yes_price = 0.0;
                    let mut no_price = 0.0;
                    if let Some(prices_str) = market["outcomePrices"].as_str() {
                        if let Ok(prices) = serde_json::from_str::<Vec<String>>(prices_str) {
                            if prices.len() >= 2 {
                                if let (Ok(yes), Ok(no)) = (prices[0].parse::<f64>(), prices[1].parse::<f64>()) {
                                    yes_price = yes;
                                    no_price = no;
                                }
                            }
                        }
                    }

                    let best_ask = market["bestAsk"].as_f64();
                    let best_bid = market["bestBid"].as_f64();
                    let last_trade_price = market["lastTradePrice"].as_f64();
                    
                    let _volume = market["volume"]
                        .as_str()
                        .and_then(|v| v.parse::<f64>().ok())
                        .or_else(|| market["volumeNum"].as_f64())
                        .unwrap_or(0.0);

                    let mut token_ids = Vec::new();
                    if let Some(token_ids_str) = market["clobTokenIds"].as_str() {
                        if let Ok(ids) = serde_json::from_str::<Vec<String>>(token_ids_str) {
                            token_ids = ids;
                        }
                    }

                    let resolution_date = market["endDate"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));

                    let event = Event {
                        platform: "polymarket".to_string(),
                        event_id: event_id.clone(),
                        title: question,
                        description: market["description"].as_str().unwrap_or_default().to_string(),
                        resolution_date,
                        category: category.clone(),
                        tags: tags.clone(),
                        slug: market["slug"].as_str().map(|s| s.to_string()),
                        token_ids,
                        outcome_prices: Some((yes_price, no_price)),
                        best_ask,
                        best_bid,
                        last_trade_price,
                        vector_cache: None,
                        categories: Vec::new(),
                    };

                    events.push(event);
                }
            }
        }

        Ok(events)
    }


    pub async fn fetch_prices(&self, event: &Event) -> Result<MarketPrices> {
        // 先查缓存
        if let Some(cached) = self.price_cache.get(&event.event_id).await {
            return Ok(cached);
        }

        // 直接从 event 中获取价格数据
        let (yes_price, no_price) = match event.outcome_prices {
            Some((yes, no)) => (yes, no),
            None => {
                // 如果没有 outcomePrices，尝试用 bestAsk/bestBid 估算
                match (event.best_ask, event.best_bid, event.last_trade_price) {
                    (Some(ask), Some(bid), _) => {
                        // 用买卖价中间值
                        ((ask + bid) / 2.0, 1.0 - ((ask + bid) / 2.0))
                    }
                    (Some(ask), None, _) => (ask, 1.0 - ask),
                    (None, Some(bid), _) => (bid, 1.0 - bid),
                    (None, None, Some(last)) => (last, 1.0 - last),
                    _ => {
                        return Err(anyhow::anyhow!(
                            "No price data available for event {}",
                            event.event_id
                        ));
                    }
                }
            }
        };

        let liquidity = 0.0;

        let prices = MarketPrices::new(yes_price, no_price, liquidity)
            .with_asks(
                event.best_ask.unwrap_or(yes_price),
                event.best_bid.map(|b| 1.0 - b).unwrap_or(no_price),
                event.last_trade_price
            );

        self.price_cache.set(event.event_id.clone(), prices.clone()).await;
        Ok(prices)
    }
}

























impl Default for PolymarketClient {
    fn default() -> Self {
        Self::new()
    }
}

const KALSHI_DEFAULT_BASE: &str = "https://api.elections.kalshi.com/trade-api/v2";

#[derive(Clone)]
pub struct KalshiClient {
    http_client: Client,
    base_url: String,
    price_cache: Arc<PriceCache>,
}

impl KalshiClient {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            http_client,
            base_url: KALSHI_DEFAULT_BASE.to_string(),
            price_cache: Arc::new(PriceCache::new(60)),
        }
    }


    
    /// 获取系列下的所有市场（最终版本 - 修复所有权问题）
    async fn fetch_markets_by_series(&self, series_ticker: &str) -> Result<Vec<serde_json::Value>> {
        let path = "/markets";
        let url = format!("{}{}", self.base_url, path);
        
        let response = self
            .http_client
            .get(&url)
            .query(&[
                ("series_ticker", series_ticker),
                ("status", "open"),
                ("limit", "500"),   
            ])
            .send()
            .await
            .context(format!("Failed to fetch markets for series {}", series_ticker))?;
        
        let status = response.status();
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Kalshi API error: {} - {}", status, error_text));
        }
        
        let data: serde_json::Value = response.json().await?;
        
        // 使用字段进行筛选：只保留活跃且未结算的市场
        let markets = data["markets"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|m| {
                        let is_active = m["status"].as_str() == Some("active");
                        let is_settled = m["result"].as_str().unwrap_or("") != "";
                        is_active && !is_settled
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        
        Ok(markets)
    }








    /// 获取所有Kalshi事件（串行版本 - 稳定可靠）
        /// 获取所有Kalshi事件（串行版本 - 稳定可靠）
        /// 获取所有Kalshi事件（串行版本 - 稳定可靠）
    pub async fn fetch_events(&self) -> Result<Vec<Event>> {
        use chrono::FixedOffset;
        
        // 第一步：获取所有事件系列
        let path = "/events";
        let url = format!("{}{}", self.base_url, path);
        
        let response = self
            .http_client
            .get(&url)
            .query(&[
                ("status", "open"),
                ("limit", "200"),  //规模
            ])
            .send()
            .await
            .context("Failed to fetch Kalshi events")?;
        
        let status = response.status();
        
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Kalshi API error: {} - {}", status, error_text));
        }
        
        let data: serde_json::Value = response.json().await?;
        
        // 提取所有 series_ticker 及其对应的 category
        let mut series_info = Vec::new();  // (series_ticker, category)
        
        if let Some(events_array) = data["events"].as_array() {
            for event_data in events_array {
                if let Some(series) = event_data["series_ticker"].as_str() {
                    let category = event_data["category"].as_str().map(String::from);
                    series_info.push((series.to_string(), category));
                }
            }
        }
        
        // 第二步：串行为每个 series_ticker 获取市场
        let mut all_events = Vec::new();
        let mut market_count = 0;
        
        for (series, category) in series_info {
            let markets = match self.fetch_markets_by_series(&series).await {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("警告: 获取系列 {} 的市场失败: {}", series, e);
                    continue;
                }
            };
            
            for market in markets {
                market_count += 1;
                
                // 提取候选人名称（如果有）
                let candidate_name = market["yes_sub_title"]
                    .as_str()
                    .filter(|s: &&str| !s.is_empty())
                    .unwrap_or("");
                
                // 提取价格（美分转美元）
                let yes_ask_cents = market["yes_ask"].as_i64().unwrap_or(0);
                let yes_bid_cents = market["yes_bid"].as_i64().unwrap_or(0);
                let last_price_cents = market["last_price"].as_i64();
                
                // 构建标题：如果存在候选人，附加到标题后
                let title = if !candidate_name.is_empty() {
                    format!("{} - {}", 
                        market["title"].as_str().unwrap_or(""),
                        candidate_name
                    )
                } else {
                    market["title"].as_str().unwrap_or("").to_string()
                };
                
                // 获取市场的 ticker（具体市场的 ID）
                let market_ticker = market["ticker"].as_str().unwrap_or("").to_string();
                
                // 构建 Event - 使用市场的 ticker 作为 event_id
                let event = Event {
                    platform: "kalshi".to_string(),
                    event_id: market_ticker.clone(),  // 使用具体市场的 ticker
                    title: title.clone(),  // 克隆 title 供后续使用
                    description: market["subtitle"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    resolution_date: market["expiration_time"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt: DateTime<FixedOffset>| dt.with_timezone(&Utc)),
                    category: category.clone(),
                    tags: Vec::new(),
                    slug: Some(market_ticker.clone()),  // 克隆 market_ticker
                    token_ids: Vec::new(),
                    outcome_prices: None,
                    best_ask: Some(yes_ask_cents as f64 / 100.0),
                    best_bid: Some(yes_bid_cents as f64 / 100.0),
                    last_trade_price: last_price_cents.map(|v| v as f64 / 100.0),
                    vector_cache: None,
                    categories: Vec::new(),
                };
                
                if market_count <= 20 {
                    println!("🔍 [Kalshi事件] 序号={}, event_id={}, title={}", 
                        market_count, 
                        market_ticker,
                        title.chars().take(30).collect::<String>()
                    );
                }
                
                all_events.push(event);
            }
        }
        
        println!("   ✅ 获取到 {} 个Kalshi具体市场", all_events.len());
        Ok(all_events)
    }


















    /// 新增：获取事件下的所有市场（候选人）
    async fn fetch_markets_by_event(&self, event_ticker: &str) -> Result<Vec<serde_json::Value>> {
        println!("🔍 获取事件 {} 的市场", event_ticker);  // 添加调试
        
        let path = format!("/events/{}/markets", event_ticker);
        let url = format!("{}{}", self.base_url, path);
        println!("📡 请求 URL: {}", url);  // 添加调试
        
        let response = self
            .http_client
            .get(&url)
            .query(&[("limit", "200")])
            .send()
            .await
            .context(format!("Failed to fetch markets for event {}", event_ticker))?;

        let status = response.status();
        println!("📦 响应状态: {}", status);  // 添加调试

        if !status.is_success() {
            println!("❌ 错误响应: {}", status);  // 添加调试
            return Ok(Vec::new());
        }

        let text = response.text().await?;
        println!("📄 响应内容前200字符: {:?}", &text[..200.min(text.len())]);  // 添加调试
        
        let data: serde_json::Value = serde_json::from_str(&text)?;
        let markets = data["markets"].as_array().cloned().unwrap_or_default();
        println!("✅ 获取到 {} 个市场", markets.len());  // 添加调试
        
        Ok(markets)
    }

    pub async fn fetch_open_market_tickers(&self, series_ticker: &str) -> Result<Vec<String>> {
        let path = "/markets";
        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .query(&[
                ("series_ticker", series_ticker),
                ("status", "open"),
                ("limit", "200"),
            ])
            .send()
            .await
            .context("Failed to fetch Kalshi markets")?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Kalshi markets API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }
        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Kalshi markets response")?;
        let tickers: Vec<String> = data["markets"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["ticker"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(tickers)
    }

    pub async fn fetch_prices(&self, event_id: &str) -> Result<MarketPrices> {
        if let Some(cached) = self.price_cache.get(event_id).await {
            return Ok(cached);
        }

        let path = format!("/events/{}/markets", event_id);

        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .send()
            .await
            .context("Failed to fetch Kalshi prices")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Kalshi API error: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        let data: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse Kalshi price response")?;

        let mut yes_price = 0.0;
        let mut no_price = 0.0;
        let mut liquidity = 0.0;

        if let Some(markets) = data["markets"].as_array() {
            for market in markets {
                let subtitle = market["subtitle"].as_str().unwrap_or("");
                let last_price = market["last_price"]
                    .as_i64()
                    .unwrap_or(0) as f64
                    / 100.0;

                if subtitle == "Yes" {
                    yes_price = last_price;
                } else if subtitle == "No" {
                    no_price = last_price;
                }

                if let Some(vol) = market["volume"].as_f64() {
                    liquidity += vol;
                }
            }
        }

        let prices = MarketPrices::new(yes_price, no_price, liquidity);
        self.price_cache.set(event_id.to_string(), prices.clone()).await;
        Ok(prices)
    }

    pub async fn get_market(&self, ticker: &str) -> Result<Option<serde_json::Value>> {
        let path = format!("/markets/{}", ticker);
        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .send()
            .await
            .context("Failed to fetch Kalshi market")?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let data = response.json().await.context("Parse market response")?;
        Ok(Some(data))
    }














    pub async fn get_market_prices(&self, ticker: &str) -> Result<Option<MarketPrices>> {
        // ==== 调试5: 每次请求价格都输出ticker ====
        static PRICE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let count = PRICE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        println!("💰 [获取价格] #{}, ticker={}", count+1, ticker);
        // ==== 结束调试5 ====
        
        if let Some(cached) = self.price_cache.get(ticker).await {
            return Ok(Some(cached));
        }
        
        // 修改这里：使用 /markets/{ticker} 接口
        let path = format!("/markets/{}", ticker);
        let url = format!("{}{}", self.base_url, path);
        
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Kalshi market")?;
        
        if !response.status().is_success() {
            println!("      ⚠️ [价格获取失败] ticker={}, 状态码: {}", ticker, response.status());
            return Ok(None);
        }
        
        let data: serde_json::Value = response.json().await?;
        
        // ==== 新增：输出前3个市场的完整返回数据 ====
        if count < 3 {
            println!("      📊 [Kalshi完整返回] ticker={}", ticker);
            println!("      {}", serde_json::to_string_pretty(&data).unwrap());
        }
        // ==== 结束新增 ====
        
        // 单个市场接口返回的是 {"market": {...}} 格式
        if let Some(market) = data.get("market") {
            let yes_ask_dollars_str = market["yes_ask_dollars"].as_str().unwrap_or("0");
            let yes_bid_dollars_str = market["yes_bid_dollars"].as_str().unwrap_or("0");

            let yes_ask_cents = (yes_ask_dollars_str.parse::<f64>().unwrap_or(0.0) * 100.0) as i64;
            let yes_bid_cents = (yes_bid_dollars_str.parse::<f64>().unwrap_or(0.0) * 100.0) as i64;
            let last_price_cents = market["last_price"].as_i64();
            let volume = market["volume_24h"].as_f64().unwrap_or(0.0);
            
            // 只输出前10个成功获取的价格
            if count < 10 {
                println!("      ✅ [价格获取成功] ticker={}, yes_ask={}, yes_bid={}", 
                    ticker, yes_ask_cents, yes_bid_cents);
            }
            
            let yes_price = (yes_ask_cents as f64 + yes_bid_cents as f64) / 200.0;
            let no_price = 1.0 - yes_price;
            
            let prices = MarketPrices::new(yes_price, no_price, volume)
                .with_asks(
                    yes_ask_cents as f64 / 100.0,
                    1.0 - (yes_bid_cents as f64 / 100.0),
                    last_price_cents.map(|v| v as f64 / 100.0)
                );
            
            self.price_cache.set(ticker.to_string(), prices.clone()).await;
            return Ok(Some(prices));
        }
        
        println!("      ⚠️ [价格获取失败] ticker={}, 响应中没有market字段", ticker);
        Ok(None)
    }


















    pub async fn get_orderbook(&self, ticker: &str) -> Result<Option<serde_json::Value>> {
        let path = format!("/markets/{}/orderbook", ticker);
        let response = self
            .http_client
            .get(&format!("{}{}", self.base_url, path))
            .send()
            .await
            .context("Failed to fetch Kalshi orderbook")?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let data = response.json().await.context("Parse orderbook response")?;
        Ok(Some(data))
    }

    fn orderbook_to_best_ask(yes_bids: &[serde_json::Value], no_bids: &[serde_json::Value]) -> (f64, f64) {
        let best_yes_bid_cents = yes_bids
            .last()
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as f64;
        let best_no_bid_cents = no_bids
            .last()
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as f64;
        let yes_ask = (100.0 - best_no_bid_cents) / 100.0;
        let no_ask = (100.0 - best_yes_bid_cents) / 100.0;
        (yes_ask, no_ask)
    }

    
}

impl Default for KalshiClient {
    fn default() -> Self {
        Self::new()
    }
}

