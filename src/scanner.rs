use crate::error::{DedupError, Result};
use crate::identity::{get_file_identity, FileIdentity};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub mtime: i64,
    pub identity: FileIdentity,
}

pub struct Scanner {
    workers: usize,
}

impl Scanner {
    pub fn new(workers: usize) -> Self {
        Self { workers }
    }

    pub fn scan_directory(&self, root: &Path) -> Result<Vec<FileEntry>> {
        let root = root
            .canonicalize()
            .map_err(|e| DedupError::Path(format!("Cannot resolve path: {}", e)))?;

        // Collect all file paths first
        let paths: Vec<PathBuf> = WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_path_buf())
            .collect();

        // Configure rayon thread pool
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.workers)
            .build()
            .map_err(|e| DedupError::Io(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Process files in parallel
        let entries: Vec<FileEntry> = pool.install(|| {
            paths
                .par_iter()
                .filter_map(|path| self.process_file(path).ok())
                .collect()
        });

        Ok(entries)
    }

    fn process_file(&self, path: &Path) -> Result<FileEntry> {
        let metadata = std::fs::metadata(path).map_err(DedupError::Io)?;

        let mtime = metadata
            .modified()
            .map_err(DedupError::Io)?
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_scan_directory() {
        let temp_dir = TempDir::new().unwrap();

        // Create some test files
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
}
