//! # 文件身份标识模块
//!
//! 跨平台的文件唯一标识实现：
//! - Windows: Volume Serial Number + FileId
//! - macOS/Linux: st_dev + st_ino
//! - 网络共享: 退化为路径标识
//!
//! 文件身份用于增量扫描时判断文件是否发生变化

use crate::infra::error::{DedupError, Result};
use std::path::Path;

// ============================================================================
// 数据结构定义
// ============================================================================

/// 文件身份标识类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityKind {
    /// Windows 文件标识（Volume Serial + FileId）
    #[cfg(target_os = "windows")]
    Windows,
    /// Unix 文件标识（st_dev + st_ino）
    #[cfg(unix)]
    Unix,
    /// 路径标识（用于网络共享等无法获取稳定标识的场景）
    Path,
}

impl IdentityKind {
    /// 转换为字符串（用于数据库存储）
    pub fn as_str(&self) -> &'static str {
        match self {
            #[cfg(target_os = "windows")]
            IdentityKind::Windows => "win",
            #[cfg(unix)]
            IdentityKind::Unix => "unix",
            IdentityKind::Path => "path",
        }
    }
}

/// 文件身份信息
#[derive(Debug, Clone)]
pub struct FileIdentity {
    /// 标识类型
    pub kind: IdentityKind,
    /// 标识值（格式取决于类型）
    pub value: String,
    /// 来源标识（卷/设备/挂载点）
    pub source_id: String,
}

// ============================================================================
// Windows 实现
// ============================================================================

#[cfg(target_os = "windows")]
pub fn get_file_identity(path: &Path) -> Result<FileIdentity> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, OPEN_EXISTING,
    };

    let path_str = path.to_string_lossy();
    let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        let handle = CreateFileW(
            PCWSTR::from_raw(wide_path.as_ptr()),
            0, // 只需要读取元数据，不需要文件访问权限
            FILE_SHARE_READ,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            HANDLE::default(),
        );

        if let Ok(handle) = handle {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            if GetFileInformationByHandle(handle, &mut info).is_ok() {
                let _ = windows::Win32::Foundation::CloseHandle(handle);

                // 组合高低位得到 64 位文件 ID
                let file_id = ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64);
                let volume_serial = info.dwVolumeSerialNumber;

                return Ok(FileIdentity {
                    kind: IdentityKind::Windows,
                    value: format!("{}:{}", volume_serial, file_id),
                    source_id: format!("vol:{}", volume_serial),
                });
            }
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
    }

    // 无法获取 Windows 文件 ID，退化为路径标识
    Ok(FileIdentity {
        kind: IdentityKind::Path,
        value: path.to_string_lossy().to_string(),
        source_id: get_source_id(path),
    })
}

// ============================================================================
// Unix (macOS / Linux) 实现
// ============================================================================

#[cfg(unix)]
pub fn get_file_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::metadata(path).map_err(DedupError::Io)?;

    let dev = metadata.dev();
    let ino = metadata.ino();

    // 检查是否可能是网络挂载
    let source_id = get_source_id(path);
    let is_network = source_id.starts_with("smb://") || source_id.starts_with("nfs://");

    if is_network {
        // 网络挂载的 inode 可能不稳定，退化为路径标识
        Ok(FileIdentity {
            kind: IdentityKind::Path,
            value: path.to_string_lossy().to_string(),
            source_id,
        })
    } else {
        Ok(FileIdentity {
            kind: IdentityKind::Unix,
            value: format!("{}:{}", dev, ino),
            source_id: format!("dev:{}", dev),
        })
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 获取文件的来源标识
///
/// 用于区分不同的存储设备/挂载点
fn get_source_id(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    #[cfg(unix)]
    {
        // Unix 系统：尝试获取挂载点
        if let Some(root) = canonical.ancestors().last() {
            return root.to_string_lossy().to_string();
        }
    }

    #[cfg(windows)]
    {
        // Windows：使用盘符或 UNC 路径
        if let Some(prefix) = canonical.components().next() {
            return prefix.as_os_str().to_string_lossy().to_string();
        }
    }

    "local".to_string()
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    /// 测试获取文件身份
    #[test]
    fn test_get_file_identity() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        File::create(&file_path).unwrap();

        let identity = get_file_identity(&file_path).unwrap();

        // 验证身份信息非空
        assert!(!identity.value.is_empty());
        assert!(!identity.source_id.is_empty());
    }

    /// 测试同一文件的身份一致性
    #[test]
    fn test_identity_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        File::create(&file_path).unwrap();

        let identity1 = get_file_identity(&file_path).unwrap();
        let identity2 = get_file_identity(&file_path).unwrap();

        assert_eq!(identity1.value, identity2.value);
        assert_eq!(identity1.kind, identity2.kind);
    }

    /// 测试不同文件的身份不同
    #[test]
    fn test_different_files_different_identity() {
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let identity1 = get_file_identity(&file1).unwrap();
        let identity2 = get_file_identity(&file2).unwrap();

        // Unix 系统下，不同文件有不同的 inode
        #[cfg(unix)]
        assert_ne!(identity1.value, identity2.value);
    }

    /// 测试 IdentityKind 字符串转换
    #[test]
    fn test_identity_kind_as_str() {
        #[cfg(unix)]
        assert_eq!(IdentityKind::Unix.as_str(), "unix");

        assert_eq!(IdentityKind::Path.as_str(), "path");
    }
}
