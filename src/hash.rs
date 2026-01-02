use crate::error::{DedupError, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const FAST_HASH_CHUNK_SIZE: u64 = 64 * 1024; // 64KB

/// Calculate fast hash: first 64KB + last 64KB (if file is large enough)
pub fn calculate_fast_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let metadata = file.metadata().map_err(DedupError::Io)?;
    let file_size = metadata.len();

    let mut hasher = blake3::Hasher::new();

    // Read first chunk
    let first_chunk_size = std::cmp::min(file_size, FAST_HASH_CHUNK_SIZE);
    let mut buffer = vec![0u8; first_chunk_size as usize];
    file.read_exact(&mut buffer).map_err(DedupError::Io)?;
    hasher.update(&buffer);

    // If file is large enough, also read last chunk
    if file_size > FAST_HASH_CHUNK_SIZE * 2 {
        file.seek(SeekFrom::End(-(FAST_HASH_CHUNK_SIZE as i64)))
            .map_err(DedupError::Io)?;
        let mut last_buffer = vec![0u8; FAST_HASH_CHUNK_SIZE as usize];
        file.read_exact(&mut last_buffer).map_err(DedupError::Io)?;
        hasher.update(&last_buffer);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Calculate full hash of entire file content
pub fn calculate_full_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let mut hasher = blake3::Hasher::new();

    // Use a larger buffer for better performance
    const BUFFER_SIZE: usize = 1024 * 1024; // 1MB
    let mut buffer = vec![0u8; BUFFER_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer).map_err(DedupError::Io)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Calculate MD5 hash (for compatibility with other tools)
#[allow(dead_code)]
pub fn calculate_md5(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(DedupError::Io)?;
    let mut context = md5::Context::new();

    const BUFFER_SIZE: usize = 1024 * 1024;
    let mut buffer = vec![0u8; BUFFER_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer).map_err(DedupError::Io)?;
        if bytes_read == 0 {
            break;
        }
        context.consume(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", context.compute()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_fast_hash_small_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();

        let hash = calculate_fast_hash(file.path()).unwrap();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_full_hash() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"hello world").unwrap();

        let hash = calculate_full_hash(file.path()).unwrap();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_same_content_same_hash() {
        let mut file1 = NamedTempFile::new().unwrap();
        let mut file2 = NamedTempFile::new().unwrap();

        file1.write_all(b"identical content").unwrap();
        file2.write_all(b"identical content").unwrap();

        let hash1 = calculate_full_hash(file1.path()).unwrap();
        let hash2 = calculate_full_hash(file2.path()).unwrap();

        assert_eq!(hash1, hash2);
    }
}
