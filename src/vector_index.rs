// vector_index.rs
//! 向量索引模块，使用 K-D Tree 实现近似最近邻搜索
//! 
//! 负责存储向量并快速查找相似向量

use std::collections::HashMap;
use kdtree::KdTree;
use ndarray::Array1;

/// K-D Tree 维度（向量维度）
const TREE_DIMENSION: usize = 100;  // 默认维度，实际会在构建时根据向量调整

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
/// 向量索引
pub struct VectorIndex {
    /// 类别名称
    category_name: String,
    /// K-D Tree 索引（使用 Vec<f64> 作为点，usize 作为索引）
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
    /// 创建新的空索引
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

    /// 创建默认维度的索引（实际会在构建时调整）
    pub fn default(category_name: String) -> Self {
        Self::new(category_name, TREE_DIMENSION)
    }

    /// 从向量列表构建索引
        /// 从向量列表构建索引
    pub fn build(&mut self, items: Vec<IndexItem>) -> Result<(), kdtree::ErrorKind> {
        if items.is_empty() {
            return Ok(());
        }

        // 清空现有索引
        self.clear();
        
        // 逐个添加，让 insert 处理维度
        for item in items {
            self.insert(item)?;
        }

        self.built = true;
        Ok(())
    }

    /// 插入单个向量到索引
        /// 插入单个向量到索引
    pub fn insert(&mut self, item: IndexItem) -> Result<(), kdtree::ErrorKind> {
        // 如果索引为空，直接设置维度并添加
        if self.items.is_empty() {
            println!("🔧 [DEBUG] 索引为空，设置维度为: {}", item.vector.len());
            self.dimension = item.vector.len();
            self.tree = KdTree::new(self.dimension);
        }
        
        // 检查维度
        if item.vector.len() != self.dimension {
            println!("⚠️ [DEBUG] 维度不匹配: 事件 '{}' 的向量维度={}, 索引维度={}, 已跳过", 
                item.id, item.vector.len(), self.dimension);
            return Ok(()); // 跳过而不是报错
        }

        let idx = self.items.len();
        self.id_to_idx.insert(item.id.clone(), idx);
        self.items.push(item);

        let point: Vec<f64> = self.items[idx].vector.iter().cloned().collect();
        if let Err(e) = self.tree.add(point, idx) {
            println!("❌ [DEBUG] 添加到K-D树失败: {:?}", e);
            return Err(e);
        }

        if idx % 100 == 0 {
            println!("   ✅ [DEBUG] 已插入 {} 个向量到索引 '{}'", idx + 1, self.category_name);
        }

        Ok(())
    }

    /// 查找最相似的 k 个向量
    pub fn find_similar(
        &self,
        query_vector: &Array1<f64>,
        k: usize,
    ) -> Vec<(IndexItem, f64)> {
        if !self.built || self.items.is_empty() {
            return Vec::new();
        }

        // 检查维度
        if query_vector.len() != self.dimension {
            eprintln!("查询向量维度 {} 不匹配索引维度 {}", query_vector.len(), self.dimension);
            return Vec::new();
        }

        let query_point: Vec<f64> = query_vector.iter().cloned().collect();

        // 使用 K-D Tree 查找最近邻（欧氏距离）
        let result = self.tree.nearest(&query_point, k, &kdtree::distance::squared_euclidean);

        match result {
            Ok(neighbors) => {
                neighbors
                    .into_iter()
                    .map(|(dist_sq, idx)| {
                        let item = self.items[*idx].clone();
                        // 将欧氏距离转换为余弦相似度
                        let similarity = 1.0 - (dist_sq / 2.0).min(1.0).max(0.0);
                        (item, similarity)
                    })
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    }

    /// 查找超过相似度阈值的所有向量
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

        // 检查维度
        if query_vector.len() != self.dimension {
            println!("      ⚠️ [DEBUG] 查询维度 {} 不匹配索引维度 {}, 无法查询",
                query_vector.len(), self.dimension);
            return Vec::new();
        }

        // 对于归一化向量，余弦相似度阈值转换为欧氏距离阈值
        let dist_sq_threshold = 2.0 * (1.0 - threshold);

        let query_point: Vec<f64> = query_vector.iter().cloned().collect();

        // 使用 K-D Tree 查找半径内的所有点
        let result = self.tree.within(&query_point, dist_sq_threshold, &kdtree::distance::squared_euclidean);

        match result {
            Ok(neighbors) => {
                let mut results: Vec<_> = neighbors
                    .into_iter()
                    .map(|(dist_sq, idx)| {
                        let item = self.items[*idx].clone();
                        let similarity = 1.0 - (dist_sq / 2.0).min(1.0).max(0.0);
                        (item, similarity)
                    })
                    .filter(|(_, sim)| *sim >= threshold)
                    .collect();

                // 按相似度降序排序
                results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                // 限制结果数量
                if results.len() > max_results {
                    results.truncate(max_results);
                }

                results
            }
            Err(e) => {
                println!("      ⚠️ [DEBUG] K-D树查询失败: {:?}", e);
                Vec::new()
            }
        }
    }

    /// 通过 ID 获取向量项
    pub fn get_by_id(&self, id: &str) -> Option<&IndexItem> {
        self.id_to_idx.get(id).and_then(|&idx| self.items.get(idx))
    }

    /// 获取所有项
    pub fn items(&self) -> &Vec<IndexItem> {
        &self.items
    }

    /// 获取索引大小
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 检查索引是否为空
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 检查是否已构建
    pub fn is_built(&self) -> bool {
        self.built
    }

    /// 获取向量维度
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// 清理索引（重建）
    pub fn clear(&mut self) {
        self.tree = KdTree::new(self.dimension);
        self.id_to_idx.clear();
        self.items.clear();
        self.built = false;
    }
}

/// 简化的批处理查找函数
pub fn batch_find_similar(
    index: &VectorIndex,
    query_vectors: &[Array1<f64>],
    threshold: f64,
    max_results_per_query: usize,
) -> Vec<Vec<(IndexItem, f64)>> {
    query_vectors
        .iter()
        .map(|vec| index.find_similar_with_threshold(vec, threshold, max_results_per_query))
        .collect()
}