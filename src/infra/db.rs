//! # 数据库模块
//!
//! 使用 SQLite 存储文件索引信息
//! 支持增量更新和查重查询

use crate::core::{
    calculate_fast_hash, calculate_full_hash, DuplicateFile, DuplicateGroup, FileEntry,
};
use crate::infra::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

// ============================================================================
// 数据结构定义
// ============================================================================

/// 数据库连接封装
pub struct Database {
    conn: Connection,
}

/// 数据库统计信息
#[derive(Debug, Default)]
pub struct DbStats {
    /// 文件总数
    pub total_files: u64,
    /// 总大小（字节）
    pub total_size: u64,
    /// 已计算 fast_hash 的文件数
    pub with_fast_hash: u64,
    /// 已计算 full_hash 的文件数
    pub with_full_hash: u64,
    /// 疑似重复组数（按 fast hash）
    pub fast_dup_groups: u64,
    /// 精确重复组数（按 full hash）
    pub full_dup_groups: u64,
}

// ============================================================================
// Database 实现
// ============================================================================

impl Database {
    /// 打开数据库
    ///
    /// 如果数据库文件不存在，会自动创建
    ///
    /// # 参数
    /// - `path`: 数据库文件路径
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // 启用 WAL 模式，提高并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        Ok(Self { conn })
    }

    /// 初始化数据库表结构
    ///
    /// 创建 files 表和必要的索引
    pub fn init(&mut self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- 文件索引表
            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                -- 平台标识 (win/mac/linux)
                platform TEXT NOT NULL,
                -- 来源标识 (卷/设备/挂载点)
                source_id TEXT NOT NULL,
                -- 身份类型 (win/unix/path)
                identity_kind TEXT NOT NULL,
                -- 身份值
                identity_value TEXT NOT NULL,
                -- 文件完整路径
                path TEXT NOT NULL,
                -- 文件名
                name TEXT NOT NULL,
                -- 文件大小 (字节)
                size INTEGER NOT NULL,
                -- 修改时间 (Unix 时间戳)
                mtime INTEGER NOT NULL,
                -- 快速哈希 (前后 64KB)
                fast_hash TEXT,
                -- 完整哈希
                full_hash TEXT,
                -- 最后扫描时间
                last_scanned_at INTEGER NOT NULL,
                -- 唯一约束：同一平台、来源、身份的文件只能有一条记录
                UNIQUE(platform, source_id, identity_kind, identity_value)
            );

            -- 索引：按大小查询（用于查重）
            CREATE INDEX IF NOT EXISTS idx_files_size ON files(size);
            -- 索引：按 fast_hash 查询
            CREATE INDEX IF NOT EXISTS idx_files_fast_hash ON files(fast_hash) WHERE fast_hash IS NOT NULL;
            -- 索引：按 full_hash 查询
            CREATE INDEX IF NOT EXISTS idx_files_full_hash ON files(full_hash) WHERE full_hash IS NOT NULL;
            -- 索引：按路径查询
            CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
            "#,
        )?;

        Ok(())
    }

    /// 获取当前平台标识
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

    /// 插入或更新文件记录
    ///
    /// 如果文件已存在（根据身份标识），则更新记录
    /// 否则插入新记录
    ///
    /// # 参数
    /// - `entry`: 文件条目
    /// - `compute_fast_hash`: 是否计算 fast hash
    /// - `compute_full_hash`: 是否计算 full hash
    pub fn upsert_file(
        &mut self,
        entry: &FileEntry,
        compute_fast_hash: bool,
        compute_full_hash: bool,
    ) -> Result<()> {
        let platform = Self::current_platform();
        let now = chrono::Utc::now().timestamp();

        // 计算 fast hash（如果需要）
        let fast_hash = if compute_fast_hash {
            match calculate_fast_hash(&entry.path) {
                Ok(h) => Some(h),
                Err(e) => {
                    log::warn!("计算 fast hash 失败 {:?}: {}", entry.path, e);
                    None
                }
            }
        } else {
            None
        };

        // 计算 full hash（如果需要）
        let full_hash = if compute_full_hash {
            match calculate_full_hash(&entry.path) {
                Ok(h) => Some(h),
                Err(e) => {
                    log::warn!("计算 full hash 失败 {:?}: {}", entry.path, e);
                    None
                }
            }
        } else {
            None
        };

        // 使用 UPSERT 语法插入或更新
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

    /// 检查文件是否需要更新
    ///
    /// 比较文件的 size 和 mtime，判断是否发生变化
    ///
    /// # 返回
    /// - `true`: 文件是新的或已变化，需要更新
    /// - `false`: 文件未变化，可以跳过
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
                // 文件存在，检查是否变化
                Ok(size != entry.size as i64 || mtime != entry.mtime)
            }
            None => {
                // 文件不存在，需要添加
                Ok(true)
            }
        }
    }

    /// 更新文件的最后扫描时间
    ///
    /// 用于增量扫描时标记文件仍然存在
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

    /// 获取文件总数
    pub fn file_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// 按 full hash 查找重复文件
    ///
    /// 返回内容完全相同的文件组
    ///
    /// # 参数
    /// - `min_size`: 最小文件大小过滤
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

        self.collect_duplicate_groups(&mut stmt, min_size, false)
    }

    /// 按 fast hash 查找疑似重复文件
    ///
    /// 返回 size + fast_hash 相同的文件组
    /// 这些文件可能是重复的，需要进一步用 full hash 确认
    ///
    /// # 参数
    /// - `min_size`: 最小文件大小过滤
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

        self.collect_duplicate_groups(&mut stmt, min_size, true)
    }

    /// 收集重复文件组
    fn collect_duplicate_groups(
        &self,
        stmt: &mut rusqlite::Statement,
        min_size: u64,
        group_by_size: bool,
    ) -> Result<Vec<DuplicateGroup>> {
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
            let key = if group_by_size {
                (size, hash.clone())
            } else {
                (0, hash.clone())
            };

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

    /// 获取数据库统计信息
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

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::identity::{FileIdentity, IdentityKind};
    use tempfile::TempDir;

    /// 创建测试用的 FileEntry
    fn create_test_entry(name: &str, size: u64) -> FileEntry {
        FileEntry {
            path: std::path::PathBuf::from(format!("/test/{}", name)),
            name: name.to_string(),
            size,
            mtime: 1234567890,
            identity: FileIdentity {
                #[cfg(unix)]
                kind: IdentityKind::Unix,
                #[cfg(windows)]
                kind: IdentityKind::Windows,
                #[cfg(not(any(unix, windows)))]
                kind: IdentityKind::Path,
                value: format!("test:{}", name),
                source_id: "test".to_string(),
            },
        }
    }

    /// 测试数据库初始化
    #[test]
    fn test_database_init() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let mut db = Database::open(&db_path).unwrap();
        db.init().unwrap();

        // 验证表已创建
        let count = db.file_count().unwrap();
        assert_eq!(count, 0);
    }

    /// 测试插入文件
    #[test]
    fn test_upsert_file() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let mut db = Database::open(&db_path).unwrap();
        db.init().unwrap();

        let entry = create_test_entry("file1.txt", 100);
        db.upsert_file(&entry, false, false).unwrap();

        assert_eq!(db.file_count().unwrap(), 1);
    }

    /// 测试增量更新检测
    #[test]
    fn test_needs_update() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let mut db = Database::open(&db_path).unwrap();
        db.init().unwrap();

        let entry = create_test_entry("file1.txt", 100);

        // 新文件需要更新
        assert!(db.needs_update(&entry).unwrap());

        // 插入后不需要更新
        db.upsert_file(&entry, false, false).unwrap();
        assert!(!db.needs_update(&entry).unwrap());

        // 修改大小后需要更新
        let mut modified_entry = entry.clone();
        modified_entry.size = 200;
        assert!(db.needs_update(&modified_entry).unwrap());
    }

    /// 测试统计信息
    #[test]
    fn test_get_stats() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let mut db = Database::open(&db_path).unwrap();
        db.init().unwrap();

        // 插入测试数据
        db.upsert_file(&create_test_entry("file1.txt", 100), false, false)
            .unwrap();
        db.upsert_file(&create_test_entry("file2.txt", 200), false, false)
            .unwrap();

        let stats = db.get_stats().unwrap();

        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.total_size, 300);
    }
}
