//! # 错误处理模块
//!
//! 定义项目中使用的错误类型

use thiserror::Error;

/// 项目错误类型
#[derive(Error, Debug)]
pub enum DedupError {
    /// 数据库错误
    #[error("数据库错误: {0}")]
    Database(#[from] rusqlite::Error),

    /// IO 错误
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    /// 路径错误
    #[error("路径错误: {0}")]
    Path(String),

    /// 导出错误
    #[error("导出错误: {0}")]
    Export(String),

    /// 用户取消操作
    #[error("用户取消操作")]
    Cancelled,
}

/// 项目 Result 类型别名
pub type Result<T> = std::result::Result<T, DedupError>;
