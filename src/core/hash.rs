//! # 哈希计算模块
//!
//! 实现分阶段哈希策略：
//! - Fast Hash: 只读取文件头尾各 64KB，快速筛选
//! - Full Hash: 读取完整文件内容，精确判定
//!
//! 使用 BLAKE3 算法，兼顾速度和安全性

use crate::infra::error::{DedupError, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Fast Hash 读取的块大小：64KB
const FAST_HASH_CHUNK_SIZE: u64 = 64 * 1024;

/// Full Hash 读取的缓冲区大小：1MB
const FULL_HASH_BUFFER_SIZE: usize = 1024 * 1024;

/// 计算 Fast Hash
///
/// 策略：读取文件前 64KB + 后 64KB（如果文件足够大）
/// 适用于快速筛选可能重复的文件
///
/// # 参数
/// - `path`: 文件路径
///
/// # 返回
/// - 64 字符的十六进制哈希字符串
///
/// # 示例
/// ```ignore
/// let hash = calculate_fast_hash(Path::new("large_file.bin"))?;
/// println!("Fast hash: {}", hash);
/// ```
pub fn calculate_fast_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let metadata = file.metadata().map_err(DedupError::Io)?;
    let file_size = metadata.len();

    let mut hasher = blake3::Hasher::new();

    // 读取文件头部
    let first_chunk_size = std::cmp::min(file_size, FAST_HASH_CHUNK_SIZE);
    let mut buffer = vec![0u8; first_chunk_size as usize];
    file.read_exact(&mut buffer).map_err(DedupError::Io)?;
    hasher.update(&buffer);

    // 如果文件足够大，也读取尾部
    // 条件：文件大小 > 2 × 64KB = 128KB
    if file_size > FAST_HASH_CHUNK_SIZE * 2 {
        file.seek(SeekFrom::End(-(FAST_HASH_CHUNK_SIZE as i64)))
            .map_err(DedupError::Io)?;
        let mut last_buffer = vec![0u8; FAST_HASH_CHUNK_SIZE as usize];
        file.read_exact(&mut last_buffer).map_err(DedupError::Io)?;
        hasher.update(&last_buffer);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// 计算 Full Hash
///
/// 读取完整文件内容，计算精确哈希值
/// 用于最终判定两个文件是否完全相同
///
/// # 参数
/// - `path`: 文件路径
///
/// # 返回
/// - 64 字符的十六进制哈希字符串
pub fn calculate_full_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let mut hasher = blake3::Hasher::new();

    // 使用较大的缓冲区提高 IO 效率
    let mut buffer = vec![0u8; FULL_HASH_BUFFER_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer).map_err(DedupError::Io)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// 计算 MD5 哈希（兼容传统工具）
///
/// 某些场景下需要与其他工具的 MD5 结果对比
#[allow(dead_code)]
pub fn calculate_md5(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let mut context = md5::Context::new();

    let mut buffer = vec![0u8; FULL_HASH_BUFFER_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer).map_err(DedupError::Io)?;
        if bytes_read == 0 {
            break;
        }
        context.consume(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", context.compute()))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// 测试小文件的 fast hash
    #[test]
    fn test_fast_hash_small_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();

        let hash = calculate_fast_hash(file.path()).unwrap();

        // 验证哈希值非空且长度正确（BLAKE3 输出 64 字符）
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    /// 测试 full hash
    #[test]
    fn test_full_hash() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();

        let hash = calculate_full_hash(file.path()).unwrap();

        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    /// 测试相同内容产生相同哈希
    #[test]
    fn test_same_content_same_hash() {
        let mut file1 = NamedTempFile::new().unwrap();
        let mut file2 = NamedTempFile::new().unwrap();

        let content = b"identical content for testing";
        file1.write_all(content).unwrap();
        file2.write_all(content).unwrap();

        let hash1 = calculate_full_hash(file1.path()).unwrap();
        let hash2 = calculate_full_hash(file2.path()).unwrap();

        assert_eq!(hash1, hash2);
    }

    /// 测试不同内容产生不同哈希
    #[test]
    fn test_different_content_different_hash() {
        let mut file1 = NamedTempFile::new().unwrap();
        let mut file2 = NamedTempFile::new().unwrap();

        file1.write_all(b"content A").unwrap();
        file2.write_all(b"content B").unwrap();

        let hash1 = calculate_full_hash(file1.path()).unwrap();
        let hash2 = calculate_full_hash(file2.path()).unwrap();

        assert_ne!(hash1, hash2);
    }

    /// 测试大文件的 fast hash（验证头尾读取逻辑）
    #[test]
    fn test_fast_hash_large_file() {
        let mut file = NamedTempFile::new().unwrap();

        // 创建一个 200KB 的文件
        let data = vec![0u8; 200 * 1024];
        file.write_all(&data).unwrap();

        let hash = calculate_fast_hash(file.path()).unwrap();

        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    /// 测试 MD5 计算
    #[test]
    fn test_md5_hash() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello").unwrap();

        let hash = calculate_md5(file.path()).unwrap();

        // MD5 输出 32 字符
        assert_eq!(hash.len(), 32);
        // "hello" 的 MD5 是已知的
        assert_eq!(hash, "5d41402abc4b2a76b9719d911017c592");
    }
}
