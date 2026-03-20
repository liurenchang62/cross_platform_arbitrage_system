// market_matcher.rs
//! 市场匹配器，使用 TF-IDF + K-D Tree 实现快速准确的市场匹配

use crate::market::Market;
use crate::category_mapper::CategoryMapper;
use crate::unclassified_logger::UnclassifiedLogger;
use crate::query_params::SIMILARITY_THRESHOLD;
use crate::category_vectorizer::{CategoryVectorizerManager};
use crate::text_vectorizer::VectorizerConfig;
use crate::validation::ValidationPipeline;
use tokio::task;

use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde_json::Value;

/// 匹配结果置信度
#[derive(Debug, Clone)]
pub struct MatchConfidence {
    pub overall_score: f64,
    pub text_similarity: f64,
    pub date_match: bool,
    pub category_match: bool,
}

impl MatchConfidence {
    pub fn is_high_confidence(&self) -> bool {
        self.overall_score >= 0.75
    }

    pub fn is_medium_confidence(&self) -> bool {
        self.overall_score >= 0.50 && self.overall_score < 0.75
    }
}

/// 市场匹配器配置
#[derive(Debug, Clone)]
pub struct MarketMatcherConfig {
    pub similarity_threshold: f64,
    pub vectorizer_config: VectorizerConfig,
    pub use_date_boost: bool,
    pub use_category_boost: bool,
    pub date_boost_factor: f64,
    pub category_boost_factor: f64,
}

impl Default for MarketMatcherConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: SIMILARITY_THRESHOLD,
            vectorizer_config: VectorizerConfig::default(),
            use_date_boost: true,
            use_category_boost: true,
            date_boost_factor: 0.05,
            category_boost_factor: 0.03,
        }
    }
}

/// 市场匹配器
pub struct MarketMatcher {
    pub config: MarketMatcherConfig,
    pub category_mapper: CategoryMapper,
    pub unclassified_logger: Option<UnclassifiedLogger>,
    pub kalshi_vectorizers: Arc<CategoryVectorizerManager>,
    pub polymarket_vectorizers: Arc<CategoryVectorizerManager>,
    pub fitted: bool,
    kalshi_market_cache: Arc<HashMap<String, Market>>,
    polymarket_market_cache: Arc<HashMap<String, Market>>,
    pub validation_pipeline: ValidationPipeline,
}

impl MarketMatcher {
    pub fn new(config: MarketMatcherConfig, category_mapper: CategoryMapper) -> Self {
        Self {
            config,
            category_mapper,
            unclassified_logger: None,
            kalshi_vectorizers: Arc::new(CategoryVectorizerManager::new()),
            polymarket_vectorizers: Arc::new(CategoryVectorizerManager::new()),
            fitted: false,
            kalshi_market_cache: Arc::new(HashMap::new()),
            polymarket_market_cache: Arc::new(HashMap::new()),
            validation_pipeline: ValidationPipeline::new(),
        }
    }

    pub fn with_logger(mut self, logger: UnclassifiedLogger) -> Self {
        self.unclassified_logger = Some(logger);
        self
    }

    pub fn fit_vectorizer(&mut self, kalshi_markets: &[Market], polymarket_markets: &[Market]) -> Result<()> {
        println!("📚 按类别训练向量化器...");
        
        let mut kalshi_by_category: HashMap<String, Vec<String>> = HashMap::new();
        for market in kalshi_markets {
            let categories = self.category_mapper.classify(&market.title);
            for cat in categories {
                kalshi_by_category
                    .entry(cat)
                    .or_insert_with(Vec::new)
                    .push(market.title.clone());
            }
        }
        
        let mut polymarket_by_category: HashMap<String, Vec<String>> = HashMap::new();
        for market in polymarket_markets {
            let categories = self.category_mapper.classify(&market.title);
            for cat in categories {
                polymarket_by_category
                    .entry(cat)
                    .or_insert_with(Vec::new)
                    .push(market.title.clone());
            }
        }
        
        // 需要获取可变引用
        let kalshi_vec = Arc::get_mut(&mut self.kalshi_vectorizers).unwrap();
        let polymarket_vec = Arc::get_mut(&mut self.polymarket_vectorizers).unwrap();
        
        println!("   📊 训练 Kalshi 类别向量化器...");
        kalshi_vec.fit_all(kalshi_by_category);
        
        println!("   📊 训练 Polymarket 类别向量化器...");
        polymarket_vec.fit_all(polymarket_by_category);
        
        self.fitted = true;
        Ok(())
    }

    pub fn build_kalshi_index(&mut self, markets: &[Market]) -> Result<(), anyhow::Error> {
        if markets.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Kalshi 市场索引...");
        
        let mut by_category: HashMap<String, Vec<(String, String, Option<Value>)>> = HashMap::new();
        let mut cache = HashMap::new();
        
        for market in markets {
            let market_id = format!("{}:{}", market.platform, market.market_id);
            cache.insert(market_id.clone(), market.clone());
            
            let categories = self.category_mapper.classify(&market.title);
            let data = Some(serde_json::json!({
                "title": market.title,
                "platform": market.platform,
            }));
            
            if categories.is_empty() {
                if let Some(logger) = &mut self.unclassified_logger {
                    if let Err(e) = logger.log_unclassified(market) {
                        eprintln!("   ⚠️ 记录未分类市场失败: {}", e);
                    }
                }
                by_category
                    .entry("unclassified".to_string())
                    .or_insert_with(Vec::new)
                    .push((market_id, market.title.clone(), data));
            } else {
                for cat in categories {
                    by_category
                        .entry(cat)
                        .or_insert_with(Vec::new)
                        .push((market_id.clone(), market.title.clone(), data.clone()));
                }
            }
        }
        
        // 注意：必须分别缓存两边市场，否则后一次 build_* 会覆盖，导致某个方向 0 匹配
        self.kalshi_market_cache = Arc::new(cache);
        
        let kalshi_vec = Arc::get_mut(&mut self.kalshi_vectorizers).unwrap();
        for (category, items) in by_category {
            if let Some(vectorizer) = kalshi_vec.get_or_create(&category) {
                vectorizer.add_markets_batch(items)?;
            }
        }
        
        println!("   ✅ Kalshi 索引构建完成，总市场数: {}", kalshi_vec.total_size());
        Ok(())
    }

    pub fn build_polymarket_index(&mut self, markets: &[Market]) -> Result<(), anyhow::Error> {
        if markets.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Polymarket 市场索引...");
        
        let mut by_category: HashMap<String, Vec<(String, String, Option<Value>)>> = HashMap::new();
        let mut cache = HashMap::new();
        
        for market in markets {
            let market_id = format!("{}:{}", market.platform, market.market_id);
            cache.insert(market_id.clone(), market.clone());
            
            let categories = self.category_mapper.classify(&market.title);
            let data = Some(serde_json::json!({
                "title": market.title,
                "platform": market.platform,
            }));
            
            if categories.is_empty() {
                if let Some(logger) = &mut self.unclassified_logger {
                    if let Err(e) = logger.log_unclassified(market) {
                        eprintln!("   ⚠️ 记录未分类市场失败: {}", e);
                    }
                }
                by_category
                    .entry("unclassified".to_string())
                    .or_insert_with(Vec::new)
                    .push((market_id, market.title.clone(), data));
            } else {
                for cat in categories {
                    by_category
                        .entry(cat)
                        .or_insert_with(Vec::new)
                        .push((market_id.clone(), market.title.clone(), data.clone()));
                }
            }
        }
        
        self.polymarket_market_cache = Arc::new(cache);
        
        let polymarket_vec = Arc::get_mut(&mut self.polymarket_vectorizers).unwrap();
        for (category, items) in by_category {
            if let Some(vectorizer) = polymarket_vec.get_or_create(&category) {
                vectorizer.add_markets_batch(items)?;
            }
        }
        
        println!("   ✅ Polymarket 索引构建完成，总市场数: {}", polymarket_vec.total_size());
        Ok(())
    }









    // 在 find_matches_bidirectional 函数中修改调用和合并逻辑

    pub async fn find_matches_bidirectional(
        &mut self,
        pm_markets: &[Market],
        kalshi_markets: &[Market],
    ) -> Vec<(Market, Market, f64, String, String, bool)> {  // 修改返回类型
        if !self.fitted {
            println!("⚠️ 索引未构建");
            return Vec::new();
        }

        self.validation_pipeline.reset_filtered_count();

        println!("\n🔍 ====== 开始双向匹配 ======");
        
        // 克隆需要的数据用于并行任务
        let kalshi_vec = self.kalshi_vectorizers.clone();
        let polymarket_vec = self.polymarket_vectorizers.clone();
        
        let category_mapper1 = self.category_mapper.clone();
        let category_mapper2 = self.category_mapper.clone();
        
        let config1 = self.config.clone();
        let config2 = self.config.clone();
        
        // 方向1（PM→Kalshi）查询 Kalshi 索引，必须用 Kalshi cache 才能用 item.id 命中目标市场
        let market_cache1 = self.kalshi_market_cache.clone();
        // 方向2（Kalshi→PM）查询 PM 索引，必须用 PM cache
        let market_cache2 = self.polymarket_market_cache.clone();
        
        let pm_markets_vec = pm_markets.to_vec();
        let kalshi_markets_vec = kalshi_markets.to_vec();
        
        let start_time = std::time::Instant::now();
        
        println!("\n📌 并行执行两个方向（spawn_blocking 确保 CPU 密集任务真并行）...");
        
        // 使用 spawn_blocking 保证 CPU 密集的匹配循环真并行，不阻塞 async worker
        let handle1 = task::spawn_blocking(move || {
            let mut pipeline = ValidationPipeline::new();
            let matches = Self::find_matches_directional_sync(
                &pm_markets_vec,
                &kalshi_vec,
                &category_mapper1,
                &config1,
                &market_cache1,
                &mut pipeline,
                "PM→Kalshi",
            );
            (matches, pipeline)
        });
        
        let handle2 = task::spawn_blocking(move || {
            let mut pipeline = ValidationPipeline::new();
            let matches = Self::find_matches_directional_sync(
                &kalshi_markets_vec,
                &polymarket_vec,
                &category_mapper2,
                &config2,
                &market_cache2,
                &mut pipeline,
                "Kalshi→PM",
            );
            (matches, pipeline)
        });
        
        let (res1, res2) = tokio::join!(handle1, handle2);
        
        let (matches1, pipeline1) = res1.unwrap();
        let (matches2, pipeline2) = res2.unwrap();
        
        let initial_count = matches1.len() + matches2.len();
        
        let mut all_matches = Vec::new();
        let mut seen_pairs = std::collections::HashSet::new();
        
        // 处理方向1的匹配 (PM→Kalshi)
        for (m1, m2, score, pm_side, ks_side, needs_inversion) in matches1 {
            let pair_key = format!("{}:{}", m1.market_id, m2.market_id);
            let reverse_key = format!("{}:{}", m2.market_id, m1.market_id);
            
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                if m1.platform == "polymarket" && m2.platform == "kalshi" {
                    all_matches.push((m1, m2, score, pm_side, ks_side, needs_inversion));
                } else {
                    // 如果方向反了，交换并保留方向信息
                    all_matches.push((m2, m1, score, pm_side, ks_side, needs_inversion));
                }
            }
        }
        
        // 处理方向2的匹配 (Kalshi→PM)
        for (m1, m2, score, pm_side, ks_side, needs_inversion) in matches2 {
            let pair_key = format!("{}:{}", m2.market_id, m1.market_id);
            let reverse_key = format!("{}:{}", m1.market_id, m2.market_id);
            
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                // 方向2中，m1是Kalshi，m2是PM，需要交换并保留方向信息
                all_matches.push((m2, m1, score, pm_side, ks_side, needs_inversion));
            }
        }
        
        all_matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let final_count = all_matches.len();
        let filtered_count = initial_count - final_count;
        
        self.validation_pipeline.filtered_count = filtered_count;
        
        // 合并留存样本
        for (cat, samples) in pipeline1.retained_samples {
            self.validation_pipeline.retained_samples.insert(cat, samples);
        }
        for (cat, samples) in pipeline2.retained_samples {
            self.validation_pipeline.retained_samples.insert(cat, samples);
        }

        let elapsed = start_time.elapsed();
        println!("\n📊 ====== 匹配统计 ======");
        println!("   并行匹配耗时: {:?}", elapsed);
        println!("   初筛匹配对: {} 个", initial_count);
        println!("   二筛过滤: {} 个", filtered_count);
        println!("   二筛后待跟踪: {} 个", final_count);
        
        self.validation_pipeline.print_retained_samples();
        
        all_matches
    }







    /// 同步版本：供 spawn_blocking 使用，实现真并行
    fn find_matches_directional_sync(
        query_markets: &[Market],
        target_vectorizers: &CategoryVectorizerManager,
        category_mapper: &CategoryMapper,
        config: &MarketMatcherConfig,
        target_market_cache: &HashMap<String, Market>,
        validation_pipeline: &mut ValidationPipeline,
        direction_label: &str,
    ) -> Vec<(Market, Market, f64, String, String, bool)> {
        Self::find_matches_directional_internal_impl(
            query_markets,
            target_vectorizers,
            category_mapper,
            config,
            target_market_cache,
            validation_pipeline,
            direction_label,
        )
    }

    fn find_matches_directional_internal_impl(
        query_markets: &[Market],
        target_vectorizers: &CategoryVectorizerManager,
        category_mapper: &CategoryMapper,
        config: &MarketMatcherConfig,
        target_market_cache: &HashMap<String, Market>,
        validation_pipeline: &mut ValidationPipeline,
        direction_label: &str,
    ) -> Vec<(Market, Market, f64, String, String, bool)> {
        let mut all_matches = Vec::new();
        let total = query_markets.len();
        
        println!("      🔍 匹配 {} 个市场 [{}]...", total, direction_label);
        let start_time = std::time::Instant::now();
        
        for (idx, query_market) in query_markets.iter().enumerate() {
            if idx > 0 && idx % 1000 == 0 {
                let elapsed = start_time.elapsed();
                let avg_time = elapsed.as_millis() as f64 / idx as f64;
                let remaining = (total - idx) as f64 * avg_time;
                println!("        进度: {}/{} 个市场 [{}], 已用 {:?}, 预计剩余 {:?}", 
                    idx, total, direction_label,
                    humantime::format_duration(elapsed),
                    humantime::format_duration(std::time::Duration::from_millis(remaining as u64)));
            }
            
            let query_categories = category_mapper.classify(&query_market.title);
            
            for category in query_categories {
                if let Some(vectorizer) = target_vectorizers.get(&category) {
                    let similar = vectorizer.find_similar(
                        &query_market.title,
                        config.similarity_threshold,
                        5,
                    );
                    
                    for (item, similarity) in similar {
                        if let Some(target_market) = target_market_cache.get(&item.id) {
                            // 二筛始终需要 (pm_title, kalshi_title)；Kalshi→PM 方向时 query=Kalshi target=PM，需交换
                            let (pm_title, kalshi_title) = if direction_label == "Kalshi→PM" {
                                (&target_market.title, &query_market.title)
                            } else {
                                (&query_market.title, &target_market.title)
                            };
                            if let Some(match_info) = validation_pipeline.validate(
                                pm_title,
                                kalshi_title,
                                similarity,
                                &category,
                            ) {
                                let confidence = Self::calculate_confidence(
                                    query_market,
                                    target_market,
                                    similarity,
                                    config,
                                );
                                
                                if confidence.overall_score >= config.similarity_threshold {
                                    all_matches.push((
                                        query_market.clone(),
                                        target_market.clone(),
                                        confidence.overall_score,
                                        match_info.pm_side,
                                        match_info.kalshi_side,
                                        match_info.needs_inversion,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let elapsed = start_time.elapsed();
        println!("        匹配完成 [{}], 耗时: {:?}, 找到 {} 个匹配", 
            direction_label, elapsed, all_matches.len());
        
        all_matches
    }



    fn calculate_confidence(
        market1: &Market,
        market2: &Market,
        vector_similarity: f64,
        config: &MarketMatcherConfig,
    ) -> MatchConfidence {
        let mut final_score = vector_similarity;

        let date_match = if let (Some(d1), Some(d2)) = (market1.resolution_date, market2.resolution_date) {
            let diff = (d1 - d2).num_seconds().abs();
            let match_quality = if diff <= 86400 { 1.0 } else { 0.0 };
            
            if config.use_date_boost {
                final_score += config.date_boost_factor * match_quality;
            }
            match_quality > 0.0
        } else {
            false
        };

        let category_match = if let (Some(c1), Some(c2)) = (&market1.category, &market2.category) {
            let match_quality = if c1.to_lowercase() == c2.to_lowercase() { 1.0 } else { 0.0 };
            
            if config.use_category_boost {
                final_score += config.category_boost_factor * match_quality;
            }
            match_quality > 0.0
        } else {
            false
        };

        final_score = final_score.min(1.0);

        MatchConfidence {
            overall_score: final_score,
            text_similarity: vector_similarity,
            date_match,
            category_match,
        }
    }

    pub fn kalshi_index_size(&self) -> usize {
        self.kalshi_vectorizers.total_size()
    }

    pub fn polymarket_index_size(&self) -> usize {
        self.polymarket_vectorizers.total_size()
    }
}