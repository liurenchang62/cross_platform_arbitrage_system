// src/category_index_manager.rs
//! 类别索引管理器：按类别管理多个 VectorIndex

use std::collections::HashMap;
use ndarray::Array1;
use anyhow::{Result, anyhow};

use crate::vector_index::{VectorIndex, IndexItem};
use crate::category_mapper::CategoryMapper;

/// 类别索引管理器
pub struct CategoryIndexManager {
    /// 类别名称到索引的映射
    indices: HashMap<String, VectorIndex>,
    /// 类别映射器（用于获取所有类别）
    mapper: CategoryMapper,
    /// 未分类索引（特殊类别）
    unclassified_index: VectorIndex,
}

impl CategoryIndexManager {
    /// 创建新的类别索引管理器
    pub fn new(mapper: CategoryMapper) -> Self {
        let unclassified_index = VectorIndex::default("unclassified".to_string());
        
        Self {
            indices: HashMap::new(),
            mapper,
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
            // 无类别：加入未分类索引
            if let Err(e) = self.unclassified_index.insert(item) {
                return Err(anyhow::anyhow!("插入未分类索引失败: {:?}", e));
            }
        } else {
            // 有类别：加入每个类别对应的索引
            for category in categories {
                let index = self.get_or_create_index(&category);
                if let Err(e) = index.insert(item.clone()) {
                    eprintln!("   ⚠️ 插入类别 {} 索引失败: {:?}, 跳过此事件", category, e);
                    // 继续执行，不中断
                }
            }
        }
        
        Ok(())
    }
    
    /// 批量添加事件
    pub fn add_events_batch(&mut self, items: Vec<(String, Array1<f64>, Vec<String>, Option<serde_json::Value>)>) -> Result<()> {
        let mut success_count = 0;
        let mut fail_count = 0;
        
        for (id, vector, categories, data) in items {
            match self.add_event(id, vector, categories, data) {
                Ok(_) => success_count += 1,
                Err(e) => {
                    fail_count += 1;
                    if fail_count <= 5 {
                        eprintln!("   ⚠️ 添加事件失败: {}", e);
                    }
                }
            }
        }
        
        if fail_count > 0 {
            println!("   ⚠️ 批量添加完成: 成功 {} 个, 失败 {} 个", success_count, fail_count);
        }
        
        Ok(())
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
            let results = if category == "unclassified" {
                if self.unclassified_index.len() > 0 {
                    self.unclassified_index.find_similar_with_threshold(query_vector, threshold, max_results)
                } else {
                    Vec::new()
                }
            } else if let Some(index) = self.indices.get(category) {
                if index.len() > 0 {
                    index.find_similar_with_threshold(query_vector, threshold, max_results)
                } else {
                    Vec::new()
                }
            } else {
                // 只在调试时打印，减少噪音
                // eprintln!("      ⚠️ [DEBUG] 类别不存在: {}", category);
                continue;
            };
            
            for (item, score) in results {
                all_results.push((item, score, category.clone()));
            }
        }
        
        // 按相似度排序
        all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(max_results);
        
        all_results
    }
    
    /// 获取所有类别名称
    pub fn get_all_categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self.indices.keys().cloned().collect();
        cats.push("unclassified".to_string());
        cats.sort();
        cats
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
    
    /// 清理所有索引
    pub fn clear(&mut self) {
        self.indices.clear();
        self.unclassified_index.clear();
    }
}