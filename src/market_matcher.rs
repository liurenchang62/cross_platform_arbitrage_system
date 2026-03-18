// market_matcher.rs
//! 市场匹配器，使用 TF-IDF + K-D Tree 实现快速准确的市场匹配

use crate::market::Market;
use crate::category_mapper::CategoryMapper;
use crate::unclassified_logger::UnclassifiedLogger;
use crate::query_params::SIMILARITY_THRESHOLD;
use crate::category_vectorizer::{CategoryVectorizerManager};
use crate::text_vectorizer::VectorizerConfig;
use crate::validation::ValidationPipeline;
use tokio::join;

use std::collections::HashMap;
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
    pub kalshi_vectorizers: CategoryVectorizerManager,
    pub polymarket_vectorizers: CategoryVectorizerManager,
    pub fitted: bool,
    market_cache: HashMap<String, Market>,
    pub validation_pipeline: ValidationPipeline,
}

impl MarketMatcher {
    pub fn new(config: MarketMatcherConfig, category_mapper: CategoryMapper) -> Self {
        Self {
            config,
            category_mapper,
            unclassified_logger: None,
            kalshi_vectorizers: CategoryVectorizerManager::new(),
            polymarket_vectorizers: CategoryVectorizerManager::new(),
            fitted: false,
            market_cache: HashMap::new(),
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
        
        println!("   📊 训练 Kalshi 类别向量化器...");
        self.kalshi_vectorizers.fit_all(kalshi_by_category);
        
        println!("   📊 训练 Polymarket 类别向量化器...");
        self.polymarket_vectorizers.fit_all(polymarket_by_category);
        
        self.fitted = true;
        Ok(())
    }

    pub fn build_kalshi_index(&mut self, markets: &[Market]) -> Result<(), anyhow::Error> {
        if markets.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Kalshi 市场索引...");
        
        let mut by_category: HashMap<String, Vec<(String, String, Option<Value>)>> = HashMap::new();
        
        for market in markets {
            let market_id = format!("{}:{}", market.platform, market.market_id);
            self.market_cache.insert(market_id.clone(), market.clone());
            
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
        
        for (category, items) in by_category {
            if let Some(vectorizer) = self.kalshi_vectorizers.get_or_create(&category) {
                vectorizer.add_markets_batch(items)?;
            }
        }
        
        println!("   ✅ Kalshi 索引构建完成，总市场数: {}", self.kalshi_vectorizers.total_size());
        Ok(())
    }

    pub fn build_polymarket_index(&mut self, markets: &[Market]) -> Result<(), anyhow::Error> {
        if markets.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Polymarket 市场索引...");
        
        let mut by_category: HashMap<String, Vec<(String, String, Option<Value>)>> = HashMap::new();
        
        for market in markets {
            let market_id = format!("{}:{}", market.platform, market.market_id);
            self.market_cache.insert(market_id.clone(), market.clone());
            
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
        
        for (category, items) in by_category {
            if let Some(vectorizer) = self.polymarket_vectorizers.get_or_create(&category) {
                vectorizer.add_markets_batch(items)?;
            }
        }
        
        println!("   ✅ Polymarket 索引构建完成，总市场数: {}", self.polymarket_vectorizers.total_size());
        Ok(())
    }

    pub async fn find_matches_bidirectional(
        &mut self,
        pm_markets: &[Market],
        kalshi_markets: &[Market],
    ) -> Vec<(Market, Market, f64)> {
        if !self.fitted {
            println!("⚠️ 索引未构建");
            return Vec::new();
        }

        self.validation_pipeline.reset_filtered_count();

        println!("\n🔍 ====== 开始双向匹配 ======");
        
        // 创建两个独立的验证管道
        let mut pipeline1 = ValidationPipeline::new();
        let mut pipeline2 = ValidationPipeline::new();
        
        let start_time = std::time::Instant::now();
        
        println!("\n📌 并行执行两个方向...");
        
        // 真正的并行执行
        let (matches1, matches2) = tokio::join!(
            Self::find_matches_directional(
                pm_markets,
                &self.kalshi_vectorizers,
                &self.category_mapper,
                &self.config,
                &self.market_cache,
                &mut pipeline1,
            ),
            Self::find_matches_directional(
                kalshi_markets,
                &self.polymarket_vectorizers,
                &self.category_mapper,
                &self.config,
                &self.market_cache,
                &mut pipeline2,
            )
        );
        
        let initial_count = matches1.len() + matches2.len();
        
        let mut all_matches = Vec::new();
        let mut seen_pairs = std::collections::HashSet::new();
        
        for (m1, m2, score) in matches1 {
            let pair_key = format!("{}:{}", m1.market_id, m2.market_id);
            let reverse_key = format!("{}:{}", m2.market_id, m1.market_id);
            
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                if m1.platform == "polymarket" && m2.platform == "kalshi" {
                    all_matches.push((m1, m2, score));
                } else {
                    all_matches.push((m2, m1, score));
                }
            }
        }
        
        for (m1, m2, score) in matches2 {
            let pair_key = format!("{}:{}", m2.market_id, m1.market_id);
            let reverse_key = format!("{}:{}", m1.market_id, m2.market_id);
            
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                all_matches.push((m2, m1, score));
            }
        }
        
        all_matches.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        let final_count = all_matches.len();
        let filtered_count = initial_count - final_count;
        
        self.validation_pipeline.filtered_count = filtered_count;
        
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

    async fn find_matches_directional(
        query_markets: &[Market],
        target_vectorizers: &CategoryVectorizerManager,
        category_mapper: &CategoryMapper,
        config: &MarketMatcherConfig,
        market_cache: &HashMap<String, Market>,
        validation_pipeline: &mut ValidationPipeline,
    ) -> Vec<(Market, Market, f64)> {
        let mut all_matches = Vec::new();
        let total = query_markets.len();
        
        println!("      🔍 匹配 {} 个市场...", total);
        let start_time = std::time::Instant::now();
        
        for (idx, query_market) in query_markets.iter().enumerate() {
            if idx > 0 && idx % 1000 == 0 {
                let elapsed = start_time.elapsed();
                let avg_time = elapsed.as_millis() as f64 / idx as f64;
                let remaining = (total - idx) as f64 * avg_time;
                println!("        进度: {}/{} 个市场, 已用 {:?}, 预计剩余 {:?}", 
                    idx, total, 
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
                        if let Some(target_market) = market_cache.get(&item.id) {
                            if !validation_pipeline.validate(
                                &query_market.title, 
                                &target_market.title,
                                similarity,
                                &category,
                            ) {
                                continue;
                            }
                            
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
                                ));
                            }
                        }
                    }
                }
            }
        }
        
        let elapsed = start_time.elapsed();
        println!("        匹配完成，耗时: {:?}", elapsed);
        
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