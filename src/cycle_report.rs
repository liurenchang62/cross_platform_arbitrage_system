//! 周期末绩效：将终端同款统计追加写入日志文件。
use std::fs::{self, OpenOptions};
use std::io::Write;

use anyhow::{Context, Result};

const REPORT_PATH: &str = "logs/cycle_report.txt";

/// 追加一整段周期报告（含分隔头与正文，格式与终端一致）。
pub fn append_cycle_report(header_line: &str, body: &str) -> Result<()> {
    let path = std::path::Path::new(REPORT_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("创建目录 {:?}", parent))?;
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("打开 {:?}", path))?;
    writeln!(f, "{}", header_line)?;
    write!(f, "{}", body)?;
    if !body.ends_with('\n') {
        writeln!(f)?;
    }
    writeln!(f)?;
    Ok(())
}
