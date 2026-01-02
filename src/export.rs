use crate::db::DuplicateGroup;
use crate::error::{DedupError, Result};
use serde::Serialize;
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
struct CsvRow {
    duplicate_group_id: usize,
    path: String,
    size: u64,
    hash: String,
    source_id: String,
}

#[derive(Serialize)]
struct JsonExport {
    groups: Vec<JsonGroup>,
    total_groups: usize,
    total_duplicate_files: usize,
    total_wasted_space: u64,
}

#[derive(Serialize)]
struct JsonGroup {
    group_id: usize,
    size: u64,
    hash: String,
    file_count: usize,
    files: Vec<JsonFile>,
}

#[derive(Serialize)]
struct JsonFile {
    path: String,
    source_id: String,
}

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

pub fn export_json(groups: &[DuplicateGroup], path: &Path) -> Result<()> {
    let total_groups = groups.len();
    let total_duplicate_files: usize = groups.iter().map(|g| g.files.len()).sum();

    // Wasted space = (file_count - 1) * size for each group
    let total_wasted_space: u64 = groups
        .iter()
        .map(|g| (g.files.len().saturating_sub(1) as u64) * g.size)
        .sum();

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
    let json = serde_json::to_string_pretty(&export)
        .map_err(|e| DedupError::Export(e.to_string()))?;

    file.write_all(json.as_bytes()).map_err(DedupError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DuplicateFile;
    use tempfile::TempDir;

    fn create_test_groups() -> Vec<DuplicateGroup> {
        vec![DuplicateGroup {
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
        }]
    }

    #[test]
    fn test_export_csv() {
        let temp_dir = TempDir::new().unwrap();
        let csv_path = temp_dir.path().join("test.csv");

        let groups = create_test_groups();
        export_csv(&groups, &csv_path).unwrap();

        assert!(csv_path.exists());
    }

    #[test]
    fn test_export_json() {
        let temp_dir = TempDir::new().unwrap();
        let json_path = temp_dir.path().join("test.json");

        let groups = create_test_groups();
        export_json(&groups, &json_path).unwrap();

        assert!(json_path.exists());

        let content = std::fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(parsed["total_groups"], 1);
        assert_eq!(parsed["total_duplicate_files"], 2);
    }
}
