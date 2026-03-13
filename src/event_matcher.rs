// src/event_matcher.rs
//! 事件匹配器，使用 TF-IDF + K-D Tree 实现快速准确的事件匹配

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

/// 事件匹配器配置
#[derive(Debug, Clone)]
pub struct EventMatcherConfig {
    pub similarity_threshold: f64,
    pub vectorizer_config: VectorizerConfig,
    pub use_date_boost: bool,
    pub use_category_boost: bool,
    pub date_boost_factor: f64,
    pub category_boost_factor: f64,
}

impl Default for EventMatcherConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.5,
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
    pub config: EventMatcherConfig,
    pub vectorizer: TextVectorizer,
    pub kalshi_index: CategoryIndexManager,
    pub polymarket_index: CategoryIndexManager,
    pub category_mapper: CategoryMapper,
    pub unclassified_logger: Option<UnclassifiedLogger>,
    pub fitted: bool,
}

impl EventMatcher {
    pub fn new(config: EventMatcherConfig, category_mapper: CategoryMapper) -> Self {
        let vectorizer = TextVectorizer::new(config.vectorizer_config.clone());
        let kalshi_index = CategoryIndexManager::new();  // 去掉参数
        let polymarket_index = CategoryIndexManager::new();  // 去掉参数
        
        Self {
            config,
            vectorizer,
            kalshi_index,
            polymarket_index,
            category_mapper,
            unclassified_logger: None,
            fitted: false,
        }
    }

    pub fn default() -> Self {
        let temp_mapper = CategoryMapper::from_file("config/categories.toml")
            .unwrap_or_else(|_| CategoryMapper::default());
        Self::new(EventMatcherConfig::default(), temp_mapper)
    }

    pub fn with_logger(mut self, logger: UnclassifiedLogger) -> Self {
        self.unclassified_logger = Some(logger);
        self
    }

    pub fn fit_vectorizer(&mut self, events: &[Event]) {
        let titles: Vec<String> = events.iter().map(|e| e.title.clone()).collect();
        self.vectorizer.fit(&titles);
        println!("📚 向量化器训练完成，词汇表大小: {}", self.vectorizer.vocab_size());
    }

    pub fn build_kalshi_index(&mut self, events: &[Event]) -> Result<(), anyhow::Error> {
        if events.is_empty() {
            return Ok(());
        }

        println!("📊 构建 Kalshi 事件索引: 处理 {} 个事件", events.len());
        println!("   📚 使用已有词汇表大小: {}", self.vectorizer.vocab_size());

        let mut items_to_add = Vec::new();
        let mut category_count: HashMap<String, usize> = HashMap::new();
        
        // ==== 调试2: 记录前20个事件的event_id，看进入索引的是哪些 ====
        let mut index_count = 0;
        // ==== 结束调试2 ====
        
        for event in events {
            if let Some(vector) = self.vectorizer.transform(&event.title) {
                let categories = self.category_mapper.classify(&event.title);
                
                // ==== 调试2续: 输出前20个进入索引的事件 ====
                if index_count < 20 {
                    println!("📊 [构建索引] event_id={}, 类别={:?}, 标题={}", 
                        event.event_id,
                        categories,
                        event.title.chars().take(30).collect::<String>()
                    );
                    index_count += 1;
                }
                // ==== 结束调试2续 ====
                
                for cat in &categories {
                    *category_count.entry(cat.clone()).or_insert(0) += 1;
                }
                
                let data = Some(serde_json::json!({
                    "title": event.title,
                    "platform": event.platform,
                }));
                
                items_to_add.push((
                    format!("{}:{}", event.platform, event.event_id),
                    vector,
                    categories,
                    data,
                ));
            }
        }

        println!("   📊 生成 {} 个待添加项", items_to_add.len());
        self.kalshi_index.add_events_batch(items_to_add)?;
        self.fitted = true;
        
        println!("   ✅ Kalshi 索引构建完成，总事件数: {}", self.kalshi_index.total_size());
        Ok(())
    }

    pub fn build_polymarket_index(&mut self, events: &[Event]) -> Result<(), anyhow::Error> {
        if events.is_empty() {
            return Ok(());
        }

        println!("\n📊 构建 Polymarket 事件索引: 处理 {} 个事件", events.len());
        println!("   📚 使用已有词汇表大小: {}", self.vectorizer.vocab_size());

        let mut items_to_add = Vec::new();
        let mut category_count: HashMap<String, usize> = HashMap::new();
        
        for event in events {
            if let Some(vector) = self.vectorizer.transform(&event.title) {
                let categories = self.category_mapper.classify(&event.title);
                
                for cat in &categories {
                    *category_count.entry(cat.clone()).or_insert(0) += 1;
                }
                
                let data = Some(serde_json::json!({
                    "title": event.title,
                    "platform": event.platform,
                }));
                
                items_to_add.push((
                    format!("{}:{}", event.platform, event.event_id),
                    vector,
                    categories,
                    data,
                ));
                

            }
        }

        println!("   📊 生成 {} 个待添加项", items_to_add.len());
        println!("   📊 类别分布:");
        let mut cats: Vec<_> = category_count.into_iter().collect();
        cats.sort_by(|a, b| b.1.cmp(&a.1));
        for (cat, count) in cats.iter().take(10) {
            println!("      - {}: {} 个", cat, count);
        }

        self.polymarket_index.add_events_batch(items_to_add)?;
        
        println!("   ✅ Polymarket 索引构建完成，总事件数: {}", self.polymarket_index.total_size());
        Ok(())
    }

    

    pub fn find_matches_bidirectional(
        &self,
        pm_events: &[Event],
        kalshi_events: &[Event],
    ) -> Vec<(Event, Event, f64)> {
        if !self.fitted {
            println!("⚠️ 索引未构建");
            return Vec::new();
        }

        println!("\n🔍 ====== 开始双向匹配 ======");
        
        println!("\n📊 共同类别检查:");
        let check_cats = vec![
            "politics_us", "politics_uk", "gaming", "crypto", "business",
            "entertainment_movies", "entertainment_music", "sports_basketball"
        ];
        
        for cat in &check_cats {
            let k_size = self.kalshi_index.category_size(cat);
            let p_size = self.polymarket_index.category_size(cat);
            if k_size > 0 && p_size > 0 {
                println!("   ✅ {}: Kalshi {} 个, Polymarket {} 个", cat, k_size, p_size);
            }
        }
        
        println!("\n   📌 方向1: Polymarket → Kalshi");
        let matches1 = self.find_matches_directional(pm_events, kalshi_events, &self.kalshi_index);
        
        println!("\n   📌 方向2: Kalshi → Polymarket");
        let matches2 = self.find_matches_directional(kalshi_events, pm_events, &self.polymarket_index);
        
        let mut all_matches = Vec::new();
        let mut seen_pairs = std::collections::HashSet::new();
        
        // 处理方向1的匹配 (Polymarket → Kalshi) - 已经是正确的顺序
        for (e1, e2, score) in matches1 {
            let pair_key = format!("{}:{}", e1.event_id, e2.event_id);
            let reverse_key = format!("{}:{}", e2.event_id, e1.event_id);
            
            // 确保没出现过，也没出现过反向的
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                // 确保顺序是 (PM, Kalshi)
                if e1.platform == "polymarket" && e2.platform == "kalshi" {
                    all_matches.push((e1, e2, score));
                } else {
                    // 如果顺序反了，交换回来
                    all_matches.push((e2, e1, score));
                }
            }
        }
        
        // 处理方向2的匹配 (Kalshi → Polymarket) - 需要反转顺序
        for (e1, e2, score) in matches2 {
            // 在方向2中，e1是Kalshi，e2是Polymarket
            let pair_key = format!("{}:{}", e2.event_id, e1.event_id);  // 交换成 (PM, Kalshi) 的key
            let reverse_key = format!("{}:{}", e1.event_id, e2.event_id);
            
            if !seen_pairs.contains(&pair_key) && !seen_pairs.contains(&reverse_key) {
                seen_pairs.insert(pair_key);
                // 直接以 (PM, Kalshi) 的顺序存入
                all_matches.push((e2, e1, score));
            }
        }
        
        // 按相似度降序排序
        all_matches.sort_by(|a, b| {
            b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
        });

        println!("\n📊 ====== 匹配完成 ======");
        println!("   共找到 {} 个匹配对", all_matches.len());
        
        all_matches
    }

    fn find_matches_directional(
        &self,
        query_events: &[Event],
        target_events: &[Event],
        target_index: &CategoryIndexManager,
    ) -> Vec<(Event, Event, f64)> {
        let mut all_matches = Vec::new();
        let mut total_queries = 0;

        for query_event in query_events {
            if let Some(query_vector) = self.vectorizer.transform(&query_event.title) {
                let query_categories = self.category_mapper.classify(&query_event.title);
                
                if query_categories.is_empty() {
                    continue;
                }
                
                total_queries += 1;
                
                let similar = target_index.find_similar_in_categories(
                    &query_categories,
                    &query_vector,
                    self.config.similarity_threshold,
                    5,
                );

                for (item, similarity, _category) in similar {
                    // 从传入的 target_events 中查找匹配的事件
                    if let Some(target_event) = target_events.iter().find(|e| {
                        format!("{}:{}", e.platform, e.event_id) == item.id
                    }) {
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

        if total_queries > 0 {
            println!("      📊 查询事件: {}, 有类别事件: {}, 匹配: {}", 
                query_events.len(), total_queries, all_matches.len());
        }
        
        all_matches
    }

    fn calculate_confidence(
        &self,
        event1: &Event,
        event2: &Event,
        vector_similarity: f64,
    ) -> MatchConfidence {
        let mut final_score = vector_similarity;

        let date_match = if let (Some(d1), Some(d2)) = (event1.resolution_date, event2.resolution_date) {
            let diff = (d1 - d2).num_seconds().abs();
            let match_quality = if diff <= 86400 { 1.0 } else { 0.0 };
            
            if self.config.use_date_boost {
                final_score += self.config.date_boost_factor * match_quality;
            }
            match_quality > 0.0
        } else {
            false
        };

        let category_match = if let (Some(c1), Some(c2)) = (&event1.category, &event2.category) {
            let match_quality = if c1.to_lowercase() == c2.to_lowercase() { 1.0 } else { 0.0 };
            
            if self.config.use_category_boost {
                final_score += self.config.category_boost_factor * match_quality;
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

    pub fn vectorizer(&self) -> &TextVectorizer {
        &self.vectorizer
    }

    pub fn kalshi_index_size(&self) -> usize {
        self.kalshi_index.total_size()
    }

    pub fn polymarket_index_size(&self) -> usize {
        self.polymarket_index.total_size()
    }
}