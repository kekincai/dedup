//! # 基础设施模块
//!
//! 包含数据库、导出、错误处理等基础功能

/// 错误类型定义
pub mod error;

/// 数据库操作
pub mod db;

/// 导出功能（CSV / JSON）
pub mod export;

/// 文件夹选择对话框
pub mod dialog;

// 重新导出常用类型
pub use db::Database;
pub use export::{export_csv, export_json};
