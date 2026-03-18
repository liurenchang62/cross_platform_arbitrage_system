// src/category_vectorizer.rs
//! 类别独立的向量化器管理

use std::collections::HashMap;
use anyhow::Result;
use serde_json::Value;

use crate::text_vectorizer::{TextVectorizer, VectorizerConfig};
use crate::vector_index::{VectorIndex, IndexItem};  // 直接从 vector_index 导入

/// 类别向量化器
pub struct CategoryVectorizer {
    /// 类别名称
    pub category: String,
    /// 文本向量化器
    pub vectorizer: TextVectorizer,
    /// 向量索引
    pub index: VectorIndex,
    /// 是否已拟合
    pub fitted: bool,
}

impl CategoryVectorizer {
    /// 创建新的类别向量化器
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
    
    /// 拟合该类别的文档
    pub fn fit(&mut self, titles: &[String]) {
        if titles.is_empty() {
            return;
        }
        self.vectorizer.fit(titles);
        self.fitted = true;
        println!("      类别 '{}' 词汇表大小: {}", self.category, self.vectorizer.vocab_size());
    }
    
    /// 添加市场到索引
    pub fn add_market(&mut self, market_id: String, title: &str, data: Option<Value>) -> Result<()> {
        if !self.fitted {
            return Ok(());
        }
        
        if let Some(vector) = self.vectorizer.transform(title) {
            let item = IndexItem {
                id: market_id,
                vector,
                data,
            };
            self.index.insert(item)?;
        }
        
        Ok(())
    }
    
    /// 批量添加市场
    pub fn add_markets_batch(&mut self, items: Vec<(String, String, Option<Value>)>) -> Result<()> {
        if !self.fitted {
            return Ok(());
        }
        
        let mut index_items = Vec::new();
        let total = items.len();
        
        for (i, (market_id, title, data)) in items.into_iter().enumerate() {
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
            println!("          构建 K-D Tree ({} 个点)...", index_items.len());
            self.index.build(index_items)?;
        }
        
        Ok(())
    }
    
    /// 查找相似市场
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
    /// 类别名称到向量化器的映射
    vectorizers: HashMap<String, CategoryVectorizer>,
    /// 未分类向量化器
    unclassified_vectorizer: CategoryVectorizer,
}

impl CategoryVectorizerManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        Self {
            vectorizers: HashMap::new(),
            unclassified_vectorizer: CategoryVectorizer::new("unclassified".to_string()),
        }
    }
    
    /// 获取或创建类别向量化器
    pub fn get_or_create(&mut self, category: &str) -> Option<&mut CategoryVectorizer> {
        if category == "unclassified" {
            return Some(&mut self.unclassified_vectorizer);
        }
        
        Some(self.vectorizers
            .entry(category.to_string())
            .or_insert_with(|| CategoryVectorizer::new(category.to_string())))
    }
    
    /// 获取类别向量化器（不可变）
    pub fn get(&self, category: &str) -> Option<&CategoryVectorizer> {
        if category == "unclassified" {
            Some(&self.unclassified_vectorizer)
        } else {
            self.vectorizers.get(category)
        }
    }
    
    /// 拟合所有类别的文档
    pub fn fit_all(&mut self, markets_by_category: HashMap<String, Vec<String>>) {
        let total = markets_by_category.len();
        let mut processed = 0;
        
        for (category, titles) in markets_by_category {
            processed += 1;
            if processed % 5 == 0 {
                println!("      拟合进度: {}/{} 个类别", processed, total);
            }
            
            if let Some(vectorizer) = self.get_or_create(&category) {
                vectorizer.fit(&titles);
            }
        }
    }
    
    /// 获取所有类别名称
    pub fn get_all_categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self.vectorizers.keys().cloned().collect();
        cats.push("unclassified".to_string());
        cats.sort();
        cats
    }
    
    /// 获取类别大小
    pub fn category_size(&self, category: &str) -> usize {
        if let Some(vec) = self.get(category) {
            vec.index.len()
        } else {
            0
        }
    }
    
    /// 获取总大小
    pub fn total_size(&self) -> usize {
        let mut total = self.unclassified_vectorizer.index.len();
        for vec in self.vectorizers.values() {
            total += vec.index.len();
        }
        total
    }
    
    /// 清理所有索引
    pub fn clear(&mut self) {
        self.vectorizers.clear();
        self.unclassified_vectorizer = CategoryVectorizer::new("unclassified".to_string());
    }
}