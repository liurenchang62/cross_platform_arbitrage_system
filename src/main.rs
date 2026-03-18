// src/main.rs
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::time;
use chrono::{Utc, Local};

mod market;
mod market_matcher;
mod text_vectorizer;
mod vector_index;
mod arbitrage_detector;
mod monitor_logger;
mod clients;
mod category_mapper;
// 在文件开头的 mod 声明中添加
mod validation;
mod unclassified_logger;
mod tracking;
mod query_params;
mod category_vectorizer;  // 新增

use crate::category_mapper::CategoryMapper;
use crate::unclassified_logger::UnclassifiedLogger;
use crate::market::Market;
use clients::{PolymarketClient, KalshiClient};
use market_matcher::{MarketMatcher, MarketMatcherConfig};
use arbitrage_detector::{ArbitrageDetector, ArbitrageOpportunity};
use monitor_logger::MonitorLogger;
use crate::tracking::{MonitorState};
use crate::query_params::{FULL_FETCH_INTERVAL, SIMILARITY_THRESHOLD};
use crate::arbitrage_detector::{
    calculate_slippage_with_fixed_usdt,
    parse_polymarket_orderbook,
    parse_kalshi_orderbook
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
    let matcher_config = MarketMatcherConfig {
        similarity_threshold: SIMILARITY_THRESHOLD,
        use_date_boost: true,
        use_category_boost: true,
        date_boost_factor: 0.05,
        category_boost_factor: 0.03,
        ..Default::default()
    };
    
    let mut matcher = MarketMatcher::new(matcher_config, category_mapper)
        .with_logger(unclassified_logger);
    
    // 初始化套利检测器
    let arb_detector = ArbitrageDetector::new(0.02);
    
    // 初始化监控状态
    let mut monitor_state = MonitorState::new(FULL_FETCH_INTERVAL, 10000);  // 改为10000
    
    println!("📡 首次获取市场并构建索引...");
    
    // 首次获取全量市场
    let (kalshi_markets, polymarket_markets) = match fetch_initial_markets(&polymarket, &kalshi).await {
        Ok(markets) => markets,
        Err(e) => {
            eprintln!("❌ 首次获取市场失败: {}", e);
            return Err(e);
        }
    };

    

    // 查找所有包含这两个队名但没有明确盘口的市场
    let pm_simple: Vec<_> = polymarket_markets.iter()
        .filter(|m| m.title.contains("Houston") && m.title.contains("Pittsburgh"))
        .filter(|m| !m.title.contains("O/U") && !m.title.contains("Winner") && !m.title.contains("Spread"))
        .collect();

    println!("\n疑似纯事件的市场 ({} 个):", pm_simple.len());
    for m in pm_simple.iter().take(5) {
        println!("  {}", m.title);
    }

    // 查找这些事件对应的子市场
    let pm_with_bets: Vec<_> = polymarket_markets.iter()
        .filter(|m| m.title.contains("Houston") && m.title.contains("Pittsburgh"))
        .filter(|m| m.title.contains("O/U") || m.title.contains("Winner") || m.title.contains("Spread"))
        .collect();

    println!("\n对应的二元市场 ({} 个):", pm_with_bets.len());
    for m in pm_with_bets.iter().take(5) {
        println!("  {}", m.title);
    }
    
    // 按类别训练向量化器
    matcher.fit_vectorizer(&kalshi_markets, &polymarket_markets)?;
    
    // 构建双索引
    println!("🌲 构建 Kalshi 市场索引...");
    matcher.build_kalshi_index(&kalshi_markets)?;
    
    println!("🌲 构建 Polymarket 市场索引...");
    matcher.build_polymarket_index(&polymarket_markets)?;
    
    println!("\n✅ 初始化完成");
    println!("   📊 Kalshi 市场数: {}", kalshi_markets.len());
    println!("   📊 Polymarket 市场数: {}", polymarket_markets.len());
    println!("   📊 Kalshi 索引大小: {}", matcher.kalshi_index_size());
    println!("   📊 Polymarket 索引大小: {}", matcher.polymarket_index_size());
    println!("\n🔄 开始监控循环 (间隔: 30秒)...\n");
    
    // 主循环
    loop {
        match run_cycle(
            &polymarket,
            &kalshi,
            &mut matcher,
            &arb_detector,
            &logger,
            &mut monitor_state,
        ).await {
            Ok(stats) => {
                println!("📊 周期统计: 新匹配 {} 对, 套利 {} 个, 追踪 {} 对", 
                    stats.new_matches, stats.arbitrage_opportunities, monitor_state.tracked_pairs.len());
            }
            Err(e) => {
                eprintln!("❌ 周期错误: {}", e);
            }
        }
        
        monitor_state.next_cycle();
        println!("⏳ 等待下一周期...\n");
        time::sleep(Duration::from_secs(30)).await;
    }
}

/// 验证单个套利对（带滑点检查）
async fn validate_arbitrage_pair(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    arb_detector: &ArbitrageDetector,
    pm_market: &Market,
    kalshi_market: &Market,
    similarity: f64,
    trade_amount: f64,
    _logger: &MonitorLogger,
) -> Option<ArbitrageOpportunity> {
    // 获取价格
    let pm_prices = match polymarket.fetch_prices(pm_market).await {
        Ok(p) => p,
        Err(_) => return None,
    };
    
    let kalshi_prices = match kalshi.get_market_prices(&kalshi_market.market_id).await {
        Ok(Some(p)) => p,
        _ => return None,
    };
    
    // 先用最优价检查潜在机会
    let opportunity = match arb_detector.check_arbitrage_optimal(&pm_prices, &kalshi_prices) {
        Some(opp) => opp,
        None => return None,
    };
    
    // 确定策略对应的买卖方向
    let (pm_side, kalshi_side) = if opportunity.strategy.contains("Buy Yes on Kalshi") {
        ("NO", "YES")
    } else {
        ("YES", "NO")
    };
    
    // 获取 Polymarket 订单簿
    let pm_orderbook = if let Some(token_id) = pm_market.token_ids.first() {
        match polymarket.get_order_book(token_id).await {
            Ok(Some(ob)) => parse_polymarket_orderbook(&ob, pm_side),
            _ => None,
        }
    } else {
        None
    };
    
    // 获取 Kalshi 订单簿
    let kalshi_orderbook = match kalshi.get_order_book(&kalshi_market.market_id).await {
        Ok(Some(ob)) => parse_kalshi_orderbook(&ob, kalshi_side),
        _ => None,
    };
    
    // 计算 Polymarket 滑点
    let pm_optimal = if pm_side == "YES" { pm_prices.yes_ask.unwrap_or(pm_prices.yes) }
                    else { pm_prices.no_ask.unwrap_or(pm_prices.no) };
    
    let (pm_avg, pm_slip) = if let Some(ob) = pm_orderbook {
        let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
        (info.avg_price, info.slippage_percent)
    } else {
        (pm_optimal, 0.0)
    };
    
    // 计算 Kalshi 滑点
    let kalshi_optimal = if kalshi_side == "YES" { kalshi_prices.yes_ask.unwrap_or(kalshi_prices.yes) }
                        else { kalshi_prices.no_ask.unwrap_or(kalshi_prices.no) };
    
    let (kalshi_avg, kalshi_slip) = if let Some(ob) = kalshi_orderbook {
        let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
        (info.avg_price, info.slippage_percent)
    } else {
        (kalshi_optimal, 0.0)
    };
    
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
    
    let verified = arb_detector.check_arbitrage_optimal(&pm_adjusted, &kalshi_adjusted)?;
    
    // 输出验证结果
    println!("\n  📌 验证通过 (相似度: {:.3})", similarity);
    println!("     PM: {}", pm_market.title);
    println!("     Kalshi: {}", kalshi_market.title);
    println!("     📊 滑点分析:");
    println!("        Polymarket {}: 最优价 {:.3} → 考虑滑点平均价 {:.3} ({:+.2}%)", 
        pm_side, pm_optimal, pm_avg, pm_slip);
    println!("        Kalshi {}: 最优价 {:.3} → 考虑滑点平均价 {:.3} ({:+.2}%)", 
        kalshi_side, kalshi_optimal, kalshi_avg, kalshi_slip);
    println!("     💰 策略: {}", verified.strategy);
    println!("     💵 净利润: ${:.3}", verified.net_profit);
    println!("     📊 ROI: {:.1}%", verified.roi_percent);
    println!("     ------------------------------------");
    
    Some(verified)
}










/// 执行全量匹配周期
async fn run_full_match_cycle(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    matcher: &mut MarketMatcher,
    arb_detector: &ArbitrageDetector,
    logger: &MonitorLogger,
    monitor_state: &mut MonitorState,
    trade_amount: f64,
) -> Result<(usize, usize)> {
    println!("   📡 执行全量匹配...");
    
    // 获取全量市场
    let polymarket_markets: Vec<Market> = polymarket.fetch_all_markets().await?;
    let kalshi_markets: Vec<Market> = kalshi.fetch_all_markets().await?;
    
    println!("      Polymarket: {} 个市场, Kalshi: {} 个市场", 
        polymarket_markets.len(), kalshi_markets.len());
    
    // ==== 调试1：纽卡和巴萨相关市场 ====
    println!("\n🔍 [调试1] 纽卡 vs 巴萨相关市场:");
    
    let pm_newcastle: Vec<_> = polymarket_markets.iter()
        .filter(|m| m.title.to_lowercase().contains("newcastle") &&
                     m.title.to_lowercase().contains("barcelona"))
        .collect();
    println!("  Polymarket 相关市场: {} 个", pm_newcastle.len());
    for (i, market) in pm_newcastle.iter().take(10).enumerate() {
        println!("    {}. {}", i+1, market.title);
    }
    
    let kalshi_newcastle: Vec<_> = kalshi_markets.iter()
        .filter(|m| m.title.to_lowercase().contains("newcastle") &&
                     m.title.to_lowercase().contains("barcelona"))
        .collect();
    println!("  Kalshi 相关市场: {} 个", kalshi_newcastle.len());
    for (i, market) in kalshi_newcastle.iter().take(10).enumerate() {
        println!("    {}. {}", i+1, market.title);
    }
    
    // ==== 调试：查看 Houston vs Pittsburgh 比赛的所有相关市场 ====
    println!("\n🔍 [调试] Houston vs Pittsburgh 相关市场:");

    // 查找同时包含 Houston 和 Pittsburgh 的市场
    let pm_houston_pitt: Vec<_> = polymarket_markets.iter()
        .filter(|m| m.title.contains("Houston") && m.title.contains("Pittsburgh"))
        .collect();
    println!("\n  Polymarket 同时包含 Houston 和 Pittsburgh 的市场 (共 {} 个):", pm_houston_pitt.len());
    for (i, market) in pm_houston_pitt.iter().enumerate() {
        println!("    {}. {}", i+1, market.title);
        if let Some(token_id) = market.token_ids.first() {
            println!("       token_id: {}", token_id);
        }
    }

    let kalshi_houston_pitt: Vec<_> = kalshi_markets.iter()
        .filter(|m| m.title.contains("Houston") && m.title.contains("Pittsburgh"))
        .collect();
    println!("\n  Kalshi 同时包含 Houston 和 Pittsburgh 的市场 (共 {} 个):", kalshi_houston_pitt.len());
    for (i, market) in kalshi_houston_pitt.iter().enumerate() {
        println!("    {}. {}", i+1, market.title);
    }
    
    
    // 重建索引
    println!("\n   🔄 重建索引...");
    matcher.build_kalshi_index(&kalshi_markets)?;
    matcher.build_polymarket_index(&polymarket_markets)?;
    
    // 双向匹配
    println!("   🔍 匹配市场...");
    let matches = matcher.find_matches_bidirectional(&polymarket_markets, &kalshi_markets);
    println!("      ✅ 找到 {} 个匹配对", matches.len());
    
    // 所有匹配对都加入追踪列表
    let mut all_matches: Vec<(Market, Market, f64)> = Vec::new();
    for (pm_market, kalshi_market, similarity) in &matches {
        all_matches.push((pm_market.clone(), kalshi_market.clone(), *similarity));
    }
    
    // 验证每个匹配对，统计有套利机会的
    let mut verified_count = 0;
    
    for (pm_market, kalshi_market, similarity) in &matches {
        if let Some(verified) = validate_arbitrage_pair(
            polymarket, kalshi, arb_detector, 
            pm_market, kalshi_market, *similarity, trade_amount, logger
        ).await {
            verified_count += 1;
            
            if let Err(e) = logger.log_opportunity(&verified) {
                eprintln!("         ⚠️ 记录日志失败: {}", e);
            }
        }
    }
    
    // 更新追踪列表（所有匹配对）
    monitor_state.update_tracked_pairs(all_matches);
    
    Ok((matches.len(), verified_count))
}





/// 执行价格追踪周期
async fn run_tracking_cycle(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    arb_detector: &ArbitrageDetector,
    logger: &MonitorLogger,
    monitor_state: &mut MonitorState,
    trade_amount: f64,
) -> Result<usize> {
    println!("   📡 执行价格追踪...");
    println!("      追踪 {} 个匹配对", monitor_state.tracked_pairs.len());
    
    let mut opportunity_count = 0;
    
    for pair in monitor_state.tracked_pairs.iter_mut() {
        if !pair.active {
            continue;
        }
        
        if let Some(verified) = validate_arbitrage_pair(
            polymarket, kalshi, arb_detector,
            &pair.pm_market, &pair.kalshi_market, pair.similarity, trade_amount, logger
        ).await {
            opportunity_count += 1;
            pair.last_check = Utc::now();
            if verified.net_profit > pair.best_profit {
                pair.best_profit = verified.net_profit;
            }
            
            if let Err(e) = logger.log_opportunity(&verified) {
                eprintln!("         ⚠️ 记录日志失败: {}", e);
            }
        }
    }
    
    Ok(opportunity_count)
}

/// 运行单个监控周期
async fn run_cycle(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    matcher: &mut MarketMatcher,
    arb_detector: &ArbitrageDetector,
    logger: &MonitorLogger,
    monitor_state: &mut MonitorState,
) -> Result<CycleStats> {
    let start_time = Local::now();
    println!("🔄 开始新周期 #{} - {}", monitor_state.current_cycle, start_time.format("%H:%M:%S"));
    
    let trade_amount = 100.0;
    
    let (new_matches, opportunities) = if monitor_state.should_full_match() {
        // 全量匹配周期
        run_full_match_cycle(
            polymarket, kalshi, matcher, arb_detector, logger, 
            monitor_state, trade_amount
        ).await?
    } else {
        // 价格追踪周期
        let opportunities = run_tracking_cycle(
            polymarket, kalshi, arb_detector, logger,
            monitor_state, trade_amount
        ).await?;
        (0, opportunities)
    };
    
    let elapsed = Local::now() - start_time;
    println!("   ⏱️ 周期完成, 耗时: {}ms", elapsed.num_milliseconds());
    
    Ok(CycleStats {
        new_matches,
        arbitrage_opportunities: opportunities,
    })
}

/// 首次获取市场
async fn fetch_initial_markets(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
) -> Result<(Vec<Market>, Vec<Market>)> {
    println!("   📡 获取 Polymarket 市场...");
    let polymarket_markets: Vec<Market> = polymarket.fetch_all_markets().await?;
    println!("      ✅ 获取到 {} 个市场", polymarket_markets.len());
    
    println!("   📡 获取 Kalshi 市场...");
    let kalshi_markets: Vec<Market> = kalshi.fetch_all_markets().await?;
    println!("      ✅ 获取到 {} 个市场", kalshi_markets.len());
    
    Ok((kalshi_markets, polymarket_markets))
}

/// 周期统计
struct CycleStats {
    new_matches: usize,
    arbitrage_opportunities: usize,
}