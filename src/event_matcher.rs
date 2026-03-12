// src/event_matcher.rs
//! 事件匹配器，使用 TF-IDF + K-D Tree 实现快速准确的事件匹配
//! 
//! 职责：
//! 1. 使用 TextVectorizer 将事件文本转换为向量
//! 2. 使用 CategoryIndexManager 按类别管理索引并查找相似事件
//! 3. 返回匹配结果及相似度分数

use crate::event::Event;
use crate::text_vectorizer::{TextVectorizer, VectorizerConfig};
use crate::category_mapper::CategoryMapper;
use crate::category_index_manager::CategoryIndexManager;
use crate::unclassified_logger::UnclassifiedLogger;

use std::collections::HashMap;
use anyhow::Result;

/// 匹配结果置信度
#[derive(Debug, Clone)]
pub struct MatchConfidence {
    /// 总体相似度分数
    pub overall_score: f64,
    /// 文本向量相似度（余弦相似度）
    pub text_similarity: f64,
    /// 日期是否匹配（用于辅助验证）
    pub date_match: bool,
    /// 类别是否匹配（用于辅助验证）
    pub category_match: bool,
}

impl MatchConfidence {
    /// 是否为高置信度匹配（>= 0.75）
    pub fn is_high_confidence(&self) -> bool {
        self.overall_score >= 0.75
    }

    /// 是否为中等置信度匹配（0.50 - 0.75）
    pub fn is_medium_confidence(&self) -> bool {
        self.overall_score >= 0.50 && self.overall_score < 0.75
    }
}

/// 事件匹配器配置
#[derive(Debug, Clone)]
pub struct EventMatcherConfig {
    /// 相似度阈值（默认 0.7）
    pub similarity_threshold: f64,
    /// 向量化器配置
    pub vectorizer_config: VectorizerConfig,
    /// 是否启用日期辅助匹配
    pub use_date_boost: bool,
    /// 是否启用类别辅助匹配
    pub use_category_boost: bool,
    /// 日期匹配加成系数
    pub date_boost_factor: f64,
    /// 类别匹配加成系数
    pub category_boost_factor: f64,
}

impl Default for EventMatcherConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.7,
            vectorizer_config: VectorizerConfig::default(),
            use_date_boost: true,
            use_category_boost: true,
            date_boost_factor: 0.05,
            category_boost_factor: 0.03,
        }
    }
}

/// 事件匹配器
pub struct EventMatcher {
    /// 配置
    pub config: EventMatcherConfig,
    /// 文本向量化器
    pub vectorizer: TextVectorizer,
    /// Kalshi 索引管理器
    pub kalshi_index: CategoryIndexManager,
    /// Polymarket 索引管理器
    pub polymarket_index: CategoryIndexManager,
    /// 类别映射器
    pub category_mapper: CategoryMapper,
    /// 未分类日志器
    pub unclassified_logger: Option<UnclassifiedLogger>,
    /// 事件 ID 到事件的映射（缓存）
    pub event_cache: HashMap<String, Event>,
    /// 是否已拟合
    pub fitted: bool,
}

impl EventMatcher {
    /// 创建新的事件匹配器
    pub fn new(config: EventMatcherConfig, category_mapper: CategoryMapper) -> Self {
        let vectorizer = TextVectorizer::new(config.vectorizer_config.clone());
        let kalshi_index = CategoryIndexManager::new(category_mapper.clone());
        let polymarket_index = CategoryIndexManager::new(category_mapper.clone());
        
        Self {
            config,
            vectorizer,
            kalshi_index,
            polymarket_index,
            category_mapper,
            unclassified_logger: None,
            event_cache: HashMap::new(),
            fitted: false,
        }
    }

    /// 使用默认配置创建事件匹配器
    pub fn default() -> Self {
        let temp_mapper = CategoryMapper::from_file("config/categories.toml")
            .unwrap_or_else(|_| CategoryMapper::default());
        Self::new(EventMatcherConfig::default(), temp_mapper)
    }

    /// 设置未分类日志器
    pub fn with_logger(mut self, logger: UnclassifiedLogger) -> Self {
        self.unclassified_logger = Some(logger);
        self
    }

    /// 设置相似度阈值
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.config.similarity_threshold = threshold;
        self
    }

        /// 从事件列表构建 Kalshi 索引（使用已有的向量化器）
    pub fn build_kalshi_index(&mut self, events: &[Event]) -> Result<(), anyhow::Error> {
        if events.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Kalshi 事件索引: 处理 {} 个事件", events.len());
        println!("   📚 使用已有词汇表大小: {}", self.vectorizer.vocab_size());

        // 为每个事件生成向量并分类
        println!("   🔄 生成事件向量并分类...");
        let mut items_to_add = Vec::new();
        
        for event in events {
            // 生成向量
            if let Some(vector) = self.vectorizer.transform(&event.title) {
                // 分类
                let categories = self.category_mapper.classify(&event.title);
                
                // 记录未分类事件
                if categories.is_empty() {
                    if let Some(logger) = &mut self.unclassified_logger {
                        if let Err(e) = logger.log_unclassified(event) {
                            eprintln!("   ⚠️ 记录未分类事件失败: {}", e);
                        }
                    }
                }
                
                let data = Some(serde_json::json!({
                    "title": event.title,
                    "platform": event.platform,
                    "category": event.category,
                    "resolution_date": event.resolution_date.map(|dt| dt.to_rfc3339()),
                }));
                
                items_to_add.push((
                    format!("{}:{}", event.platform, event.event_id),
                    vector,
                    categories,
                    data,
                ));
                
                // 缓存事件
                self.event_cache.insert(
                    format!("{}:{}", event.platform, event.event_id),
                    event.clone()
                );
            } else {
                println!("   ⚠️ 警告: 无法为事件生成向量: {}", event.title);
            }
        }

        // 批量添加到 Kalshi 索引
        println!("   🌲 构建类别索引...");
        if let Err(e) = self.kalshi_index.add_events_batch(items_to_add) {
            eprintln!("   ❌ 构建索引失败: {}", e);
            return Err(anyhow::anyhow!("构建索引失败: {}", e));
        }
        
        self.fitted = true;
        println!("   ✅ 索引构建完成");
        println!("      📊 总事件数: {}", self.kalshi_index.total_size());
        println!("      📊 类别数量: {}", self.kalshi_index.get_all_categories().len());
        
        // ==== 新增：输出 Kalshi 各类别样本 ====
        println!("\n   📋 Kalshi 各类别样本 (最多5个):");
        let categories = self.kalshi_index.get_all_categories();
        for cat in categories.iter().take(10) {  // 只显示前10个类别，避免太多
            let size = self.kalshi_index.category_size(cat);
            println!("      📌 {}: {} 个事件", cat, size);
            
            // 从缓存中找几个该类别的事件示例
            let mut samples = Vec::new();
            for (_, event) in self.event_cache.iter() {
                if event.platform == "kalshi" {
                    let event_cats = self.category_mapper.classify(&event.title);
                    if event_cats.contains(cat) {
                        samples.push(event.title.chars().take(40).collect::<String>());
                        if samples.len() >= 5 {
                            break;
                        }
                    }
                }
            }
            
            for (i, sample) in samples.iter().enumerate() {
                println!("         {}. {}", i+1, sample);
            }
            if samples.is_empty() {
                println!("         无样本");
            }
        }
        // ==== 结束新增 ====
        
        Ok(())
    }

        /// 从事件列表构建 Polymarket 索引（使用已有的向量化器）
    pub fn build_polymarket_index(&mut self, events: &[Event]) -> Result<(), anyhow::Error> {
        if events.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Polymarket 事件索引: 处理 {} 个事件", events.len());
        println!("   📚 使用已有词汇表大小: {}", self.vectorizer.vocab_size());

        // 为每个事件生成向量并分类
        println!("   🔄 生成事件向量并分类...");
        let mut items_to_add = Vec::new();
        
        for event in events {
            // 生成向量
            if let Some(vector) = self.vectorizer.transform(&event.title) {
                // 分类
                let categories = self.category_mapper.classify(&event.title);
                
                // 记录未分类事件
                if categories.is_empty() {
                    if let Some(logger) = &mut self.unclassified_logger {
                        if let Err(e) = logger.log_unclassified(event) {
                            eprintln!("   ⚠️ 记录未分类事件失败: {}", e);
                        }
                    }
                }
                
                let data = Some(serde_json::json!({
                    "title": event.title,
                    "platform": event.platform,
                    "category": event.category,
                    "resolution_date": event.resolution_date.map(|dt| dt.to_rfc3339()),
                }));
                
                items_to_add.push((
                    format!("{}:{}", event.platform, event.event_id),
                    vector,
                    categories,
                    data,
                ));
                
                // 缓存事件
                self.event_cache.insert(
                    format!("{}:{}", event.platform, event.event_id),
                    event.clone()
                );
            } else {
                println!("   ⚠️ 警告: 无法为事件生成向量: {}", event.title);
            }
        }

        // 批量添加到 Polymarket 索引
        println!("   🌲 构建类别索引...");
        if let Err(e) = self.polymarket_index.add_events_batch(items_to_add) {
            eprintln!("   ❌ 构建索引失败: {}", e);
            return Err(anyhow::anyhow!("构建索引失败: {}", e));
        }
        
        self.fitted = true;
        println!("   ✅ 索引构建完成");
        println!("      📊 总事件数: {}", self.polymarket_index.total_size());
        println!("      📊 类别数量: {}", self.polymarket_index.get_all_categories().len());
        
        // ==== 新增：输出 Polymarket 各类别样本 ====
        println!("\n   📋 Polymarket 各类别样本 (最多5个):");
        let categories = self.polymarket_index.get_all_categories();
        for cat in categories.iter().take(10) {  // 只显示前10个类别，避免太多
            let size = self.polymarket_index.category_size(cat);
            println!("      📌 {}: {} 个事件", cat, size);
            
            // 从缓存中找几个该类别的事件示例
            let mut samples = Vec::new();
            for (_, event) in self.event_cache.iter() {
                if event.platform == "polymarket" {
                    let event_cats = self.category_mapper.classify(&event.title);
                    if event_cats.contains(cat) {
                        samples.push(event.title.chars().take(40).collect::<String>());
                        if samples.len() >= 5 {
                            break;
                        }
                    }
                }
            }
            
            for (i, sample) in samples.iter().enumerate() {
                println!("         {}. {}", i+1, sample);
            }
            if samples.is_empty() {
                println!("         无样本");
            }
        }
        // ==== 结束新增 ====
        
        Ok(())
    }

    /// 双向查找匹配的事件对
    pub fn find_matches_bidirectional(
        &self,
        pm_events: &[Event],
        kalshi_events: &[Event],
    ) -> Vec<(Event, Event, f64)> {
        if !self.fitted {
            println!("⚠️ 警告: 索引未构建");
            return Vec::new();
        }

        println!("🔍 开始双向匹配...");
        
        // 方向1: Polymarket 查 Kalshi
        println!("   📌 方向1: Polymarket → Kalshi");
        let matches1 = self.find_matches_directional(pm_events, &self.kalshi_index, "Polymarket", "Kalshi");
        
        // 方向2: Kalshi 查 Polymarket
        println!("   📌 方向2: Kalshi → Polymarket");
        let matches2 = self.find_matches_directional(kalshi_events, &self.polymarket_index, "Kalshi", "Polymarket");
        
        // 合并结果，去重
        let mut all_matches = Vec::new();
        let mut seen_pairs = std::collections::HashSet::new();
        
        for (e1, e2, score) in matches1 {
            let pair_key = format!("{}:{}", e1.event_id, e2.event_id);
            if !seen_pairs.contains(&pair_key) {
                seen_pairs.insert(pair_key);
                all_matches.push((e1, e2, score));
            }
        }
        
        for (e1, e2, score) in matches2 {
            let pair_key = format!("{}:{}", e1.event_id, e2.event_id);
            if !seen_pairs.contains(&pair_key) {
                seen_pairs.insert(pair_key);
                all_matches.push((e1, e2, score));
            }
        }
        
        // 按相似度降序排序
        all_matches.sort_by(|a, b| {
            b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
        });

        println!("   ✅ 双向匹配完成，共找到 {} 个匹配对", all_matches.len());
        all_matches
    }

    /// 单向查找匹配的事件对
    fn find_matches_directional(
        &self,
        query_events: &[Event],
        target_index: &CategoryIndexManager,
        query_platform: &str,
        target_platform: &str,
    ) -> Vec<(Event, Event, f64)> {
        if target_index.total_size() == 0 {
            println!("   ⚠️ {} 索引为空", target_platform);
            return Vec::new();
        }

        println!("      🔍 开始匹配 {} 个 {} 事件", query_events.len(), query_platform);

        let mut all_matches = Vec::new();
        let mut processed = 0;

        for query_event in query_events {
            processed += 1;
            if processed % 100 == 0 {
                println!("         📊 已处理 {}/{}", processed, query_events.len());
            }

            // 为查询事件生成向量
            if let Some(query_vector) = self.vectorizer.transform(&query_event.title) {
                // 获取查询事件的类别
                let query_categories = self.category_mapper.classify(&query_event.title);
                
                // 如果没有类别，只在未分类池中查找
                let categories_to_search = if query_categories.is_empty() {
                    vec!["unclassified".to_string()]
                } else {
                    query_categories
                };
                
                // 在目标索引中查找
                let similar = target_index.find_similar_in_categories(
                    &categories_to_search,
                    &query_vector,
                    self.config.similarity_threshold,
                    5,
                );

                for (item, similarity, _category) in similar {
                    if let Some(target_event) = self.event_cache.get(&item.id) {
                        // 计算最终置信度
                        let confidence = self.calculate_confidence(
                            query_event,
                            target_event,
                            similarity,
                        );

                        if confidence.overall_score >= self.config.similarity_threshold {
                            all_matches.push((
                                query_event.clone(),
                                target_event.clone(),
                                confidence.overall_score,
                            ));
                        }
                    }
                }
            }
        }

        println!("         ✅ 找到 {} 个匹配对", all_matches.len());
        all_matches
    }

    /// 计算最终置信度（向量相似度 + 辅助特征加成）
    fn calculate_confidence(
        &self,
        event1: &Event,
        event2: &Event,
        vector_similarity: f64,
    ) -> MatchConfidence {
        let mut final_score = vector_similarity;

        // 日期匹配加成
        let date_match = if let (Some(d1), Some(d2)) = (event1.resolution_date, event2.resolution_date) {
            let diff = (d1 - d2).num_seconds().abs();
            let match_quality = if diff <= 86400 { 1.0 }  // 1天内
                else if diff <= 604800 { 0.5 }  // 1周内
                else { 0.0 };
            
            if self.config.use_date_boost && match_quality > 0.0 {
                final_score += self.config.date_boost_factor * match_quality;
            }
            
            match_quality > 0.0
        } else {
            false
        };

        // 类别匹配加成
        let category_match = if let (Some(c1), Some(c2)) = (&event1.category, &event2.category) {
            let match_quality = if c1.to_lowercase() == c2.to_lowercase() { 1.0 } else { 0.0 };
            
            if self.config.use_category_boost && match_quality > 0.0 {
                final_score += self.config.category_boost_factor * match_quality;
            }
            
            match_quality > 0.0
        } else {
            false
        };

        // 确保分数不超过 1.0
        final_score = final_score.min(1.0);

        MatchConfidence {
            overall_score: final_score,
            text_similarity: vector_similarity,
            date_match,
            category_match,
        }
    }

    /// 获取向量化器（用于调试）
    pub fn vectorizer(&self) -> &TextVectorizer {
        &self.vectorizer
    }

    /// 获取 Kalshi 索引大小
    pub fn kalshi_index_size(&self) -> usize {
        self.kalshi_index.total_size()
    }

    /// 获取 Polymarket 索引大小
    pub fn polymarket_index_size(&self) -> usize {
        self.polymarket_index.total_size()
    }

    /// 检查是否已拟合
    pub fn is_fitted(&self) -> bool {
        self.fitted
    }

    /// 训练向量化器（统一词汇表）
    pub fn fit_vectorizer(&mut self, events: &[Event]) {
        let titles: Vec<String> = events.iter().map(|e| e.title.clone()).collect();
        self.vectorizer.fit(&titles);
        println!("📚 向量化器训练完成，词汇表大小: {}", self.vectorizer.vocab_size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Utc, TimeZone};

    fn create_test_event(id: &str, title: &str, platform: &str, days_from_now: i64) -> Event {
        let date = Utc::now() + chrono::Duration::days(days_from_now);
        
        Event {
            platform: platform.to_string(),
            event_id: id.to_string(),
            title: title.to_string(),
            description: String::new(),
            resolution_date: Some(date),
            category: Some("politics".to_string()),
            tags: Vec::new(),
            slug: None,
            token_ids: Vec::new(),
            outcome_prices: None,
            best_ask: None,
            best_bid: None,
            last_trade_price: None,
            vector_cache: None,
            categories: Vec::new(),
        }
    }

    #[test]
    fn test_bidirectional_match() {
        let temp_mapper = CategoryMapper::default();
        let mut matcher = EventMatcher::new(EventMatcherConfig::default(), temp_mapper);
        
        // 创建测试事件
        let kalshi_events = vec![
            create_test_event("k1", "Who will win the 2024 US Presidential Election?", "kalshi", 100),
            create_test_event("k2", "Will Bitcoin reach $100,000 in 2024?", "kalshi", 50),
        ];
        
        let pm_events = vec![
            create_test_event("p1", "Presidential Election Winner 2024", "polymarket", 100),
            create_test_event("p2", "Bitcoin $100K in 2024", "polymarket", 50),
        ];
        
        // 先训练统一的向量化器
        let all_events: Vec<Event> = kalshi_events.iter().chain(pm_events.iter()).cloned().collect();
        matcher.fit_vectorizer(&all_events);
        
        // 构建双索引
        matcher.build_kalshi_index(&kalshi_events).unwrap();
        matcher.build_polymarket_index(&pm_events).unwrap();
        
        // 双向匹配
        let matches = matcher.find_matches_bidirectional(&pm_events, &kalshi_events);
        
        assert!(!matches.is_empty());
    }
}