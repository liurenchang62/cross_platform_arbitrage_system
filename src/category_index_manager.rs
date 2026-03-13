// src/category_index_manager.rs
//! 类别索引管理器：按类别管理多个 VectorIndex

use std::collections::HashMap;
use ndarray::Array1;
use anyhow::Result;

use crate::vector_index::{VectorIndex, IndexItem};

/// 类别索引管理器
pub struct CategoryIndexManager {
    /// 类别名称到索引的映射
    indices: HashMap<String, VectorIndex>,
    /// 未分类索引
    unclassified_index: VectorIndex,
}

impl CategoryIndexManager {
    /// 创建新的类别索引管理器
    pub fn new() -> Self {
        let unclassified_index = VectorIndex::default("unclassified".to_string());
        
        Self {
            indices: HashMap::new(),
            unclassified_index,
        }
    }
    
    /// 获取或创建类别索引
    fn get_or_create_index(&mut self, category: &str) -> &mut VectorIndex {
        if category == "unclassified" {
            return &mut self.unclassified_index;
        }
        
        self.indices.entry(category.to_string())
            .or_insert_with(|| VectorIndex::default(category.to_string()))
    }
    
    /// 添加事件到对应的类别索引
    pub fn add_event(&mut self, event_id: String, vector: Array1<f64>, categories: Vec<String>, data: Option<serde_json::Value>) -> Result<()> {
        let item = IndexItem {
            id: event_id,
            vector,
            data,
        };
        
        if categories.is_empty() {
            self.unclassified_index.insert(item)?;
        } else {
            for category in categories {
                let index = self.get_or_create_index(&category);
                let _ = index.insert(item.clone());
            }
        }
        
        Ok(())
    }
    
    /// 批量添加事件
    pub fn add_events_batch(&mut self, items: Vec<(String, Array1<f64>, Vec<String>, Option<serde_json::Value>)>) -> Result<()> {
        for (id, vector, categories, data) in items {
            let _ = self.add_event(id, vector, categories, data);
        }
        Ok(())
    }
    
    /// 清理所有索引
    pub fn clear(&mut self) {
        self.indices.clear();
        self.unclassified_index.clear();
    }
    
    /// 在多个类别中查找相似事件
    pub fn find_similar_in_categories(
        &self,
        categories: &[String],
        query_vector: &Array1<f64>,
        threshold: f64,
        max_results: usize,
    ) -> Vec<(IndexItem, f64, String)> {
        let mut all_results = Vec::new();
        
        for category in categories {
            let index = if category == "unclassified" {
                &self.unclassified_index
            } else if let Some(idx) = self.indices.get(category) {
                idx
            } else {
                continue;
            };
            
            let results = index.find_similar_with_threshold(query_vector, threshold, max_results);
            
            for (item, score) in results {
                all_results.push((item, score, category.clone()));
            }
        }
        
        all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(max_results);
        
        all_results
    }
    
    /// 获取类别索引大小
    pub fn category_size(&self, category: &str) -> usize {
        if category == "unclassified" {
            self.unclassified_index.len()
        } else {
            self.indices.get(category).map(|i| i.len()).unwrap_or(0)
        }
    }
    
    /// 获取所有索引的总大小
    pub fn total_size(&self) -> usize {
        let mut total = self.unclassified_index.len();
        for index in self.indices.values() {
            total += index.len();
        }
        total
    }
}