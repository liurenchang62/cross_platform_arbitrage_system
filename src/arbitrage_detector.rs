// src/arbitrage_detector.rs
use crate::market::MarketPrices;
use serde_json::Value;


#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub strategy: String,
    pub kalshi_action: (String, String, f64),
    pub polymarket_action: (String, String, f64),
    pub total_cost: f64,
    pub gross_profit: f64,
    pub fees: f64,
    pub net_profit: f64,
    pub roi_percent: f64,
}

#[derive(Debug, Clone)]
pub struct Fees {
    pub polymarket: f64,
    pub kalshi: f64,
}

impl Default for Fees {
    fn default() -> Self {
        Self {
            polymarket: 0.01,
            kalshi: 0.01,
        }
    }
}

pub struct ArbitrageDetector {
    min_profit_threshold: f64,
    fees: Fees,
}

impl ArbitrageDetector {
    pub fn new(min_profit_threshold: f64) -> Self {
        Self {
            min_profit_threshold,
            fees: Fees::default(),
        }
    }

    pub fn with_fees(mut self, fees: Fees) -> Self {
        self.fees = fees;
        self
    }

    pub fn check_arbitrage_optimal(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
    ) -> Option<ArbitrageOpportunity> {
        // 验证价格有效性
        if kalshi_prices.yes == 0.0 && kalshi_prices.no == 0.0 {
            return None;
        }
        if !pm_prices.validate() || !kalshi_prices.validate() {
            return None;
        }

        // 确保有必要的 ask 数据
        if pm_prices.yes_ask.is_none() || pm_prices.no_ask.is_none() {
            return None;
        }
        if kalshi_prices.yes_ask.is_none() || kalshi_prices.no_ask.is_none() {
            return None;
        }

        let pm_yes_ask = pm_prices.yes_ask.unwrap();
        let pm_no_ask = pm_prices.no_ask.unwrap();
        let kalshi_yes_ask = kalshi_prices.yes_ask.unwrap();
        let kalshi_no_ask = kalshi_prices.no_ask.unwrap();

        // 策略1: Buy Yes on Kalshi + Buy No on Polymarket
        let cost_strategy_1 = kalshi_yes_ask + pm_no_ask;
        let profit_strategy_1 = 1.0 - cost_strategy_1;

        // 策略2: Buy No on Kalshi + Buy Yes on Polymarket
        let cost_strategy_2 = kalshi_no_ask + pm_yes_ask;
        let profit_strategy_2 = 1.0 - cost_strategy_2;

        let total_fees = self.fees.polymarket + self.fees.kalshi;

        // 检查策略1
        if profit_strategy_1 > total_fees + self.min_profit_threshold {
            let net_profit = profit_strategy_1 - total_fees;
            let roi = if cost_strategy_1 > 0.0 {
                (net_profit / cost_strategy_1) * 100.0
            } else {
                0.0
            };

            return Some(ArbitrageOpportunity {
                strategy: "Buy Yes on Kalshi + Buy No on Polymarket".to_string(),
                kalshi_action: ("BUY".to_string(), "YES".to_string(), kalshi_yes_ask),
                polymarket_action: ("BUY".to_string(), "NO".to_string(), pm_no_ask),
                total_cost: cost_strategy_1,
                gross_profit: profit_strategy_1,
                fees: total_fees,
                net_profit,
                roi_percent: roi,
            });
        }

        // 检查策略2
        if profit_strategy_2 > total_fees + self.min_profit_threshold {
            let net_profit = profit_strategy_2 - total_fees;
            let roi = if cost_strategy_2 > 0.0 {
                (net_profit / cost_strategy_2) * 100.0
            } else {
                0.0
            };

            return Some(ArbitrageOpportunity {
                strategy: "Buy No on Kalshi + Buy Yes on Polymarket".to_string(),
                kalshi_action: ("BUY".to_string(), "NO".to_string(), kalshi_no_ask),
                polymarket_action: ("BUY".to_string(), "YES".to_string(), pm_yes_ask),
                total_cost: cost_strategy_2,
                gross_profit: profit_strategy_2,
                fees: total_fees,
                net_profit,
                roi_percent: roi,
            });
        }

        None
    }

    // 兼容旧调用
    pub fn check_arbitrage(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
    ) -> Option<ArbitrageOpportunity> {
        self.check_arbitrage_optimal(pm_prices, kalshi_prices)
    }
}

// 滑点计算相关函数
#[derive(Debug, Clone)]
pub struct SlippageInfo {
    pub avg_price: f64,
    pub slippage_percent: f64,
    pub filled: bool,
    pub filled_amount: f64,
    pub filled_contracts: f64,
}

pub fn calculate_slippage_with_fixed_usdt(
    asks: &[(f64, f64)],
    usdt_amount: f64,
) -> SlippageInfo {
    let mut remaining_usdt = usdt_amount;
    let mut total_contracts = 0.0;
    let mut total_cost = 0.0;
    let best_price = if asks.is_empty() { 0.0 } else { asks[0].0 };
    
    for (price, size) in asks {
        let level_value = price * size;
        
        if remaining_usdt >= level_value {
            total_contracts += size;
            total_cost += level_value;
            remaining_usdt -= level_value;
        } else {
            let buy_size = remaining_usdt / price;
            total_contracts += buy_size;
            total_cost += remaining_usdt;
            remaining_usdt = 0.0;
            break;
        }
    }
    
    let filled_amount = usdt_amount - remaining_usdt;
    let filled = remaining_usdt == 0.0;
    
    if total_contracts == 0.0 {
        return SlippageInfo {
            avg_price: 0.0,
            slippage_percent: 0.0,
            filled,
            filled_amount,
            filled_contracts: 0.0,
        };
    }
    
    let avg_price = total_cost / total_contracts;
    let slippage_percent = if best_price > 0.0 {
        (avg_price - best_price) / best_price * 100.0
    } else {
        0.0
    };
    
    SlippageInfo {
        avg_price,
        slippage_percent,
        filled,
        filled_amount,
        filled_contracts: total_contracts,
    }
}

pub fn parse_polymarket_orderbook(data: &Value, side: &str) -> Option<Vec<(f64, f64)>> {
    if side == "YES" {
        let asks = data.get("asks")?.as_array()?;
        let mut result = Vec::new();
        for ask in asks {
            let price = ask.get("price")?.as_str()?.parse::<f64>().ok()?;
            let size = ask.get("size")?.as_str()?.parse::<f64>().ok()?;
            result.push((price, size));
        }
        result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Some(result)
    } else if side == "NO" {
        let bids = data.get("bids")?.as_array()?;
        let mut result = Vec::new();
        for bid in bids {
            let bid_price = bid.get("price")?.as_str()?.parse::<f64>().ok()?;
            let size = bid.get("size")?.as_str()?.parse::<f64>().ok()?;
            let ask_price = 1.0 - bid_price;
            if ask_price > 0.01 && ask_price < 1.0 {
                result.push((ask_price, size));
            }
        }
        result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Some(result)
    } else {
        None
    }
}

pub fn parse_kalshi_orderbook(data: &Value, side: &str) -> Option<Vec<(f64, f64)>> {
    let orderbook = data.get("orderbook_fp")?;
    
    if side == "YES" {
        let no_bids = orderbook.get("no_dollars")?.as_array()?;
        let mut result = Vec::new();
        for entry in no_bids {
            let bid_price = entry.get(0)?.as_str()?.parse::<f64>().ok()?;
            let size = entry.get(1)?.as_str()?.parse::<f64>().ok()?;
            let ask_price = 1.0 - bid_price;
            if ask_price > 0.01 && ask_price < 1.0 {
                result.push((ask_price, size));
            }
        }
        result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Some(result)
    } else if side == "NO" {
        let yes_bids = orderbook.get("yes_dollars")?.as_array()?;
        let mut result = Vec::new();
        for entry in yes_bids {
            let bid_price = entry.get(0)?.as_str()?.parse::<f64>().ok()?;
            let size = entry.get(1)?.as_str()?.parse::<f64>().ok()?;
            let ask_price = 1.0 - bid_price;
            if ask_price > 0.01 && ask_price < 1.0 {
                result.push((ask_price, size));
            }
        }
        result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        Some(result)
    } else {
        None
    }
}