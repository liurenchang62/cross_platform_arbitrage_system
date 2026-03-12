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
        self.fetch_events_from_gamma(tag_slug, 200).await
    }













    pub async fn fetch_events_from_gamma(
        &self,
        tag_slug: Option<&str>,
        limit: u32,
    ) -> Result<Vec<Event>> {
        let limit = limit.min(50);  //縮減規模測試 $
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
            // 在解析时打印
            // 在解析时打印

            // println!("[DEBUG] Polymarket event 所有字段: {:?}", 
            //     event_data.as_object().map(|obj| obj.keys().collect::<Vec<_>>()));
            
            
            let tags: Vec<String> = event_data["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|t| t["slug"].as_str().or_else(|| t["label"].as_str()))
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            //println!("[DEBUG] Polymarket tags: {:?}", tags);

            let category = if let Some(cat) = event_data["category"].as_str() {
                    Some(cat.to_string())
                } else {
                    None
                };

            // 获取该事件下的所有市场
            if let Some(markets) = event_data["markets"].as_array() {
                for market in markets {
                    // 从 market 中提取所有需要的数据
                    let event_id = market["id"].as_str().unwrap_or_default().to_string();
                    let question = market["question"].as_str().unwrap_or_default().to_string();
                    
                    // 解析 outcomePrices
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

                    // 获取其他价格字段
                    let best_ask = market["bestAsk"].as_f64();
                    let best_bid = market["bestBid"].as_f64();
                    let last_trade_price = market["lastTradePrice"].as_f64();
                    
                    // 流动性/成交量
                    // 流动性/成交量
                    let _volume = market["volume"]
                        .as_str()
                        .and_then(|v| v.parse::<f64>().ok())
                        .or_else(|| market["volumeNum"].as_f64())
                        .unwrap_or(0.0);

                    // 解析 token_ids
                    let mut token_ids = Vec::new();
                    if let Some(token_ids_str) = market["clobTokenIds"].as_str() {
                        if let Ok(ids) = serde_json::from_str::<Vec<String>>(token_ids_str) {
                            token_ids = ids;
                        }
                    }

                    // 解析到期日
                    let resolution_date = market["endDate"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc));

                    // 构建 Event
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
                        categories: Vec::new(),  // 新增字段
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

        let liquidity = 0.0; // 可以从 volume 字段获取，但需要转换

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
                ("limit", "200"),
            ])
            .send()
            .await
            .context(format!("Failed to fetch markets for series {}", series_ticker))?;
        
        let status = response.status();
        
        if !status.is_success() {
            // 先获取状态码，再获取错误文本
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Kalshi API error: {} - {}", status, error_text));
        }
        
        let data: serde_json::Value = response.json().await?;
        
        // 从响应中提取 markets 数组
        let markets = data["markets"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        
        Ok(markets)
    }





    // /// 获取所有Kalshi事件（最终版本 - 并发控制）
    // pub async fn fetch_events(&self) -> Result<Vec<Event>> {
    //     use futures::future::join_all;
    //     use chrono::FixedOffset;
    //     use std::sync::Arc;
    //     use tokio::sync::Semaphore;
        
    //     // 第一步：获取所有事件系列
    //     let path = "/events";
    //     let url = format!("{}{}", self.base_url, path);
        
    //     let response = self
    //         .http_client
    //         .get(&url)
    //         .query(&[
    //             ("status", "open"),
    //             ("limit", "200"),
    //         ])
    //         .send()
    //         .await
    //         .context("Failed to fetch Kalshi events")?;
        
    //     let status = response.status();
        
    //     if !status.is_success() {
    //         let error_text = response.text().await.unwrap_or_default();
    //         return Err(anyhow::anyhow!("Kalshi API error: {} - {}", status, error_text));
    //     }
        
    //     let data: serde_json::Value = response.json().await?;
        
    //     // 提取所有 series_ticker 及其对应的 category
    //     let mut series_info = Vec::new();  // (series_ticker, category)
        
    //     if let Some(events_array) = data["events"].as_array() {
    //         for event_data in events_array {
    //             if let Some(series) = event_data["series_ticker"].as_str() {
    //                 let category = event_data["category"].as_str().map(String::from);
    //                 series_info.push((series.to_string(), category));
    //             }
    //         }
    //     }
        
    //     // 第二步：为每个 series_ticker 并行获取市场（限制并发数）
    //     let semaphore = Arc::new(Semaphore::new(1)); // 最多5个并发
    //     let mut tasks = Vec::new();
        
    //     for (series, _) in &series_info {
    //         let semaphore = semaphore.clone();
    //         let series = series.clone();
    //         let client = self.clone(); // 需要 KalshiClient 实现 Clone
            
    //         let task = async move {
    //             let _permit = semaphore.acquire().await.unwrap(); // 获取并发许可
    //             client.fetch_markets_by_series(&series).await
    //         };
    //         tasks.push(task);
    //     }
        
    //     // 等待所有并行任务完成
    //     let results = join_all(tasks).await;
        
    //     // 处理所有结果
    //     let mut all_events = Vec::new();
        
    //     for ((series, category), result) in series_info.into_iter().zip(results) {
    //         let markets = match result {
    //             Ok(m) => m,
    //             Err(e) => {
    //                 eprintln!("警告: 获取系列 {} 的市场失败: {}", series, e);
    //                 continue;
    //             }
    //         };
            
    //         for market in markets {
    //             // 提取候选人名称（如果有）
    //             let candidate_name = market["yes_sub_title"]
    //                 .as_str()
    //                 .filter(|s: &&str| !s.is_empty())
    //                 .unwrap_or("");
                
    //             // 构建标题：如果存在候选人，附加到标题后
    //             let title = if !candidate_name.is_empty() {
    //                 format!("{} - {}", 
    //                     market["title"].as_str().unwrap_or(""),
    //                     candidate_name
    //                 )
    //             } else {
    //                 market["title"].as_str().unwrap_or("").to_string()
    //             };
                
    //             // 提取价格（美分转美元）
    //             let yes_ask_cents = market["yes_ask"].as_i64().unwrap_or(0);
    //             let yes_bid_cents = market["yes_bid"].as_i64().unwrap_or(0);
    //             let last_price_cents = market["last_price"].as_i64();
                
    //             let event = Event {
    //                 platform: "kalshi".to_string(),
    //                 event_id: market["ticker"]
    //                     .as_str()
    //                     .unwrap_or("")
    //                     .to_string(),
    //                 title,
    //                 description: market["subtitle"]
    //                     .as_str()
    //                     .unwrap_or("")
    //                     .to_string(),
    //                 resolution_date: market["expiration_time"]
    //                     .as_str()
    //                     .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    //                     .map(|dt: DateTime<FixedOffset>| dt.with_timezone(&Utc)),
    //                 category: category.clone(),
    //                 tags: Vec::new(),
    //                 slug: market["ticker"].as_str().map(String::from),
    //                 token_ids: Vec::new(),
    //                 outcome_prices: None,
    //                 best_ask: Some(yes_ask_cents as f64 / 100.0),
    //                 best_bid: Some(yes_bid_cents as f64 / 100.0),
    //                 last_trade_price: last_price_cents.map(|v| v as f64 / 100.0),
    //             };
                
    //             all_events.push(event);
    //         }
    //     }
        
    //     Ok(all_events)
    // }


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
                ("limit", "100"), //缩减
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
        
        for (series, category) in series_info {
            let markets = match self.fetch_markets_by_series(&series).await {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("警告: 获取系列 {} 的市场失败: {}", series, e);
                    continue;
                }
            };
            
            for market in markets {
                // 提取候选人名称（如果有）
                let candidate_name = market["yes_sub_title"]
                    .as_str()
                    .filter(|s: &&str| !s.is_empty())
                    .unwrap_or("");
                
                // 构建标题：如果存在候选人，附加到标题后
                let title = if !candidate_name.is_empty() {
                    format!("{} - {}", 
                        market["title"].as_str().unwrap_or(""),
                        candidate_name
                    )
                } else {
                    market["title"].as_str().unwrap_or("").to_string()
                };
                
                // 提取价格（美分转美元）
                let yes_ask_cents = market["yes_ask"].as_i64().unwrap_or(0);
                let yes_bid_cents = market["yes_bid"].as_i64().unwrap_or(0);
                let last_price_cents = market["last_price"].as_i64();
                
                let event = Event {
                    platform: "kalshi".to_string(),
                    event_id: market["ticker"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                    title,
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
                    slug: market["ticker"].as_str().map(String::from),
                    token_ids: Vec::new(),
                    outcome_prices: None,
                    best_ask: Some(yes_ask_cents as f64 / 100.0),
                    best_bid: Some(yes_bid_cents as f64 / 100.0),
                    last_trade_price: last_price_cents.map(|v| v as f64 / 100.0),
                    vector_cache: None,
                    categories: Vec::new(),  // 新增字段
                };
                
                all_events.push(event);
            }
        }
        
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














    /// 获取市场价格（最终版本 - 修复所有权问题）
    pub async fn get_market_prices(&self, ticker: &str) -> Result<Option<MarketPrices>> {
        if let Some(cached) = self.price_cache.get(ticker).await {
            return Ok(Some(cached));
        }
        
        let path = format!("/markets/{}", ticker);
        let url = format!("{}{}", self.base_url, path);
        
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Kalshi market")?;
        
        let status = response.status();
        
        if !status.is_success() {
            return Ok(None);
        }
        
        let data: serde_json::Value = response.json().await?;
        
        // 提取市场数据
        if let Some(market) = data.get("market") {
            let yes_ask = market["yes_ask"]
                .as_i64()
                .unwrap_or(0) as f64 / 100.0;
            let no_ask = market["no_ask"]
                .as_i64()
                .unwrap_or(0) as f64 / 100.0;
            let yes_bid = market["yes_bid"]
                .as_i64()
                .unwrap_or(0) as f64 / 100.0;
            let no_bid = market["no_bid"]
                .as_i64()
                .unwrap_or(0) as f64 / 100.0;
            let last_price = market["last_price"]
                .as_i64()
                .map(|v| v as f64 / 100.0);
            let liquidity = market["volume_24h"]
                .as_i64()
                .unwrap_or(0) as f64;
            
            // 对于二元市场，YES和NO的价格之和应该接近1
            // 使用买卖价的中间值作为当前价格
            let yes_price = (yes_ask + yes_bid) / 2.0;
            let no_price = (no_ask + no_bid) / 2.0;
            
            let prices = MarketPrices::new(yes_price, no_price, liquidity)
                .with_asks(yes_ask, no_ask, last_price);
            
            self.price_cache.set(ticker.to_string(), prices.clone()).await;
            Ok(Some(prices))
        } else {
            Ok(None)
        }
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

