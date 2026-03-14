use crate::event::MarketPrices;

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

pub struct ArbitrageDetector {
    min_profit_threshold: f64,
    fees: Fees,
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





    
}


/// 滑点计算结果
#[derive(Debug, Clone)]
pub struct SlippageInfo {
    pub avg_price: f64,        // 平均成交价
    pub slippage_percent: f64, // 滑点百分比
    pub filled: bool,           // 是否完全成交
    pub filled_amount: f64,     // 实际成交的USDT金额
    pub filled_contracts: f64,  // 实际成交的合约数
}

/// 根据固定USDT金额计算滑点
pub fn calculate_slippage_with_fixed_usdt(
    asks: &[(f64, f64)],  // (价格, 数量) 数组，已按价格升序
    usdt_amount: f64,      // 固定投入金额，比如 100 USDT
) -> SlippageInfo {
    let mut remaining_usdt = usdt_amount;
    let mut total_contracts = 0.0;
    let mut total_cost = 0.0;
    let best_price = if asks.is_empty() { 0.0 } else { asks[0].0 };
    
    for (price, size) in asks {
        // 这一档的总价值 = 价格 × 数量
        let level_value = price * size;
        
        if remaining_usdt >= level_value {
            // 可以吃掉整档
            total_contracts += size;
            total_cost += level_value;
            remaining_usdt -= level_value;
        } else {
            // 只能吃部分
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



/// 解析Polymarket订单簿，根据方向返回对应的asks
pub fn parse_polymarket_orderbook(data: &serde_json::Value, side: &str) -> Option<Vec<(f64, f64)>> {
    match side {
        "YES" => {
            // 买 YES：直接取 asks（YES 的卖单）
            let asks = data.get("asks")?.as_array()?;
            let mut result = Vec::new();
            
            for ask in asks {
                let price = ask.get("price")?.as_str()?.parse::<f64>().ok()?;
                let size = ask.get("size")?.as_str()?.parse::<f64>().ok()?;
                result.push((price, size));
            }
            
            // 确保按价格升序
            result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            Some(result)
        },
        "NO" => {
            // 买 NO：需要从 bids 转换（NO 卖价 = 1 - YES 买价）
            let bids = data.get("bids")?.as_array()?;
            let mut result = Vec::new();
            
            for bid in bids {
                let bid_price = bid.get("price")?.as_str()?.parse::<f64>().ok()?;
                let size = bid.get("size")?.as_str()?.parse::<f64>().ok()?;
                
                // NO 的卖价 = 1 - YES 的买价
                let ask_price = 1.0 - bid_price;
                // 只保留合理价格
                if ask_price > 0.01 && ask_price < 1.0 {
                    result.push((ask_price, size));
                }
            }
            
            // 按价格升序（从最便宜的 NO 卖单开始）
            result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            Some(result)
        },
        _ => None,
    }
}











/// 解析Kalshi订单簿，返回指定方向的 asks（卖单）
pub fn parse_kalshi_orderbook(
    data: &serde_json::Value, 
    side: &str  // "YES" 或 "NO"
) -> Option<Vec<(f64, f64)>> {
    let orderbook = data.get("orderbook_fp")?;
    
    match side {
        "YES" => {
            // 买 YES 需要用 YES 的卖单，但 Kalshi 只返回 bids
            // YES 的卖价 = 1 - NO 的买价
            let no_bids = orderbook.get("no_dollars")?.as_array()?;
            let mut result = Vec::new();
            
            for entry in no_bids {
                let price_str = entry.get(0)?.as_str()?;
                let size_str = entry.get(1)?.as_str()?;
                let bid_price = price_str.parse::<f64>().ok()?;
                let size = size_str.parse::<f64>().ok()?;
                
                // NO 的买单价格对应的 YES 卖价
                let ask_price = 1.0 - bid_price;
                // 只保留合理价格 (>0.01) 并确保价格为正
                if ask_price > 0.01 && ask_price < 1.0 {
                    result.push((ask_price, size));
                }
            }
            
            result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            Some(result)
        },
        "NO" => {
            // 买 NO 需要用 NO 的卖单，但 Kalshi 只返回 bids
            // NO 的卖价 = 1 - YES 的买价
            let yes_bids = orderbook.get("yes_dollars")?.as_array()?;
            let mut result = Vec::new();
            
            for entry in yes_bids {
                let price_str = entry.get(0)?.as_str()?;
                let size_str = entry.get(1)?.as_str()?;
                let bid_price = price_str.parse::<f64>().ok()?;
                let size = size_str.parse::<f64>().ok()?;
                
                // YES 的买单价格对应的 NO 卖价
                let ask_price = 1.0 - bid_price;
                // 只保留合理价格 (>0.01) 并确保价格为正
                if ask_price > 0.01 && ask_price < 1.0 {
                    result.push((ask_price, size));
                }
            }
            
            result.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
            Some(result)
        },
        _ => None,
    }
}