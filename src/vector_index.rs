// src/vector_index.rs
//! 向量索引模块，使用 K-D Tree 实现近似最近邻搜索

use std::collections::HashMap;
use kdtree::KdTree;
use ndarray::Array1;

/// K-D Tree 维度（向量维度）
const TREE_DIMENSION: usize = 100;

/// 向量索引项
#[derive(Debug, Clone)]
pub struct IndexItem {
    /// 唯一标识符（如事件ID）
    pub id: String,
    /// 原始向量
    pub vector: Array1<f64>,
    /// 附加数据（可选）
    pub data: Option<serde_json::Value>,
}

/// 向量索引
pub struct VectorIndex {
    /// 类别名称
    pub category_name: String,
    /// K-D Tree 索引
    tree: KdTree<f64, usize, Vec<f64>>,
    /// ID 到索引的映射
    id_to_idx: HashMap<String, usize>,
    /// 索引到项的映射
    items: Vec<IndexItem>,
    /// 向量维度
    dimension: usize,
    /// 是否已构建
    built: bool,
}

impl VectorIndex {
    /// 创建新的空索引（指定类别）
    pub fn new(category_name: String, dimension: usize) -> Self {
        Self {
            category_name,
            tree: KdTree::new(dimension),
            id_to_idx: HashMap::new(),
            items: Vec::new(),
            dimension,
            built: false,
        }
    }

    /// 创建默认维度的索引
    pub fn default(category_name: String) -> Self {
        Self::new(category_name, TREE_DIMENSION)
    }

    /// 从向量列表构建索引
    pub fn build(&mut self, items: Vec<IndexItem>) -> Result<(), kdtree::ErrorKind> {
        if items.is_empty() {
            return Ok(());
        }

        self.clear();
        
        for item in items {
            if let Err(e) = self.insert(item) {
                eprintln!("   ⚠️ 插入失败: {:?}", e);
            }
        }

        self.built = true;
        Ok(())
    }

    /// 插入单个向量到索引
    pub fn insert(&mut self, item: IndexItem) -> Result<(), kdtree::ErrorKind> {
        if self.items.is_empty() {
            self.dimension = item.vector.len();
            self.tree = KdTree::new(self.dimension);
        }
        
        if item.vector.len() != self.dimension {
            return Ok(());
        }

        let idx = self.items.len();
        self.id_to_idx.insert(item.id.clone(), idx);
        self.items.push(item);

        let point: Vec<f64> = self.items[idx].vector.iter().cloned().collect();
        self.tree.add(point, idx)?;
        
        if !self.built && !self.items.is_empty() {
            self.built = true;
        }

        Ok(())
    }

    /// 查找超过相似度阈值的所有向量
    pub fn find_similar_with_threshold(
        &self,
        query_vector: &Array1<f64>,
        threshold: f64,
        max_results: usize,
    ) -> Vec<(IndexItem, f64)> {
        if !self.built || self.items.is_empty() {
            return Vec::new();
        }

        if query_vector.len() != self.dimension {
            return Vec::new();
        }

        let dist_sq_threshold = 2.0 * (1.0 - threshold);
        let query_point: Vec<f64> = query_vector.iter().cloned().collect();

        let result = self.tree.within(&query_point, dist_sq_threshold, &kdtree::distance::squared_euclidean);

        match result {
            Ok(neighbors) => {
                let original_len = neighbors.len();
                
                let mut results: Vec<_> = neighbors
                    .into_iter()
                    .map(|(dist_sq, idx)| {
                        let item = self.items[*idx].clone();
                        let similarity = 1.0 - (dist_sq / 2.0).min(1.0).max(0.0);
                        (item, similarity)
                    })
                    .collect();
                
                results.retain(|(_, sim)| *sim >= threshold);
                
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                results.truncate(max_results);
                
                results
            }
            Err(_) => Vec::new(),
        }
    }

    /// 获取索引大小
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 检查索引是否为空
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 清理索引
    pub fn clear(&mut self) {
        self.tree = KdTree::new(self.dimension);
        self.id_to_idx.clear();
        self.items.clear();
        self.built = false;
    }
}