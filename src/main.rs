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
    
    // 确定策略对应的买卖方向
    let (pm_side, kalshi_side) = if pm_market.title.contains("O/U") {
        ("YES", "NO")  // 默认
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
    
    // 计算滑点
    let (pm_slip, pm_avg) = if let Some(ob) = pm_orderbook {
        let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
        (info.slippage_percent, info.avg_price)
    } else {
        (0.0, if pm_side == "YES" { pm_prices.yes } else { pm_prices.no })
    };
    
    let (kalshi_slip, kalshi_avg) = if let Some(ob) = kalshi_orderbook {
        let info = calculate_slippage_with_fixed_usdt(&ob, trade_amount);
        (info.slippage_percent, info.avg_price)
    } else {
        (0.0, if kalshi_side == "YES" { kalshi_prices.yes } else { kalshi_prices.no })
    };
    
    // 计算最终利润（考虑滑点、手续费、Gas）
    let verified = arb_detector.calculate_final_profit(
        &pm_prices,
        &kalshi_prices,
        pm_slip,
        kalshi_slip,
    )?;
    
    // 输出验证结果
    println!("\n  📌 验证通过 (相似度: {:.3})", similarity);
    println!("     PM: {}", pm_market.title);
    println!("     Kalshi: {}", kalshi_market.title);
    println!();
    println!("     📊 成本分析:");
    println!("        Polymarket {}: 最优价 {:.3} → 考虑滑点平均价 {:.3} ({:+.2}%)", 
        pm_side, 
        if pm_side == "YES" { pm_prices.yes } else { pm_prices.no },
        pm_avg, pm_slip);
    println!("        Kalshi {}: 最优价 {:.3} → 考虑滑点平均价 {:.3} ({:+.2}%)", 
        kalshi_side,
        if kalshi_side == "YES" { kalshi_prices.yes } else { kalshi_prices.no },
        kalshi_avg, kalshi_slip);
    println!();
    println!("     💰 利润计算:");
    println!("        理想利润: ${:.3}", verified.net_profit + verified.fees + verified.gas_fee);
    println!("        - 滑点影响: ${:.3}", (verified.total_cost - (pm_prices.yes + kalshi_prices.no)).abs());
    println!("        - 手续费: ${:.3}", verified.fees);
    println!("        - Gas费: ${:.3}", verified.gas_fee);
    println!("        = 最终净利润: ${:.3}", verified.final_profit);
    println!("        ROI: {:.1}%", verified.final_roi_percent);
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
    
    let polymarket_markets: Vec<Market> = polymarket.fetch_all_markets().await?;
    let kalshi_markets: Vec<Market> = kalshi.fetch_all_markets().await?;
    
    println!("      Polymarket: {} 个市场, Kalshi: {} 个市场", 
        polymarket_markets.len(), kalshi_markets.len());
    
    println!("\n   🔄 重建索引...");
    matcher.build_kalshi_index(&kalshi_markets)?;
    matcher.build_polymarket_index(&polymarket_markets)?;
    
    println!("   🔍 匹配市场...");
    let matches = matcher.find_matches_bidirectional(&polymarket_markets, &kalshi_markets).await;
    println!("      ✅ 找到 {} 个匹配对", matches.len());
    
    let mut all_matches: Vec<(Market, Market, f64)> = Vec::new();
    for (pm_market, kalshi_market, similarity) in &matches {
        all_matches.push((pm_market.clone(), kalshi_market.clone(), *similarity));
    }
    
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