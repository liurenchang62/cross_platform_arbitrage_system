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
        similarity_threshold: 0.5,
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

/// 运行单个监控周期
async fn run_cycle(
    polymarket: &PolymarketClient,
    kalshi: &KalshiClient,
    matcher: &mut EventMatcher,
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
    
    // 每10个周期重建一次索引（但保持向量化器不变）
    if cycle_count % 10 == 0 {
        println!("   🔄 重建索引...");
        matcher.kalshi_index.clear();
        matcher.polymarket_index.clear();
        matcher.build_kalshi_index(&kalshi_events)?;
        matcher.build_polymarket_index(&pm_events)?;
    }
    
    // 双向匹配事件
    println!("   🔍 双向匹配事件...");
    let matches = matcher.find_matches_bidirectional(&pm_events, &kalshi_events);
    println!("      ✅ 找到 {} 个匹配对", matches.len());
    
    // 检查套利机会
    let mut opportunity_count = 0;
    
    for (pm_event, kalshi_event, similarity) in &matches {
        // 获取价格
        let pm_prices = match polymarket.fetch_prices(pm_event).await {
            Ok(p) => p,
            Err(e) => {
                println!("      ⚠️ 无法获取 Polymarket 价格: {}", e);
                continue;
            }
        };
        
        let kalshi_prices = match kalshi.get_market_prices(&kalshi_event.event_id).await {
            Ok(Some(p)) => p,
            _ => {
                println!("      ⚠️ 无法获取 Kalshi 价格");
                continue;
            }
        };
        
        // 检查套利
        if let Some(opportunity) = arb_detector.check_arbitrage(&pm_prices, &kalshi_prices) {
            opportunity_count += 1;
            
            // 记录套利机会
            println!("\n      🎯 发现套利机会! (相似度: {:.2})", similarity);
            println!("         📌 PM: {}", pm_event.title);
            println!("         📌 Kalshi: {}", kalshi_event.title);
            println!("         💰 策略: {}", opportunity.strategy);
            println!("         💵 净利润: ${:.3}", opportunity.net_profit);
            println!("         📊 ROI: {:.1}%", opportunity.roi_percent);
            
            if let Err(e) = logger.log_opportunity(&opportunity) {
                eprintln!("         ⚠️ 记录日志失败: {}", e);
            }
        }
    }
    
    let elapsed = chrono::Local::now() - start_time;
    println!("   ⏱️ 周期完成, 耗时: {}ms", elapsed.num_milliseconds());
    
    Ok(CycleStats {
        matches: matches.len(),
        opportunities: opportunity_count,
    })
}

/// 周期统计
struct CycleStats {
    matches: usize,
    opportunities: usize,
}