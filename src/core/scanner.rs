//! # 文件扫描器模块
//!
//! 负责遍历目录并采集文件元数据
//! 支持并行扫描以提高大目录的处理速度

use crate::core::identity::{get_file_identity, FileIdentity};
use crate::infra::error::{DedupError, Result};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

// ============================================================================
// 数据结构定义
// ============================================================================

/// 文件条目
///
/// 包含文件的基本元数据信息
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// 文件完整路径
    pub path: PathBuf,
    /// 文件名
    pub name: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 修改时间（Unix 时间戳）
    pub mtime: i64,
    /// 文件身份标识
    pub identity: FileIdentity,
}

/// 文件扫描器
pub struct Scanner {
    /// 并行工作线程数
    workers: usize,
}

// ============================================================================
// Scanner 实现
// ============================================================================

impl Scanner {
    /// 创建新的扫描器
    ///
    /// # 参数
    /// - `workers`: 并行工作线程数
    pub fn new(workers: usize) -> Self {
        Self { workers }
    }

    /// 扫描目录
    ///
    /// 遍历指定目录下的所有文件，采集元数据
    /// 使用并行处理提高效率
    ///
    /// # 参数
    /// - `root`: 要扫描的根目录
    ///
    /// # 返回
    /// - 文件条目列表
    pub fn scan_directory(&self, root: &Path) -> Result<Vec<FileEntry>> {
        // 规范化路径
        let root = root
            .canonicalize()
            .map_err(|e| DedupError::Path(format!("无法解析路径: {}", e)))?;

        // 第一步：收集所有文件路径
        let paths: Vec<PathBuf> = WalkDir::new(&root)
            .follow_links(false) // 不跟随符号链接，避免循环
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect();

        // 第二步：配置线程池
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.workers)
            .build()
            .map_err(|e| DedupError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // 第三步：并行处理文件
        let entries: Vec<FileEntry> = pool.install(|| {
            paths
                .par_iter()
                .filter_map(|path| {
                    match self.process_file(path) {
                        Ok(entry) => Some(entry),
                        Err(e) => {
                            // 记录错误但继续处理其他文件
                            log::warn!("处理文件失败 {:?}: {}", path, e);
                            None
                        }
                    }
                })
                .collect()
        });

        Ok(entries)
    }

    /// 处理单个文件
    ///
    /// 读取文件元数据并构建 FileEntry
    fn process_file(&self, path: &Path) -> Result<FileEntry> {
        let metadata = std::fs::metadata(path).map_err(DedupError::Io)?;

        // 获取修改时间
        let mtime = metadata
            .modified()
            .map_err(DedupError::Io)?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // 获取文件名
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // 获取文件身份标识
        let identity = get_file_identity(path)?;

        Ok(FileEntry {
            path: path.to_path_buf(),
            name,
            size: metadata.len(),
            mtime,
            identity,
        })
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    /// 测试扫描空目录
    #[test]
    fn test_scan_empty_directory() {
        let temp_dir = TempDir::new().unwrap();

        let scanner = Scanner::new(2);
        let entries = scanner.scan_directory(temp_dir.path()).unwrap();

        assert!(entries.is_empty());
    }

    /// 测试扫描包含文件的目录
    #[test]
    fn test_scan_directory_with_files() {
        let temp_dir = TempDir::new().unwrap();

        // 创建测试文件
        let file1_path = temp_dir.path().join("file1.txt");
        let file2_path = temp_dir.path().join("file2.txt");

        let mut file1 = File::create(&file1_path).unwrap();
        file1.write_all(b"content1").unwrap();

        let mut file2 = File::create(&file2_path).unwrap();
        file2.write_all(b"content2").unwrap();

        let scanner = Scanner::new(2);
        let entries = scanner.scan_directory(temp_dir.path()).unwrap();

        assert_eq!(entries.len(), 2);
    }

    /// 测试扫描嵌套目录
    #[test]
    fn test_scan_nested_directory() {
        let temp_dir = TempDir::new().unwrap();

        // 创建嵌套目录结构
        let sub_dir = temp_dir.path().join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();

        let file1 = temp_dir.path().join("root.txt");
        let file2 = sub_dir.join("nested.txt");

        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let scanner = Scanner::new(2);
        let entries = scanner.scan_directory(temp_dir.path()).unwrap();

        assert_eq!(entries.len(), 2);
    }

    /// 测试文件元数据正确性
    #[test]
    fn test_file_entry_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        let content = b"hello world";
        let mut file = File::create(&file_path).unwrap();
        file.write_all(content).unwrap();

        let scanner = Scanner::new(1);
        let entries = scanner.scan_directory(temp_dir.path()).unwrap();

        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.size, content.len() as u64);
        assert!(entry.mtime > 0);
    }

    /// 测试并行扫描
    #[test]
    fn test_parallel_scan() {
        let temp_dir = TempDir::new().unwrap();

        // 创建多个文件
        for i in 0..10 {
            let file_path = temp_dir.path().join(format!("file{}.txt", i));
            File::create(&file_path).unwrap();
        }

        let scanner = Scanner::new(4);
        let entries = scanner.scan_directory(temp_dir.path()).unwrap();

        assert_eq!(entries.len(), 10);
    }
}
