// src/category_vectorizer.rs
//! 类别独立的向量化器管理

use std::collections::HashMap;
use anyhow::Result;
use serde_json::Value;

use crate::text_vectorizer::{TextVectorizer, VectorizerConfig};
use crate::vector_index::{VectorIndex, IndexItem};

/// 类别向量化器
pub struct CategoryVectorizer {
    pub category: String,
    pub vectorizer: TextVectorizer,
    pub index: VectorIndex,
    pub fitted: bool,
}

impl CategoryVectorizer {
    pub fn new(category: String) -> Self {
        let vectorizer = TextVectorizer::new(VectorizerConfig::default());
        let index = VectorIndex::default(category.clone());
        
        Self {
            category,
            vectorizer,
            index,
            fitted: false,
        }
    }
    
    pub fn fit(&mut self, titles: &[String]) {
        if titles.is_empty() {
            return;
        }
        self.vectorizer.fit(titles);
        self.fitted = true;
        // 只输出词汇表大小，不输出每个类别
    }
    
    pub fn add_markets_batch(&mut self, items: Vec<(String, String, Option<Value>)>) -> Result<()> {
        if !self.fitted {
            return Ok(());
        }
        
        let mut index_items = Vec::new();
        let total = items.len();
        
        for (i, (market_id, title, data)) in items.into_iter().enumerate() {
            // 每5000个输出一次进度
            if i % 5000 == 0 && i > 0 {
                println!("          构建索引: {}/{}", i, total);
            }
            
            if let Some(vector) = self.vectorizer.transform(&title) {
                index_items.push(IndexItem {
                    id: market_id,
                    vector,
                    data,
                });
            }
        }
        
        if !index_items.is_empty() {
            if total > 1000 {
                println!("          构建 K-D Tree ({} 个点)...", index_items.len());
            }
            self.index.build(index_items)?;
        }
        
        Ok(())
    }
    
    pub fn find_similar(
        &self,
        title: &str,
        threshold: f64,
        max_results: usize,
    ) -> Vec<(IndexItem, f64)> {
        if !self.fitted {
            return Vec::new();
        }
        
        if let Some(query_vector) = self.vectorizer.transform(title) {
            self.index.find_similar_with_threshold(&query_vector, threshold, max_results)
        } else {
            Vec::new()
        }
    }
}

/// 类别向量化器管理器
pub struct CategoryVectorizerManager {
    vectorizers: HashMap<String, CategoryVectorizer>,
    unclassified_vectorizer: CategoryVectorizer,
}

impl CategoryVectorizerManager {
    pub fn new() -> Self {
        Self {
            vectorizers: HashMap::new(),
            unclassified_vectorizer: CategoryVectorizer::new("unclassified".to_string()),
        }
    }
    
    pub fn get_or_create(&mut self, category: &str) -> Option<&mut CategoryVectorizer> {
        if category == "unclassified" {
            return Some(&mut self.unclassified_vectorizer);
        }
        
        Some(self.vectorizers
            .entry(category.to_string())
            .or_insert_with(|| CategoryVectorizer::new(category.to_string())))
    }
    
    pub fn get(&self, category: &str) -> Option<&CategoryVectorizer> {
        if category == "unclassified" {
            Some(&self.unclassified_vectorizer)
        } else {
            self.vectorizers.get(category)
        }
    }
    
    pub fn fit_all(&mut self, markets_by_category: HashMap<String, Vec<String>>) {
        let total = markets_by_category.len();
        let mut processed = 0;
        
        for (category, titles) in markets_by_category {
            processed += 1;
            // 每5个类别输出一次进度
            if processed % 5 == 0 || processed == 1 {
                println!("      拟合进度: {}/{} 个类别", processed, total);
            }
            
            if let Some(vectorizer) = self.get_or_create(&category) {
                vectorizer.fit(&titles);
            }
        }
    }
    
    pub fn get_all_categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self.vectorizers.keys().cloned().collect();
        cats.push("unclassified".to_string());
        cats.sort();
        cats
    }
    
    pub fn category_size(&self, category: &str) -> usize {
        if let Some(vec) = self.get(category) {
            vec.index.len()
        } else {
            0
        }
    }
    
    pub fn total_size(&self) -> usize {
        let mut total = self.unclassified_vectorizer.index.len();
        for vec in self.vectorizers.values() {
            total += vec.index.len();
        }
        total
    }
    
    pub fn clear(&mut self) {
        self.vectorizers.clear();
        self.unclassified_vectorizer = CategoryVectorizer::new("unclassified".to_string());
    }
}