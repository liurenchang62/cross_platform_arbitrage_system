// src/arbitrage_detector.rs
use crate::market::MarketPrices;
use serde_json::Value;
use std::collections::HashMap;

/// Gas 费配置（固定值，单位 USDT）
pub const GAS_FEE: f64 = 0.02;  // 每笔交易 $0.02

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
    pub gas_fee: f64,
    pub final_profit: f64,
    pub final_roi_percent: f64,
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

    // 在 ArbitrageDetector 中添加新方法

    pub fn calculate_arbitrage_with_direction(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
        pm_side: &str,
        kalshi_side: &str,
        needs_inversion: bool,
    ) -> Option<ArbitrageOpportunity> {
        // 根据方向确定实际买卖
        let (pm_action, pm_price) = if pm_side == "YES" {
            ("BUY", pm_prices.yes_ask.unwrap_or(pm_prices.yes))
        } else {
            ("BUY", pm_prices.no_ask.unwrap_or(pm_prices.no))
        };
        
        let (kalshi_action, kalshi_price) = if kalshi_side == "YES" {
            ("BUY", kalshi_prices.yes_ask.unwrap_or(kalshi_prices.yes))
        } else {
            ("BUY", kalshi_prices.no_ask.unwrap_or(kalshi_prices.no))
        };
        
        // 计算成本
        let total_cost = pm_price + kalshi_price;
        let profit = 1.0 - total_cost;
        let total_fees = self.fees.polymarket + self.fees.kalshi;
        
        if profit <= total_fees + self.min_profit_threshold {
            return None;
        }
        
        let net_profit = profit - total_fees;
        let final_profit = net_profit - GAS_FEE;
        
        if final_profit <= self.min_profit_threshold {
            return None;
        }
        
        let roi = if total_cost > 0.0 {
            (final_profit / total_cost) * 100.0
        } else {
            0.0
        };
        
        // 构建策略描述
        let inversion_note = if needs_inversion {
            " [Y/N颠倒]"
        } else {
            ""
        };
        
        let strategy = format!("Buy {} on Polymarket + Buy {} on Kalshi{}", 
            pm_side, kalshi_side, inversion_note);
        
        Some(ArbitrageOpportunity {
            strategy,
            kalshi_action: (kalshi_action.to_string(), kalshi_side.to_string(), kalshi_price),
            polymarket_action: (pm_action.to_string(), pm_side.to_string(), pm_price),
            total_cost,
            gross_profit: profit,
            fees: total_fees,
            net_profit,
            roi_percent: roi,
            gas_fee: GAS_FEE,
            final_profit,
            final_roi_percent: roi,
        })
    }

    pub fn calculate_final_profit(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
        pm_slippage: f64,
        kalshi_slippage: f64,
    ) -> Option<ArbitrageOpportunity> {
        let opportunity = match self.check_arbitrage_optimal(pm_prices, kalshi_prices) {
            Some(opp) => opp,
            None => return None,
        };

        let pm_slipped = if opportunity.strategy.contains("Buy Yes on Polymarket") {
            pm_prices.yes * (1.0 + pm_slippage / 100.0)
        } else {
            pm_prices.no * (1.0 + pm_slippage / 100.0)
        };

        let kalshi_slipped = if opportunity.strategy.contains("Buy Yes on Kalshi") {
            kalshi_prices.yes * (1.0 + kalshi_slippage / 100.0)
        } else {
            kalshi_prices.no * (1.0 + kalshi_slippage / 100.0)
        };

        let slipped_cost = pm_slipped + kalshi_slipped;
        let slipped_profit = 1.0 - slipped_cost;

        if slipped_profit <= 0.0 {
            return None;
        }

        let total_fees = self.fees.polymarket + self.fees.kalshi;
        let net_profit = slipped_profit - total_fees;
        let final_profit = net_profit - GAS_FEE;

        if final_profit <= self.min_profit_threshold {
            return None;
        }

        let roi = if slipped_cost > 0.0 {
            (final_profit / slipped_cost) * 100.0
        } else {
            0.0
        };

        Some(ArbitrageOpportunity {
            strategy: opportunity.strategy,
            kalshi_action: opportunity.kalshi_action,
            polymarket_action: opportunity.polymarket_action,
            total_cost: slipped_cost,
            gross_profit: slipped_profit,
            fees: total_fees,
            net_profit,
            roi_percent: (net_profit / slipped_cost) * 100.0,
            gas_fee: GAS_FEE,
            final_profit,
            final_roi_percent: roi,
        })
    }

    pub fn check_arbitrage_optimal(
        &self,
        pm_prices: &MarketPrices,
        kalshi_prices: &MarketPrices,
    ) -> Option<ArbitrageOpportunity> {
        if kalshi_prices.yes == 0.0 && kalshi_prices.no == 0.0 {
            return None;
        }
        if !pm_prices.validate() || !kalshi_prices.validate() {
            return None;
        }

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

        let cost_strategy_1 = kalshi_yes_ask + pm_no_ask;
        let profit_strategy_1 = 1.0 - cost_strategy_1;

        let cost_strategy_2 = kalshi_no_ask + pm_yes_ask;
        let profit_strategy_2 = 1.0 - cost_strategy_2;

        let total_fees = self.fees.polymarket + self.fees.kalshi;

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
                gas_fee: 0.0,
                final_profit: 0.0,
                final_roi_percent: 0.0,
            });
        }

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
                gas_fee: 0.0,
                final_profit: 0.0,
                final_roi_percent: 0.0,
            });
        }

        None
    }
}

// ==================== 滑点计算相关函数（需要公开导出）====================

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