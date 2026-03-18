// market_matcher.rs
//! 市场匹配器，使用 TF-IDF + K-D Tree 实现快速准确的市场匹配

use crate::market::Market;
use crate::category_mapper::CategoryMapper;
use crate::unclassified_logger::UnclassifiedLogger;
use crate::query_params::SIMILARITY_THRESHOLD;
use crate::category_vectorizer::{CategoryVectorizerManager};
use crate::text_vectorizer::VectorizerConfig;
use crate::validation::ValidationPipeline;

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

    pub fn find_matches_bidirectional(
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
        
        println!("\n📊 共同类别检查:");
        let all_categories = self.polymarket_vectorizers.get_all_categories();
        
        for cat in &all_categories {
            let k_size = self.kalshi_vectorizers.category_size(cat);
            let p_size = self.polymarket_vectorizers.category_size(cat);
            if k_size > 0 && p_size > 0 {
                println!("   ✅ {}: Kalshi {} 个, Polymarket {} 个", cat, k_size, p_size);
            }
        }
        
        println!("\n   📌 方向1: Polymarket → Kalshi");
        // 使用静态方法，完全避免借用 self
        let matches1 = Self::find_matches_directional_static(
            pm_markets,
            &self.kalshi_vectorizers,
            &self.category_mapper,
            &self.config,
            &self.market_cache,
            &mut self.validation_pipeline,
        );
        
        println!("\n   📌 方向2: Kalshi → Polymarket");
        let matches2 = Self::find_matches_directional_static(
            kalshi_markets,
            &self.polymarket_vectorizers,
            &self.category_mapper,
            &self.config,
            &self.market_cache,
            &mut self.validation_pipeline,
        );
        
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

        println!("\n📊 ====== 匹配完成 ======");
        println!("   共找到 {} 个匹配对", all_matches.len());
        println!("   二筛过滤: {} 个", self.validation_pipeline.filtered_count);
        
        println!("\n📊 最高相似度匹配 (前10):");
        for (i, (pm, kalshi, score)) in all_matches.iter().take(10).enumerate() {
            println!("  {}. 相似度: {:.3}", i+1, score);
            println!("     PM: {}", pm.title);
            println!("     Kalshi: {}", kalshi.title);
            println!();
        }
        
        all_matches
    }

    /// 静态方法，不借用 self，避免借用检查错误
    fn find_matches_directional_static(
        query_markets: &[Market],
        target_vectorizers: &CategoryVectorizerManager,
        category_mapper: &CategoryMapper,
        config: &MarketMatcherConfig,
        market_cache: &HashMap<String, Market>,
        validation_pipeline: &mut ValidationPipeline,
    ) -> Vec<(Market, Market, f64)> {
        let mut all_matches = Vec::new();
        let total = query_markets.len();
        
        println!("\n      🔍 开始匹配 {} 个市场...", total);
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
                        10,
                    );
                    
                    for (item, similarity) in similar {
                        if let Some(target_market) = market_cache.get(&item.id) {
                            // 在 find_matches_directional_static 函数中修改调用

                            if !validation_pipeline.validate(
                                &query_market.title, 
                                &target_market.title,
                                similarity,
                                &category,
                            ) {
                                continue;
                            }
                            
                            let confidence = Self::calculate_confidence_static(
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
        
        // 在函数末尾，返回 all_matches 之前
        validation_pipeline.print_retained_samples();
        println!("        匹配完成，找到 {} 个匹配", all_matches.len());
        
        all_matches
    }

    /// 静态方法计算置信度
    fn calculate_confidence_static(
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