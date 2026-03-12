// src/unclassified_logger.rs
//! 未分类日志模块：记录没有匹配到任何类别的事件
//! 
//! 按天分文件存储，自动去重，便于后续分析添加新规则

use chrono::{Local, Datelike};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::{Path, PathBuf};
use anyhow::{Result, Context};
use crate::event::Event;

/// 未分类日志记录
#[derive(Debug)]
struct UnclassifiedRecord {
    /// 时间戳
    timestamp: String,
    /// 事件ID
    event_id: String,
    /// 平台
    platform: String,
    /// 标题
    title: String,
    /// 提取的关键词
    keywords: String,
}

/// 未分类日志器
pub struct UnclassifiedLogger {
    /// 日志目录
    log_dir: PathBuf,
    /// 当天已记录的标题哈希集合（用于去重）
    today_records: HashSet<String>,
    /// 当前日期
    current_date: String,
}

impl UnclassifiedLogger {
    /// 创建新的未分类日志器
    pub fn new<P: AsRef<Path>>(log_dir: P) -> Result<Self> {
        let log_dir = log_dir.as_ref().to_path_buf();
        fs::create_dir_all(&log_dir)?;
        
        let today = Local::now().format("%Y-%m-%d").to_string();
        
        Ok(Self {
            log_dir,
            today_records: HashSet::new(),
            current_date: today,
        })
    }
    
    /// 检查日期是否变化，如果变化则清空今日记录
    fn check_date_change(&mut self) {
        let today = Local::now().format("%Y-%m-%d").to_string();
        if today != self.current_date {
            self.today_records.clear();
            self.current_date = today;
        }
    }
    
    /// 记录未分类事件
    pub fn log_unclassified(&mut self, event: &Event) -> Result<()> {
        self.check_date_change();
        
        // 生成标题哈希用于去重
        let title_hash = format!("{}:{}", event.platform, event.event_id);
        
        // 检查是否已在今日记录过
        if self.today_records.contains(&title_hash) {
            return Ok(()); // 已记录，跳过
        }
        
        // 从标题提取关键词（简单提取长度>3的词）
        let keywords: Vec<String> = event.title
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() > 3)
            .collect();
        
        let record = UnclassifiedRecord {
            timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            event_id: event.event_id.clone(),
            platform: event.platform.clone(),
            title: event.title.clone(),
            keywords: keywords.join(","),
        };
        
        // 写入日志文件
        self.write_record(&record)?;
        
        // 记录哈希
        self.today_records.insert(title_hash);
        
        Ok(())
    }
    
    /// 写入记录到文件
    fn write_record(&self, record: &UnclassifiedRecord) -> Result<()> {
        let date = Local::now().format("%Y-%m-%d");
        let log_file = self.log_dir.join(format!("unclassified-{}.csv", date));
        
        let file_exists = log_file.exists();
        
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context(format!("打开日志文件失败: {:?}", log_file))?;
        
        let mut writer = BufWriter::new(file);
        
        // 如果是新文件，写入表头
        if !file_exists {
            writeln!(writer, "timestamp,event_id,platform,title,keywords")?;
        }
        
        // 写入记录
        writeln!(
            writer,
            "{},{},{},\"{}\",{}",
            record.timestamp,
            record.event_id,
            record.platform,
            record.title.replace('"', "\"\""), // CSV 转义
            record.keywords
        )?;
        
        Ok(())
    }
    
    /// 批量记录未分类事件
    pub fn log_batch_unclassified(&mut self, events: &[Event]) -> Result<usize> {
        let mut count = 0;
        for event in events {
            if self.log_unclassified(event).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }
    
    /// 获取今日已记录数量
    pub fn today_record_count(&self) -> usize {
        self.today_records.len()
    }
    
    /// 获取日志文件路径
    pub fn get_log_file_path(&self) -> PathBuf {
        let date = Local::now().format("%Y-%m-%d");
        self.log_dir.join(format!("unclassified-{}.csv", date))
    }
    
    /// 分析日志文件，统计高频关键词
    pub fn analyze_logs(days: i64) -> Result<Vec<(String, usize)>> {
        let mut keyword_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let log_dir = Path::new("logs/unclassified");
        
        if !log_dir.exists() {
            return Ok(Vec::new());
        }
        
        let cutoff_date = Local::now() - chrono::Duration::days(days);
        
        for entry in fs::read_dir(log_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            // 只处理 .csv 文件
            if path.extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            
            // 检查文件修改时间
            let metadata = fs::metadata(&path)?;
            if let Ok(modified) = metadata.modified() {
                let modified: chrono::DateTime<Local> = modified.into();
                if modified < cutoff_date {
                    continue; // 跳过旧文件
                }
            }
            
            // 读取文件内容
            let content = fs::read_to_string(&path)?;
            for line in content.lines().skip(1) { // 跳过表头
                let fields: Vec<&str> = line.split(',').collect();
                if fields.len() >= 5 {
                    let keywords_str = fields[4];
                    for keyword in keywords_str.split(',') {
                        if !keyword.is_empty() {
                            *keyword_count.entry(keyword.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
        
        // 排序并返回前30个
        let mut result: Vec<_> = keyword_count.into_iter().collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result.truncate(30);
        
        Ok(result)
    }
}

/// 便捷函数：快速记录未分类事件
pub fn log_unclassified_event(logger: &mut UnclassifiedLogger, event: &Event) {
    if let Err(e) = logger.log_unclassified(event) {
        eprintln!("⚠️ 记录未分类事件失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_logger_creation() {
        let dir = tempdir().unwrap();
        let mut logger = UnclassifiedLogger::new(dir.path()).unwrap();
        
        let event = Event::new(
            "polymarket".to_string(),
            "123".to_string(),
            "Test Event".to_string(),
            "".to_string(),
        );
        
        assert!(logger.log_unclassified(&event).is_ok());
    }
}