use crate::error::Result;
use crate::hash::{calculate_fast_hash, calculate_full_hash};
use crate::scanner::FileEntry;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct Database {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct DuplicateFile {
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub source_id: String,
}

#[derive(Debug)]
pub struct DuplicateGroup {
    pub group_id: usize,
    pub size: u64,
    pub hash: String,
    pub files: Vec<DuplicateFile>,
}

#[derive(Debug, Default)]
pub struct DbStats {
    pub total_files: u64,
    pub total_size: u64,
    pub with_fast_hash: u64,
    pub with_full_hash: u64,
    pub fast_dup_groups: u64,
    pub full_dup_groups: u64,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        Ok(Self { conn })
    }

    pub fn init(&mut self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                platform TEXT NOT NULL,
                source_id TEXT NOT NULL,
                identity_kind TEXT NOT NULL,
                identity_value TEXT NOT NULL,
                path TEXT NOT NULL,
                name TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime INTEGER NOT NULL,
                fast_hash TEXT,
                full_hash TEXT,
                last_scanned_at INTEGER NOT NULL,
                UNIQUE(platform, source_id, identity_kind, identity_value)
            );

            CREATE INDEX IF NOT EXISTS idx_files_size ON files(size);
            CREATE INDEX IF NOT EXISTS idx_files_fast_hash ON files(fast_hash) WHERE fast_hash IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_files_full_hash ON files(full_hash) WHERE full_hash IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
            "#,
        )?;

        Ok(())
    }

    fn current_platform() -> &'static str {
        #[cfg(target_os = "windows")]
        return "win";
        #[cfg(target_os = "macos")]
        return "mac";
        #[cfg(target_os = "linux")]
        return "linux";
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return "other";
    }

    pub fn upsert_file(
        &mut self,
        entry: &FileEntry,
        compute_fast_hash: bool,
        compute_full_hash: bool,
    ) -> Result<()> {
        let platform = Self::current_platform();
        let now = chrono::Utc::now().timestamp();

        let fast_hash = if compute_fast_hash {
            match calculate_fast_hash(&entry.path) {
                Ok(h) => Some(h),
                Err(e) => {
                    log::warn!("Failed to calculate fast hash for {:?}: {}", entry.path, e);
                    None
                }
            }
        } else {
            None
        };

        let full_hash = if compute_full_hash {
            match calculate_full_hash(&entry.path) {
                Ok(h) => Some(h),
                Err(e) => {
                    log::warn!("Failed to calculate full hash for {:?}: {}", entry.path, e);
                    None
                }
            }
        } else {
            None
        };

        self.conn.execute(
            r#"
            INSERT INTO files (platform, source_id, identity_kind, identity_value, path, name, size, mtime, fast_hash, full_hash, last_scanned_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(platform, source_id, identity_kind, identity_value) DO UPDATE SET
                path = excluded.path,
                name = excluded.name,
                size = excluded.size,
                mtime = excluded.mtime,
                fast_hash = COALESCE(excluded.fast_hash, files.fast_hash),
                full_hash = COALESCE(excluded.full_hash, files.full_hash),
                last_scanned_at = excluded.last_scanned_at
            "#,
            params![
                platform,
                entry.identity.source_id,
                entry.identity.kind.as_str(),
                entry.identity.value,
                entry.path.to_string_lossy().to_string(),
                entry.name,
                entry.size as i64,
                entry.mtime,
                fast_hash,
                full_hash,
                now,
            ],
        )?;

        Ok(())
    }

    pub fn needs_update(&self, entry: &FileEntry) -> Result<bool> {
        let platform = Self::current_platform();

        let existing: Option<(i64, i64)> = self
            .conn
            .query_row(
                r#"
                SELECT size, mtime FROM files
                WHERE platform = ?1 AND source_id = ?2 AND identity_kind = ?3 AND identity_value = ?4
                "#,
                params![
                    platform,
                    entry.identity.source_id,
                    entry.identity.kind.as_str(),
                    entry.identity.value,
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        match existing {
            Some((size, mtime)) => {
                Ok(size != entry.size as i64 || mtime != entry.mtime)
            }
            None => Ok(true),
        }
    }

    pub fn touch_file(&self, entry: &FileEntry) -> Result<()> {
        let platform = Self::current_platform();
        let now = chrono::Utc::now().timestamp();

        self.conn.execute(
            r#"
            UPDATE files SET last_scanned_at = ?1, path = ?2
            WHERE platform = ?3 AND source_id = ?4 AND identity_kind = ?5 AND identity_value = ?6
            "#,
            params![
                now,
                entry.path.to_string_lossy().to_string(),
                platform,
                entry.identity.source_id,
                entry.identity.kind.as_str(),
                entry.identity.value,
            ],
        )?;

        Ok(())
    }

    pub fn file_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    pub fn find_duplicates_by_full_hash(&self, min_size: u64) -> Result<Vec<DuplicateGroup>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT full_hash, size, path, source_id
            FROM files
            WHERE full_hash IS NOT NULL AND size >= ?1
            AND full_hash IN (
                SELECT full_hash FROM files
                WHERE full_hash IS NOT NULL AND size >= ?1
                GROUP BY full_hash
                HAVING COUNT(*) > 1
            )
            ORDER BY full_hash, path
            "#,
        )?;

        let rows = stmt.query_map([min_size as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut groups: Vec<DuplicateGroup> = Vec::new();
        let mut current_hash: Option<String> = None;
        let mut group_id = 0;

        for row in rows {
            let (hash, size, path, source_id) = row?;

            if current_hash.as_ref() != Some(&hash) {
                group_id += 1;
                groups.push(DuplicateGroup {
                    group_id,
                    size,
                    hash: hash.clone(),
                    files: Vec::new(),
                });
                current_hash = Some(hash.clone());
            }

            if let Some(group) = groups.last_mut() {
                group.files.push(DuplicateFile {
                    path,
                    size,
                    hash,
                    source_id,
                });
            }
        }

        Ok(groups)
    }

    pub fn find_duplicates_by_fast_hash(&self, min_size: u64) -> Result<Vec<DuplicateGroup>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT fast_hash, size, path, source_id
            FROM files
            WHERE fast_hash IS NOT NULL AND size >= ?1
            AND (size, fast_hash) IN (
                SELECT size, fast_hash FROM files
                WHERE fast_hash IS NOT NULL AND size >= ?1
                GROUP BY size, fast_hash
                HAVING COUNT(*) > 1
            )
            ORDER BY size DESC, fast_hash, path
            "#,
        )?;

        let rows = stmt.query_map([min_size as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut groups: Vec<DuplicateGroup> = Vec::new();
        let mut current_key: Option<(u64, String)> = None;
        let mut group_id = 0;

        for row in rows {
            let (hash, size, path, source_id) = row?;
            let key = (size, hash.clone());

            if current_key.as_ref() != Some(&key) {
                group_id += 1;
                groups.push(DuplicateGroup {
                    group_id,
                    size,
                    hash: hash.clone(),
                    files: Vec::new(),
                });
                current_key = Some(key);
            }

            if let Some(group) = groups.last_mut() {
                group.files.push(DuplicateFile {
                    path,
                    size,
                    hash,
                    source_id,
                });
            }
        }

        Ok(groups)
    }

    pub fn get_stats(&self) -> Result<DbStats> {
        let total_files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;

        let total_size: i64 = self
            .conn
            .query_row("SELECT COALESCE(SUM(size), 0) FROM files", [], |row| {
                row.get(0)
            })?;

        let with_fast_hash: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE fast_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )?;

        let with_full_hash: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE full_hash IS NOT NULL",
            [],
            |row| row.get(0),
        )?;

        let fast_dup_groups: i64 = self.conn.query_row(
            r#"
            SELECT COUNT(*) FROM (
                SELECT 1 FROM files
                WHERE fast_hash IS NOT NULL
                GROUP BY size, fast_hash
                HAVING COUNT(*) > 1
            )
            "#,
            [],
            |row| row.get(0),
        )?;

        let full_dup_groups: i64 = self.conn.query_row(
            r#"
            SELECT COUNT(*) FROM (
                SELECT 1 FROM files
                WHERE full_hash IS NOT NULL
                GROUP BY full_hash
                HAVING COUNT(*) > 1
            )
            "#,
            [],
            |row| row.get(0),
        )?;

        Ok(DbStats {
            total_files: total_files as u64,
            total_size: total_size as u64,
            with_fast_hash: with_fast_hash as u64,
            with_full_hash: with_full_hash as u64,
            fast_dup_groups: fast_dup_groups as u64,
            full_dup_groups: full_dup_groups as u64,
        })
    }
}
