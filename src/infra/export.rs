//! # 导出模块
//!
//! 将查重结果导出为 CSV 或 JSON 格式

use crate::core::DuplicateGroup;
use crate::infra::error::{DedupError, Result};
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::path::Path;

// ============================================================================
// CSV 导出
// ============================================================================

/// CSV 行数据结构
#[derive(Serialize)]
struct CsvRow {
    /// 重复组 ID
    duplicate_group_id: usize,
    /// 文件路径
    path: String,
    /// 文件大小
    size: u64,
    /// 哈希值
    hash: String,
    /// 来源标识
    source_id: String,
}

/// 导出为 CSV 格式
///
/// 每行一个文件，包含组 ID、路径、大小、哈希、来源
///
/// # 参数
/// - `groups`: 重复文件组列表
/// - `path`: 输出文件路径
pub fn export_csv(groups: &[DuplicateGroup], path: &Path) -> Result<()> {
    let file = File::create(path).map_err(DedupError::Io)?;
    let mut writer = csv::Writer::from_writer(file);

    for group in groups {
        for file in &group.files {
            writer
                .serialize(CsvRow {
                    duplicate_group_id: group.group_id,
                    path: file.path.clone(),
                    size: file.size,
                    hash: file.hash.clone(),
                    source_id: file.source_id.clone(),
                })
                .map_err(|e| DedupError::Export(e.to_string()))?;
        }
    }

    writer.flush().map_err(DedupError::Io)?;
    Ok(())
}

// ============================================================================
// JSON 导出
// ============================================================================

/// JSON 导出根结构
#[derive(Serialize)]
struct JsonExport {
    /// 重复组列表
    groups: Vec<JsonGroup>,
    /// 总组数
    total_groups: usize,
    /// 总重复文件数
    total_duplicate_files: usize,
    /// 总浪费空间（字节）
    total_wasted_space: u64,
}

/// JSON 重复组结构
#[derive(Serialize)]
struct JsonGroup {
    /// 组 ID
    group_id: usize,
    /// 文件大小
    size: u64,
    /// 哈希值
    hash: String,
    /// 组内文件数
    file_count: usize,
    /// 文件列表
    files: Vec<JsonFile>,
}

/// JSON 文件结构
#[derive(Serialize)]
struct JsonFile {
    /// 文件路径
    path: String,
    /// 来源标识
    source_id: String,
}

/// 导出为 JSON 格式
///
/// 按组分组，包含统计信息
///
/// # 参数
/// - `groups`: 重复文件组列表
/// - `path`: 输出文件路径
pub fn export_json(groups: &[DuplicateGroup], path: &Path) -> Result<()> {
    let total_groups = groups.len();
    let total_duplicate_files: usize = groups.iter().map(|g| g.files.len()).sum();

    // 计算浪费空间：每组 (文件数 - 1) × 文件大小
    let total_wasted_space: u64 = groups.iter().map(|g| g.wasted_space()).sum();

    let json_groups: Vec<JsonGroup> = groups
        .iter()
        .map(|g| JsonGroup {
            group_id: g.group_id,
            size: g.size,
            hash: g.hash.clone(),
            file_count: g.files.len(),
            files: g
                .files
                .iter()
                .map(|f| JsonFile {
                    path: f.path.clone(),
                    source_id: f.source_id.clone(),
                })
                .collect(),
        })
        .collect();

    let export = JsonExport {
        groups: json_groups,
        total_groups,
        total_duplicate_files,
        total_wasted_space,
    };

    let mut file = File::create(path).map_err(DedupError::Io)?;
    let json =
        serde_json::to_string_pretty(&export).map_err(|e| DedupError::Export(e.to_string()))?;

    file.write_all(json.as_bytes()).map_err(DedupError::Io)?;
    Ok(())
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::DuplicateFile;
    use tempfile::TempDir;

    /// 创建测试用的重复组
    fn create_test_groups() -> Vec<DuplicateGroup> {
        vec![
            DuplicateGroup {
                group_id: 1,
                size: 1024,
                hash: "abc123".to_string(),
                files: vec![
                    DuplicateFile {
                        path: "/path/to/file1.txt".to_string(),
                        size: 1024,
                        hash: "abc123".to_string(),
                        source_id: "local".to_string(),
                    },
                    DuplicateFile {
                        path: "/path/to/file2.txt".to_string(),
                        size: 1024,
                        hash: "abc123".to_string(),
                        source_id: "local".to_string(),
                    },
                ],
            },
            DuplicateGroup {
                group_id: 2,
                size: 2048,
                hash: "def456".to_string(),
                files: vec![
                    DuplicateFile {
                        path: "/path/to/file3.txt".to_string(),
                        size: 2048,
                        hash: "def456".to_string(),
                        source_id: "external".to_string(),
                    },
                    DuplicateFile {
                        path: "/path/to/file4.txt".to_string(),
                        size: 2048,
                        hash: "def456".to_string(),
                        source_id: "external".to_string(),
                    },
                    DuplicateFile {
                        path: "/path/to/file5.txt".to_string(),
                        size: 2048,
                        hash: "def456".to_string(),
                        source_id: "external".to_string(),
                    },
                ],
            },
        ]
    }

    /// 测试 CSV 导出
    #[test]
    fn test_export_csv() {
        let temp_dir = TempDir::new().unwrap();
        let csv_path = temp_dir.path().join("test.csv");

        let groups = create_test_groups();
        export_csv(&groups, &csv_path).unwrap();

        // 验证文件已创建
        assert!(csv_path.exists());

        // 验证内容
        let content = std::fs::read_to_string(&csv_path).unwrap();
        assert!(content.contains("duplicate_group_id"));
        assert!(content.contains("file1.txt"));
        assert!(content.contains("abc123"));
    }

    /// 测试 JSON 导出
    #[test]
    fn test_export_json() {
        let temp_dir = TempDir::new().unwrap();
        let json_path = temp_dir.path().join("test.json");

        let groups = create_test_groups();
        export_json(&groups, &json_path).unwrap();

        // 验证文件已创建
        assert!(json_path.exists());

        // 验证 JSON 结构
        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["total_groups"], 2);
        assert_eq!(parsed["total_duplicate_files"], 5);
        // 浪费空间：组1 (2-1)*1024 + 组2 (3-1)*2048 = 1024 + 4096 = 5120
        assert_eq!(parsed["total_wasted_space"], 5120);
    }

    /// 测试空组导出
    #[test]
    fn test_export_empty_groups() {
        let temp_dir = TempDir::new().unwrap();
        let json_path = temp_dir.path().join("empty.json");

        let groups: Vec<DuplicateGroup> = vec![];
        export_json(&groups, &json_path).unwrap();

        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["total_groups"], 0);
        assert_eq!(parsed["total_duplicate_files"], 0);
        assert_eq!(parsed["total_wasted_space"], 0);
    }
}
