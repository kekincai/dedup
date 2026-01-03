//! # 命令行接口模块
//!
//! 定义 CLI 参数结构和命令处理函数

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use crate::core::Scanner;
use crate::infra::dialog::pick_multiple_folders;
use crate::infra::error::DedupError;
use crate::infra::{export_csv, export_json, Database};

// ============================================================================
// CLI 参数定义
// ============================================================================

/// dedup - 跨平台文件索引与查重工具
#[derive(Parser)]
#[command(name = "dedup")]
#[command(about = "跨平台文件索引与查重工具")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// 子命令
#[derive(Subcommand)]
pub enum Commands {
    /// 建立初始索引
    Index(IndexArgs),
    /// 增量更新索引
    Update(UpdateArgs),
    /// 查找并显示重复文件
    Dup(DupArgs),
    /// 导出查重结果到文件
    Export(ExportArgs),
    /// 显示数据库统计信息
    Stats(StatsArgs),
}

/// index 命令参数
#[derive(Parser)]
pub struct IndexArgs {
    /// 要索引的目录路径（可指定多个，不指定则弹出选择对话框）
    pub path: Vec<PathBuf>,
    /// 数据库文件路径
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// 计算快速哈希（前后 64KB）
    #[arg(long)]
    pub fast_hash: bool,
    /// 计算完整哈希（整个文件）
    #[arg(long)]
    pub full_hash: bool,
    /// 并行工作线程数
    #[arg(long, default_value = "4")]
    pub workers: usize,
}

/// update 命令参数
#[derive(Parser)]
pub struct UpdateArgs {
    /// 要更新的目录路径（可指定多个，不指定则弹出选择对话框）
    pub path: Vec<PathBuf>,
    /// 数据库文件路径
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// 计算快速哈希
    #[arg(long)]
    pub fast_hash: bool,
    /// 计算完整哈希
    #[arg(long)]
    pub full_hash: bool,
    /// 并行工作线程数
    #[arg(long, default_value = "4")]
    pub workers: usize,
}

/// dup 命令参数
#[derive(Parser)]
pub struct DupArgs {
    /// 数据库文件路径
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// 查重模式
    #[arg(long, value_enum, default_value = "full")]
    pub by: DupMode,
    /// 最小文件大小过滤
    #[arg(long)]
    pub min_size: Option<String>,
}

/// export 命令参数
#[derive(Parser)]
pub struct ExportArgs {
    /// 数据库文件路径
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// 查重模式
    #[arg(long, value_enum, default_value = "full")]
    pub by: DupMode,
    /// 导出为 CSV 文件
    #[arg(long)]
    pub csv: Option<PathBuf>,
    /// 导出为 JSON 文件
    #[arg(long)]
    pub json: Option<PathBuf>,
    /// 最小文件大小过滤
    #[arg(long)]
    pub min_size: Option<String>,
}

/// stats 命令参数
#[derive(Parser)]
pub struct StatsArgs {
    /// 数据库文件路径
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
}

/// 查重模式
#[derive(Clone, Copy, ValueEnum)]
pub enum DupMode {
    /// 按完整哈希匹配（内容完全一致）
    Full,
    /// 按大小+快速哈希匹配（疑似重复）
    Fast,
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 解析文件大小字符串
///
/// 支持格式：10B, 10KB, 10MB, 10GB
fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    let num: u64 = num_str.trim().parse()?;
    Ok(num * multiplier)
}

/// 创建进度条
fn create_progress_bar(total: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

/// 获取目录路径列表
///
/// 如果命令行指定了路径，使用指定的路径
/// 否则弹出文件夹选择对话框（支持多选）
fn get_directory_paths(paths: Vec<PathBuf>, title: &str) -> Result<Vec<PathBuf>> {
    if paths.is_empty() {
        println!("未指定目录，正在打开文件夹选择对话框...");
        println!("提示：可多次选择文件夹，点击取消结束选择\n");
        pick_multiple_folders(title).map_err(|e| match e {
            DedupError::Cancelled => anyhow::anyhow!("用户取消了操作"),
            _ => anyhow::anyhow!("{}", e),
        })
    } else {
        Ok(paths)
    }
}

// ============================================================================
// 命令处理函数
// ============================================================================

/// 执行 index 命令
pub fn cmd_index(args: IndexArgs) -> Result<()> {
    let paths = get_directory_paths(args.path, "选择要索引的文件夹")?;

    // 打开数据库
    let mut db = Database::open(&args.db)?;
    db.init()?;

    let scanner = Scanner::new(args.workers);

    for path in &paths {
        println!("\n正在索引: {:?}", path);

        // 扫描目录
        let entries = scanner.scan_directory(path)?;

        // 显示进度条
        let pb = create_progress_bar(entries.len() as u64, "索引文件");

        // 处理每个文件
        for entry in entries {
            db.upsert_file(&entry, args.fast_hash, args.full_hash)?;
            pb.inc(1);
        }

        pb.finish_with_message("完成");
    }

    println!("\n已索引 {} 个文件", db.file_count()?);

    Ok(())
}

/// 执行 update 命令
pub fn cmd_update(args: UpdateArgs) -> Result<()> {
    let paths = get_directory_paths(args.path, "选择要更新的文件夹")?;

    // 打开数据库
    let mut db = Database::open(&args.db)?;

    let scanner = Scanner::new(args.workers);

    let mut total_updated = 0;
    let mut total_skipped = 0;

    for path in &paths {
        println!("\n正在更新索引: {:?}", path);

        // 扫描目录
        let entries = scanner.scan_directory(path)?;

        // 显示进度条
        let pb = create_progress_bar(entries.len() as u64, "更新文件");

        let mut updated = 0;
        let mut skipped = 0;

        // 增量更新
        for entry in entries {
            if db.needs_update(&entry)? {
                db.upsert_file(&entry, args.fast_hash, args.full_hash)?;
                updated += 1;
            } else {
                db.touch_file(&entry)?;
                skipped += 1;
            }
            pb.inc(1);
        }

        pb.finish_with_message("完成");
        println!("  更新: {}, 跳过: {}", updated, skipped);

        total_updated += updated;
        total_skipped += skipped;
    }

    println!("\n总计 - 已更新: {}, 已跳过: {}", total_updated, total_skipped);

    Ok(())
}

/// 执行 dup 命令
pub fn cmd_dup(args: DupArgs) -> Result<()> {
    let db = Database::open(&args.db)?;

    // 解析最小文件大小
    let min_size = args
        .min_size
        .map(|s| parse_size(&s))
        .transpose()?
        .unwrap_or(0);

    // 查找重复文件
    let groups = match args.by {
        DupMode::Full => db.find_duplicates_by_full_hash(min_size)?,
        DupMode::Fast => db.find_duplicates_by_fast_hash(min_size)?,
    };

    if groups.is_empty() {
        println!("未找到重复文件。");
        return Ok(());
    }

    // 显示结果
    println!("找到 {} 个重复组:\n", groups.len());

    for group in &groups {
        println!(
            "组 {} ({} 个文件, 每个 {}):",
            group.group_id,
            group.files.len(),
            bytesize::ByteSize(group.size)
        );
        for file in &group.files {
            println!("  {}", file.path);
        }
        println!();
    }

    // 显示统计
    let total_wasted: u64 = groups.iter().map(|g| g.wasted_space()).sum();
    println!(
        "总计可节省空间: {}",
        bytesize::ByteSize(total_wasted)
    );

    Ok(())
}

/// 执行 export 命令
pub fn cmd_export(args: ExportArgs) -> Result<()> {
    let db = Database::open(&args.db)?;

    // 解析最小文件大小
    let min_size = args
        .min_size
        .map(|s| parse_size(&s))
        .transpose()?
        .unwrap_or(0);

    // 查找重复文件
    let groups = match args.by {
        DupMode::Full => db.find_duplicates_by_full_hash(min_size)?,
        DupMode::Fast => db.find_duplicates_by_fast_hash(min_size)?,
    };

    // 导出 CSV
    if let Some(csv_path) = args.csv {
        export_csv(&groups, &csv_path)?;
        println!("已导出 CSV: {:?}", csv_path);
    }

    // 导出 JSON
    if let Some(json_path) = args.json {
        export_json(&groups, &json_path)?;
        println!("已导出 JSON: {:?}", json_path);
    }

    Ok(())
}

/// 执行 stats 命令
pub fn cmd_stats(args: StatsArgs) -> Result<()> {
    let db = Database::open(&args.db)?;
    let stats = db.get_stats()?;

    println!("数据库统计信息:");
    println!("  文件总数: {}", stats.total_files);
    println!("  总大小: {}", bytesize::ByteSize(stats.total_size));
    println!("  已计算 fast_hash: {}", stats.with_fast_hash);
    println!("  已计算 full_hash: {}", stats.with_full_hash);
    println!("  疑似重复组数 (fast): {}", stats.fast_dup_groups);
    println!("  精确重复组数 (full): {}", stats.full_dup_groups);

    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试文件大小解析
    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("10KB").unwrap(), 10 * 1024);
        assert_eq!(parse_size("10MB").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
    }

    /// 测试大小写不敏感
    #[test]
    fn test_parse_size_case_insensitive() {
        assert_eq!(parse_size("10kb").unwrap(), 10 * 1024);
        assert_eq!(parse_size("10Kb").unwrap(), 10 * 1024);
        assert_eq!(parse_size("10KB").unwrap(), 10 * 1024);
    }

    /// 测试带空格的输入
    #[test]
    fn test_parse_size_with_spaces() {
        assert_eq!(parse_size(" 10 KB ").unwrap(), 10 * 1024);
    }
}
