//! # 文件夹选择对话框模块
//!
//! 提供跨平台的文件夹选择功能
//! 当命令行未指定路径时，弹出系统原生对话框让用户选择

use crate::infra::error::{DedupError, Result};
use std::path::PathBuf;

/// 打开文件夹选择对话框（单选）
#[allow(dead_code)]
pub fn pick_folder(title: &str) -> Result<PathBuf> {
    let folder = rfd::FileDialog::new().set_title(title).pick_folder();

    match folder {
        Some(path) => Ok(path),
        None => Err(DedupError::Cancelled),
    }
}

/// 循环选择多个文件夹
///
/// 用户可以多次选择文件夹，直到点击取消结束
///
/// # 参数
/// - `title`: 对话框标题
///
/// # 返回
/// - 用户选择的所有文件夹路径列表
pub fn pick_multiple_folders(title: &str) -> Result<Vec<PathBuf>> {
    let mut folders = Vec::new();

    loop {
        let msg = if folders.is_empty() {
            format!("{}", title)
        } else {
            format!("{} (已选 {} 个，取消结束选择)", title, folders.len())
        };

        let folder = rfd::FileDialog::new().set_title(&msg).pick_folder();

        match folder {
            Some(path) => {
                if !folders.contains(&path) {
                    println!("已添加: {:?}", path);
                    folders.push(path);
                } else {
                    println!("该文件夹已选择，跳过");
                }
            }
            None => break,
        }
    }

    if folders.is_empty() {
        Err(DedupError::Cancelled)
    } else {
        Ok(folders)
    }
}

/// 打开文件保存对话框
///
/// 弹出系统原生的文件保存对话框
///
/// # 参数
/// - `title`: 对话框标题
/// - `default_name`: 默认文件名
/// - `filters`: 文件类型过滤器 (扩展名, 描述)
///
/// # 返回
/// - 用户选择的保存路径
#[allow(dead_code)]
pub fn pick_save_file(title: &str, default_name: &str, extension: &str) -> Result<PathBuf> {
    let file = rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(default_name)
        .add_filter(extension, &[extension])
        .save_file();

    match file {
        Some(path) => Ok(path),
        None => Err(DedupError::Cancelled),
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    // 注意：对话框测试需要 GUI 环境，在 CI 中会跳过
    // 这里只测试模块是否正确编译

    #[test]
    fn test_module_compiles() {
        // 确保模块可以正确编译
        assert!(true);
    }
}
