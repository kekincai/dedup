use crate::error::{DedupError, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityKind {
    #[cfg(target_os = "windows")]
    Windows,
    #[cfg(unix)]
    Unix,
    Path,
}

impl IdentityKind {
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

#[derive(Debug, Clone)]
pub struct FileIdentity {
    pub kind: IdentityKind,
    pub value: String,
    pub source_id: String,
}

#[cfg(target_os = "windows")]
pub fn get_file_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_NORMAL,
        FILE_SHARE_READ, OPEN_EXISTING,
    };

    let metadata = std::fs::metadata(path).map_err(DedupError::Io)?;

    // Try to get Windows file ID
    let path_str = path.to_string_lossy();
    let wide_path: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();

    unsafe {
        use windows::Win32::Storage::FileSystem::CreateFileW;
        use windows::core::PCWSTR;

        let handle = CreateFileW(
            PCWSTR::from_raw(wide_path.as_ptr()),
            0, // No access needed for metadata
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

    // Fallback to path-based identity
    Ok(FileIdentity {
        kind: IdentityKind::Path,
        value: path.to_string_lossy().to_string(),
        source_id: get_source_id(path),
    })
}

#[cfg(unix)]
pub fn get_file_identity(path: &Path) -> Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = std::fs::metadata(path).map_err(DedupError::Io)?;

    let dev = metadata.dev();
    let ino = metadata.ino();

    // Check if this might be a network mount (heuristic)
    let source_id = get_source_id(path);
    let is_network = source_id.starts_with("smb://") || source_id.starts_with("nfs://");

    if is_network {
        // For network mounts, inode might not be stable
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

fn get_source_id(path: &Path) -> String {
    // Try to determine the mount point or volume
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    #[cfg(unix)]
    {
        // On Unix, try to find the mount point
        if let Some(root) = canonical.ancestors().last() {
            return root.to_string_lossy().to_string();
        }
    }

    #[cfg(windows)]
    {
        // On Windows, use the drive letter or UNC path
        if let Some(prefix) = canonical.components().next() {
            return prefix.as_os_str().to_string_lossy().to_string();
        }
    }

    "local".to_string()
}


