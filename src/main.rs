//! # dedup - 跨平台文件索引与查重工具
//!
//! 本工具用于：
//! 1. 遍历指定目录，采集文件与文件夹的基础信息（元数据）
//! 2. 将索引结果存储到本地 SQLite 数据库
//! 3. 通过分阶段 Hash（fast → full）的方式高效实现文件查重
//! 4. 支持增量扫描，避免重复计算
//! 5. 将查重结果输出为 CSV / JSON 文件

// ============================================================================
// 模块声明
// ============================================================================

/// 命令行接口模块
mod cli;

/// 核心业务逻辑模块
mod core;

/// 基础设施模块
mod infra;

// ============================================================================
// 入口函数
// ============================================================================

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, IndexArgs};
use std::path::PathBuf;

fn main() -> Result<()> {
    // 初始化日志系统
    env_logger::init();

    // 检查是否有命令行参数
    let args: Vec<String> = std::env::args().collect();

    // 如果只有程序名（双击运行），默认执行 index 命令
    if args.len() == 1 {
        println!("dedup - 跨平台文件索引与查重工具\n");
        println!("双击运行模式：将弹出文件夹选择对话框进行索引\n");

        let default_args = IndexArgs {
            path: None,
            db: PathBuf::from("dedup.db"),
            fast_hash: true,
            full_hash: true,
            workers: 4,
        };

        let result = cli::cmd_index(default_args);

        // 双击运行时暂停，让用户看到结果
        println!("\n按 Enter 键退出...");
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);

        return result;
    }

    // 正常解析命令行参数
    let cli = Cli::parse();

    // 根据子命令执行对应操作
    match cli.command {
        Commands::Index(args) => cli::cmd_index(args),
        Commands::Update(args) => cli::cmd_update(args),
        Commands::Dup(args) => cli::cmd_dup(args),
        Commands::Export(args) => cli::cmd_export(args),
        Commands::Stats(args) => cli::cmd_stats(args),
    }
}
