// src/main.rs
//! 跨平台套利监控系统主入口

use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time;

mod event;
mod event_matcher;
mod text_vectorizer;
mod vector_index;
mod arbitrage_detector;
mod monitor_logger;
mod clients;
mod category_mapper;
mod category_index_manager;
mod unclassified_logger;

use crate::category_mapper::CategoryMapper;
use crate::unclassified_logger::UnclassifiedLogger;
use crate::event::Event;
use clients::{PolymarketClient, KalshiClient};
use event_matcher::{EventMatcher, EventMatcherConfig};
use arbitrage_detector::ArbitrageDetector;
use monitor_logger::MonitorLogger;
use crate::arbitrage_detector::{
    parse_polymarket_orderbook, 
    parse_kalshi_orderbook, 
    calculate_slippage_with_fixed_usdt
};

#[tokio::main]
async fn main() -> Result<()> {
    println!("🚀 启动跨平台套利监控系统");
    println!("📊 监控平台: Polymarket ↔ Kalshi");
    
    // 初始化日志
    let logger = MonitorLogger::new("logs".to_string())?;
    
    // 初始化未分类日志器
    let unclassified_logger = UnclassifiedLogger::new("logs/unclassified")?;
    
    // 初始化类别映射器
    println!("📚 加载类别配置...");
    let category_mapper = CategoryMapper::from_file("config/categories.toml")
        .context("加载类别配置文件失败")?;
    
    // 初始化客户端
    let polymarket = PolymarketClient::new();
    let kalshi = KalshiClient::new();
    
    // 初始化匹配器
    let matcher_config = EventMatcherConfig {
        similarity_threshold: 0.8,
        use_date_boost: true,
        use_category_boost: true,
        date_boost_factor: 0.05,
        category_boost_factor: 0.03,
        ..Default::default()
    };
    
    let mut matcher = EventMatcher::new(matcher_config, category_mapper)
        .with_logger(unclassified_logger);
    
    // 初始化套利检测器
    let arb_detector = ArbitrageDetector::new(0.02);
    
    println!("📡 首次获取事件并构建双索引...");
    
    // 首次获取事件
    let (kalshi_events, pm_events) = match fetch_initial_events(&polymarket, &kalshi).await {
        Ok(events) => events,
        Err(e) => {
            eprintln!("❌ 首次获取事件失败: {}", e);
            return Err(e);
        }
    };
    
    // 先用所有事件训练统一的向量化器
    println!("📚 训练统一向量化器...");
    let all_events: Vec<Event> = kalshi_events.iter().chain(pm_events.iter()).cloned().collect();
    matcher.fit_vectorizer(&all_events);

    // 构建双索引
    println!("🌲 构建 Kalshi 事件索引...");
    matcher.build_kalshi_index(&kalshi_events)?;
    
    println!("🌲 构建 Polymarket 事件索引...");
    matcher.build_polymarket_index(&pm_events)?;
    
    println!("\n✅ 初始化完成");
    println!("   📊 Kalshi 事件数: {}", kalshi_events.len());
    println!("   📊 Polymarket 事件数: {}", pm_events.len());
    println!("   📚 词汇表大小: {}", matcher.vectorizer().vocab_size());
    println!("   📊 Kalshi 索引大小: {}", matcher.kalshi_index_size());
    println!("   📊 Polymarket 索引大小: {}", matcher.polymarket_index_size());
    println!("\n🔄 开始监控循环 (间隔: 30秒)...\n");
    
    // 主循环
    let mut cycle_count = 0;
    loop {
        cycle_count += 1;
        
        match run_cycle(
            &polymarket,
            &kalshi,
            &mut matcher,
            &arb_detector,
            &logger,
            cycle_count,
        ).await {
            Ok(stats) => {
                println!("📊 周期统计: 匹配 {} 对, 套利 {} 个", 
                    stats.matches, stats.opportunities);
            }
            Err(e) => {
                eprintln!("❌ 周期错误: {}", e);
            }
        }
        
        println!("⏳ 等待下一周期...\n");
        time::sleep(Duration::from_secs(30)).await;
    }
}

/// 首次获取事件
async fn fetch_initial_events(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
) -> Result<(Vec<Event>, Vec<Event>)> {
    println!("   📡 获取 Polymarket 事件...");
    let pm_events = polymarket.fetch_events().await?;
    println!("      ✅ 获取到 {} 个事件", pm_events.len());
    
    println!("   📡 获取 Kalshi 事件...");
    let kalshi_events = kalshi.fetch_events().await?;
    println!("      ✅ 获取到 {} 个事件", kalshi_events.len());
    
    Ok((kalshi_events, pm_events))
}

async fn run_cycle(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    matcher: &EventMatcher,
    arb_detector: &ArbitrageDetector,
    logger: &MonitorLogger,
    cycle_count: i32,
) -> Result<CycleStats> {
    let start_time = chrono::Local::now();
    println!("🔄 开始新周期 #{} - {}", cycle_count, start_time.format("%H:%M:%S"));
    
    // 获取最新事件
    println!("   📡 获取最新事件...");
    let pm_events = polymarket.fetch_events().await?;
    let kalshi_events = kalshi.fetch_events().await?;
    
    println!("      Polymarket: {} 个, Kalshi: {} 个", 
        pm_events.len(), kalshi_events.len());
    
    // 匹配事件
    println!("   🔍 匹配事件...");
    let matches = matcher.find_matches_bidirectional(&pm_events, &kalshi_events);
    println!("      ✅ 找到 {} 个潜在匹配对", matches.len());
    
    println!("\n📊 ====== 套利机会深度验证 ======");
    
    let mut opportunity_count = 0;
    let mut verified_count = 0;
    let trade_amount = 100.0; // 固定交易金额 100 USDT
    
    for (pm_event, kalshi_event, similarity) in &matches {
        // 获取价格
        let pm_prices = match polymarket.fetch_prices(pm_event).await {
            Ok(p) => p,
            Err(_) => continue,
        };
        
        let kalshi_prices = match kalshi.get_market_prices(&kalshi_event.event_id).await {
            Ok(Some(p)) => p,
            _ => continue,
        };
        
        // 先用最优价检查潜在机会
        if let Some(opportunity) = arb_detector.check_arbitrage_optimal(&pm_prices, &kalshi_prices) {
            opportunity_count += 1;
            
            // 确定策略对应的买卖方向
            let (pm_side, kalshi_side) = if opportunity.strategy.contains("Buy Yes on Kalshi") {
                ("NO", "YES")  // 买 Polymarket NO + 买 Kalshi YES
            } else {
                ("YES", "NO")  // 买 Polymarket YES + 买 Kalshi NO
            };
            
            println!("\n  📌 潜在机会 #{} (相似度: {:.2})", opportunity_count, similarity);
            println!("     PM: {}", pm_event.title);
            println!("     Kalshi: {}", kalshi_event.title);
            println!("     策略: {}", opportunity.strategy);
            println!();
            println!("     📊 最优价格:");
            println!("        Polymarket {}: {:.3}", pm_side, 
                if pm_side == "YES" { pm_prices.yes_ask.unwrap_or(pm_prices.yes) } 
                else { pm_prices.no_ask.unwrap_or(pm_prices.no) });
            println!("        Kalshi {}: {:.3}", kalshi_side,
                if kalshi_side == "YES" { kalshi_prices.yes_ask.unwrap_or(kalshi_prices.yes) }
                else { kalshi_prices.no_ask.unwrap_or(kalshi_prices.no) });
            println!("     💰 理想利润: {:.3} | 理想成本: {:.3} | ROI: {:.1}%", 
                opportunity.net_profit, opportunity.total_cost, opportunity.roi_percent);
            println!();
            println!("     🔍 验证深度 (投入 {} USDT)...", trade_amount);
            
            // 获取 Polymarket 订单簿
            let pm_orderbook = if let Some(token_id) = pm_event.token_ids.first() {
                match polymarket.get_order_book(token_id).await {
                    Ok(Some(ob)) => parse_polymarket_orderbook(&ob, pm_side),
                    _ => None,
                }
            } else {
                None
            };
            
            // 获取 Kalshi 订单簿
            let kalshi_orderbook = match kalshi.get_order_book(&kalshi_event.event_id).await {
                Ok(Some(ob)) => parse_kalshi_orderbook(&ob, kalshi_side),
                _ => None,
            };
            
            // 计算 Polymarket 滑点
            let (pm_slip, pm_avg) = if let Some(ob) = pm_orderbook {
                let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
                (info.slippage_percent, info.avg_price)
            } else {
                let price = if pm_side == "YES" { pm_prices.yes_ask.unwrap_or(pm_prices.yes) }
                            else { pm_prices.no_ask.unwrap_or(pm_prices.no) };
                (0.0, price)
            };
            
            // 计算 Kalshi 滑点
            let (kalshi_slip, kalshi_avg) = if let Some(ob) = kalshi_orderbook {
                let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
                (info.slippage_percent, info.avg_price)
            } else {
                let price = if kalshi_side == "YES" { kalshi_prices.yes_ask.unwrap_or(kalshi_prices.yes) }
                            else { kalshi_prices.no_ask.unwrap_or(kalshi_prices.no) };
                (0.0, price)
            };
            
            let optimal_pm_price = if pm_side == "YES" { pm_prices.yes_ask.unwrap_or(pm_prices.yes) }
                                   else { pm_prices.no_ask.unwrap_or(pm_prices.no) };
            let optimal_kalshi_price = if kalshi_side == "YES" { kalshi_prices.yes_ask.unwrap_or(kalshi_prices.yes) }
                                       else { kalshi_prices.no_ask.unwrap_or(kalshi_prices.no) };
            
            println!();
            println!("     📊 滑点分析:");
            println!("        Polymarket {}: 最优价 {:.3} -> 考虑滑点平均价 {:.3} ({:+.2}%)", 
                pm_side, optimal_pm_price, pm_avg, pm_slip);
            println!("        Kalshi {}: 最优价 {:.3} -> 考虑滑点平均价 {:.3} ({:+.2}%)", 
                kalshi_side, optimal_kalshi_price, kalshi_avg, kalshi_slip);
            
            // 用考虑了滑点的价格重新计算套利机会
            let mut pm_adjusted = pm_prices.clone();
            let mut kalshi_adjusted = kalshi_prices.clone();
            
            if pm_side == "YES" {
                pm_adjusted.yes = pm_avg;
            } else {
                pm_adjusted.no = pm_avg;
            }
            
            if kalshi_side == "YES" {
                kalshi_adjusted.yes = kalshi_avg;
            } else {
                kalshi_adjusted.no = kalshi_avg;
            }
            
            if let Some(verified) = arb_detector.check_arbitrage_optimal(&pm_adjusted, &kalshi_adjusted) {
                verified_count += 1;
                println!();
                println!("     ✅ 考虑滑点后仍然有机会!");
                println!("        💰 实际利润: ${:.3} | 实际成本: ${:.3} | ROI: {:.1}%", 
                    verified.net_profit, verified.total_cost, verified.roi_percent);
                
                // 记录套利机会
                if let Err(e) = logger.log_opportunity(&verified) {
                    eprintln!("         ⚠️ 记录日志失败: {}", e);
                }
            } else {
                println!();
                println!("     ❌ 考虑滑点后无套利机会 ");
            }
            println!("     ------------------------------------");
        }
    }
    
    println!("");
    println!("====== 周期统计 ======");
    println!("   潜在机会: {} 个 ", opportunity_count);
    println!("   验证通过: {} 个 ", verified_count);
    println!("   验证失败: {} 个 ", opportunity_count - verified_count);
    
    let elapsed = chrono::Local::now() - start_time;
    let duration_ms = elapsed.num_milliseconds();
    println!("   周期完成, 耗时: {} ms", duration_ms);
    
    Ok(CycleStats {
        matches: matches.len(),
        opportunities: verified_count,
    })
}

/// 周期统计
struct CycleStats {
    matches: usize,
    opportunities: usize,
}