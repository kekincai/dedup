//! # 核心业务逻辑模块
//!
//! 包含文件查重的核心算法和数据结构

/// 文件扫描器 - 负责遍历目录并采集文件元数据
pub mod scanner;

/// 哈希计算 - 实现 fast hash 和 full hash 策略
pub mod hash;

/// 文件身份标识 - 跨平台的文件唯一标识
pub mod identity;

/// 查重结果数据结构
pub mod duplicate;

// 重新导出常用类型
pub use duplicate::{DuplicateFile, DuplicateGroup};
pub use hash::{calculate_fast_hash, calculate_full_hash};
pub use scanner::{FileEntry, Scanner};
