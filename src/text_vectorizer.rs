// text_vectorizer.rs
//! TF-IDF 文本向量化模块
//! 
//! 负责将文本转换为 TF-IDF 向量，供匹配引擎使用

use std::collections::{HashMap, HashSet};
use rust_stemmers::{Algorithm, Stemmer};
use ndarray::Array1;

/// 停用词集合（常见无意义词语）
fn get_stop_words() -> HashSet<&'static str> {
    let mut set = HashSet::new();
    
    // 英文停用词
    for word in &[
        "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into", "is",
        "it", "no", "not", "of", "on", "or", "such", "that", "the", "their", "then", "there",
        "these", "they", "this", "to", "was", "will", "with", "would", "am", "been", "being",
        "did", "do", "does", "doing", "had", "has", "have", "having", "he", "her", "here",
        "hers", "herself", "him", "himself", "his", "how", "i", "me", "my", "myself",
        "our", "ours", "ourselves", "she", "should", "than", "that", "theirs", "them",
        "themselves", "there", "these", "they", "this", "those", "through", "too", "under",
        "until", "up", "very", "was", "we", "were", "what", "when", "where", "which", "while",
        "who", "whom", "why", "you", "your", "yours", "yourself", "yourselves",
        
        // 预测市场常见无意义词
        "will", "be", "the", "market", "price", "prediction", "event", "outcome",
        "contract", "share", "stock", "binary", "option", "trade", "trading",
        "buy", "sell", "yes", "no", "up", "down", "over", "under","points","point","round"
    ] {
        set.insert(*word);
    }
    
    set
}

/// 文本向量化器配置
#[derive(Debug, Clone)]
pub struct VectorizerConfig {
    /// 是否启用词干提取
    pub use_stemming: bool,
    /// 是否过滤停用词
    pub filter_stop_words: bool,
    /// 最小词长度
    pub min_word_length: usize,
    /// 最大词频比例（超过此比例的词将被忽略）
    pub max_df_ratio: f64,
    /// 最小文档频率（低于此值的词将被忽略）
    pub min_df: usize,
    /// 是否进行 L2 归一化
    pub normalize: bool,
    /// 自定义停用词
    pub custom_stop_words: HashSet<String>,
}

impl Default for VectorizerConfig {
    fn default() -> Self {
        Self {
            use_stemming: true,
            filter_stop_words: true,
            min_word_length: 2,
            max_df_ratio: 0.8,
            min_df: 1,
            normalize: true,
            custom_stop_words: HashSet::new(),
        }
    }
}

/// 包装 Stemmer 以支持 Debug 和 Clone
struct StemmerWrapper(Stemmer);

impl StemmerWrapper {
    fn new(algorithm: Algorithm) -> Self {
        Self(Stemmer::create(algorithm))
    }
    
    fn stem(&self, word: &str) -> String {
        self.0.stem(word).to_string()
    }
}

// 手动实现 Debug
impl std::fmt::Debug for StemmerWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StemmerWrapper").finish()
    }
}

// 手动实现 Clone
impl Clone for StemmerWrapper {
    fn clone(&self) -> Self {
        Self(Stemmer::create(Algorithm::English))
    }
}

/// 文本向量化器
#[derive(Debug, Clone)]
pub struct TextVectorizer {
    /// 配置
    config: VectorizerConfig,
    /// 词干提取器
    stemmer: Option<StemmerWrapper>,
    /// 停用词集合
    stop_words: HashSet<String>,
    /// 词汇表（词 -> 索引）
    vocabulary: HashMap<String, usize>,
    /// IDF 值
    idf: Vec<f64>,
    /// 文档数量
    n_docs: usize,
    /// 是否已拟合
    fitted: bool,
}

impl TextVectorizer {
    /// 创建新的文本向量化器
    pub fn new(config: VectorizerConfig) -> Self {
        let stemmer = if config.use_stemming {
            Some(StemmerWrapper::new(Algorithm::English))
        } else {
            None
        };
        
        // 合并默认停用词和自定义停用词
        let mut stop_words = get_stop_words()
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>();
        
        for word in &config.custom_stop_words {
            stop_words.insert(word.clone());
        }
        
        Self {
            config,
            stemmer,
            stop_words,
            vocabulary: HashMap::new(),
            idf: Vec::new(),
            n_docs: 0,
            fitted: false,
        }
    }
    
    /// 创建默认配置的文本向量化器
    pub fn default() -> Self {
        Self::new(VectorizerConfig::default())
    }
    
    /// 对文本进行分词和预处理
    pub fn tokenize(&self, text: &str) -> Vec<String> {
        let text = text.to_lowercase();
        
        // 简单分词：按非字母数字字符分割，但保留连字符连接的词
        let words: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        
        let mut result = Vec::new();
        
        for word in words {
            // 如果词包含连字符，拆分成多个词
            if word.contains('-') {
                for part in word.split('-') {
                    if !part.is_empty() {
                        if let Some(processed) = self.process_token(part) {
                            result.push(processed);
                        }
                    }
                }
            } else {
                if let Some(processed) = self.process_token(&word) {
                    result.push(processed);
                }
            }
        }
        
        result
    }
    
    /// 处理单个词元（过滤、词干提取）
    fn process_token(&self, token: &str) -> Option<String> {
        // 检查是否全是数字
        if token.chars().all(|c| c.is_ascii_digit()) {
            // 保留年份（4位数字）和常见数字（如2024）
            if token.len() == 4 && token.starts_with(|c: char| c == '1' || c == '2') {
                return Some(format!("YEAR_{}", token));
            }
            return None;
        }
        
        // 检查长度
        if token.len() < self.config.min_word_length {
            return None;
        }
        
        // 检查停用词
        if self.config.filter_stop_words && self.stop_words.contains(token) {
            return None;
        }
        
        // 词干提取
        if let Some(stemmer) = &self.stemmer {
            Some(stemmer.stem(token))
        } else {
            Some(token.to_string())
        }
    }
    
    /// 拟合文档集，构建词汇表和 IDF
    pub fn fit(&mut self, documents: &[String]) -> &mut Self {
        if documents.is_empty() {
            return self;
        }
        
        self.n_docs = documents.len();
        
        // 第一步：对所有文档分词
        let mut all_tokens: Vec<Vec<String>> = Vec::with_capacity(documents.len());
        for doc in documents {
            all_tokens.push(self.tokenize(doc));
        }
        
        // 第二步：统计文档频率
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        
        for tokens in &all_tokens {
            // 文档内去重用于文档频率统计
            let unique_tokens: HashSet<_> = tokens.iter().cloned().collect();
            
            for token in unique_tokens {
                *doc_freq.entry(token.clone()).or_insert(0) += 1;
            }
        }
        
        // 第三步：过滤词汇表
        let max_df = (self.config.max_df_ratio * self.n_docs as f64).ceil() as usize;
        
        let mut vocab: Vec<String> = doc_freq
            .into_iter()
            .filter(|(_, df)| {
                *df >= self.config.min_df && *df <= max_df
            })
            .map(|(word, _)| word)
            .collect();
        
        // 按字母排序保持一致性
        vocab.sort();
        
        // 构建词汇表索引
        self.vocabulary = vocab
            .into_iter()
            .enumerate()
            .map(|(i, word)| (word, i))
            .collect();
        
        // 第四步：计算 IDF
        let vocab_size = self.vocabulary.len();
        self.idf = vec![0.0; vocab_size];
        
        // 重新统计文档频率（只保留词汇表中的词）
        let mut filtered_doc_freq = vec![0; vocab_size];
        
        for tokens in &all_tokens {
            let unique_tokens: HashSet<_> = tokens.iter().cloned().collect();
            
            for token in unique_tokens {
                if let Some(&idx) = self.vocabulary.get(&token) {
                    filtered_doc_freq[idx] += 1;
                }
            }
        }
        
        // 计算 IDF: idf = log((1 + n) / (1 + df)) + 1
        for (idx, &df) in filtered_doc_freq.iter().enumerate() {
            if df > 0 {
                self.idf[idx] = ((1.0 + self.n_docs as f64) / (1.0 + df as f64)).ln() + 1.0;
            } else {
                self.idf[idx] = 1.0; // 默认值
            }
        }
        
        self.fitted = true;
        self
    }
    
    /// 将单个文本转换为 TF-IDF 向量
    pub fn transform(&self, text: &str) -> Option<Array1<f64>> {
        if !self.fitted || self.vocabulary.is_empty() {
            return None;
        }
        
        let tokens = self.tokenize(text);
        let mut vector = vec![0.0; self.vocabulary.len()];
        
        // 计算 TF
        for token in tokens {
            if let Some(&idx) = self.vocabulary.get(&token) {
                vector[idx] += 1.0;
            }
        }
        
        // 如果文本中没有词汇表中的词，返回 None
        if vector.iter().all(|&x| x == 0.0) {
            return None;
        }
        
        // 计算 TF-IDF
        for (idx, &idf_value) in self.idf.iter().enumerate() {
            if vector[idx] > 0.0 {
                vector[idx] *= idf_value;
            }
        }
        
        let mut array = Array1::from_vec(vector);
        
        // L2 归一化
        if self.config.normalize {
            let norm = array.dot(&array).sqrt();
            if norm > 1e-12 {
                array.mapv_inplace(|x| x / norm);
            }
        }
        
        Some(array)
    }
    
    /// 批量转换文本为向量
    pub fn transform_batch(&self, texts: &[String]) -> Vec<Option<Array1<f64>>> {
        texts.iter()
            .map(|text| self.transform(text))
            .collect()
    }
    
    /// 拟合并转换所有文档
    pub fn fit_transform(&mut self, documents: &[String]) -> Vec<Option<Array1<f64>>> {
        self.fit(documents);
        self.transform_batch(documents)
    }
    
    /// 获取词汇表大小
    pub fn vocab_size(&self) -> usize {
        self.vocabulary.len()
    }
    
    /// 检查是否已拟合
    pub fn is_fitted(&self) -> bool {
        self.fitted
    }
    
    /// 获取词汇表（用于调试）
    pub fn vocabulary(&self) -> &HashMap<String, usize> {
        &self.vocabulary
    }
}

/// 计算两个向量的余弦相似度
pub fn cosine_similarity(v1: &Array1<f64>, v2: &Array1<f64>) -> f64 {
    if v1.len() != v2.len() {
        return 0.0;
    }
    
    let dot_product = v1.dot(v2);
    let norm1 = v1.dot(v1).sqrt();
    let norm2 = v2.dot(v2).sqrt();
    
    if norm1 < 1e-12 || norm2 < 1e-12 {
        0.0
    } else {
        dot_product / (norm1 * norm2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tokenize() {
        let vectorizer = TextVectorizer::default();
        let tokens = vectorizer.tokenize("Will Bitcoin reach $100,000 in 2024?");
        println!("{:?}", tokens);
        assert!(!tokens.is_empty());
    }
    
    #[test]
    fn test_fit_transform() {
        let mut vectorizer = TextVectorizer::default();
        let docs = vec![
            "Will Bitcoin reach $100,000 in 2024?".to_string(),
            "Ethereum price prediction for 2024".to_string(),
            "Solana vs Ethereum: which will win?".to_string(),
        ];
        
        let vectors = vectorizer.fit_transform(&docs);
        assert_eq!(vectors.len(), 3);
        assert!(vectorizer.vocab_size() > 0);
    }
}