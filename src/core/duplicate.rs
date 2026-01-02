//! # 查重结果数据结构
//!
//! 定义重复文件和重复组的数据结构

use serde::Serialize;

/// 重复文件信息
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateFile {
    /// 文件完整路径
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 文件哈希值
    pub hash: String,
    /// 来源标识（卷/挂载点）
    pub source_id: String,
}

/// 重复文件组
///
/// 一组内容相同（或疑似相同）的文件
#[derive(Debug)]
pub struct DuplicateGroup {
    /// 组编号
    pub group_id: usize,
    /// 文件大小（组内所有文件大小相同）
    pub size: u64,
    /// 哈希值
    pub hash: String,
    /// 组内的文件列表
    pub files: Vec<DuplicateFile>,
}

impl DuplicateGroup {
    /// 计算该组浪费的空间
    ///
    /// 浪费空间 = (文件数 - 1) × 文件大小
    /// 因为只需要保留一份
    pub fn wasted_space(&self) -> u64 {
        self.files.len().saturating_sub(1) as u64 * self.size
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasted_space() {
        let group = DuplicateGroup {
            group_id: 1,
            size: 1000,
            hash: "abc".to_string(),
            files: vec![
                DuplicateFile {
                    path: "/a".to_string(),
                    size: 1000,
                    hash: "abc".to_string(),
                    source_id: "local".to_string(),
                },
                DuplicateFile {
                    path: "/b".to_string(),
                    size: 1000,
                    hash: "abc".to_string(),
                    source_id: "local".to_string(),
                },
                DuplicateFile {
                    path: "/c".to_string(),
                    size: 1000,
                    hash: "abc".to_string(),
                    source_id: "local".to_string(),
                },
            ],
        };

        // 3个文件，保留1个，浪费2个 = 2000字节
        assert_eq!(group.wasted_space(), 2000);
        assert_eq!(group.files.len(), 3);
    }

    #[test]
    fn test_single_file_no_waste() {
        let group = DuplicateGroup {
            group_id: 1,
            size: 1000,
            hash: "abc".to_string(),
            files: vec![DuplicateFile {
                path: "/a".to_string(),
                size: 1000,
                hash: "abc".to_string(),
                source_id: "local".to_string(),
            }],
        };

        assert_eq!(group.wasted_space(), 0);
    }
}
