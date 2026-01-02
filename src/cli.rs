use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use crate::db::Database;
use crate::export::{export_csv, export_json};
use crate::scanner::Scanner;

#[derive(Parser)]
#[command(name = "dedup")]
#[command(about = "Cross-platform file indexing and deduplication tool")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Build initial index for a directory
    Index(IndexArgs),
    /// Incrementally update existing index
    Update(UpdateArgs),
    /// Find and display duplicate files
    Dup(DupArgs),
    /// Export duplicate results to file
    Export(ExportArgs),
    /// Show database statistics
    Stats(StatsArgs),
}

#[derive(Parser)]
pub struct IndexArgs {
    /// Directory path to index
    pub path: PathBuf,
    /// Database file path
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// Calculate fast hash (first/last 64KB)
    #[arg(long)]
    pub fast_hash: bool,
    /// Calculate full hash (entire file)
    #[arg(long)]
    pub full_hash: bool,
    /// Number of parallel workers
    #[arg(long, default_value = "4")]
    pub workers: usize,
}

#[derive(Parser)]
pub struct UpdateArgs {
    /// Directory path to update
    pub path: PathBuf,
    /// Database file path
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// Calculate fast hash for new/changed files
    #[arg(long)]
    pub fast_hash: bool,
    /// Calculate full hash for new/changed files
    #[arg(long)]
    pub full_hash: bool,
    /// Number of parallel workers
    #[arg(long, default_value = "4")]
    pub workers: usize,
}

#[derive(Parser)]
pub struct DupArgs {
    /// Database file path
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// Deduplication mode
    #[arg(long, value_enum, default_value = "full")]
    pub by: DupMode,
    /// Minimum file size to consider
    #[arg(long)]
    pub min_size: Option<String>,
}

#[derive(Parser)]
pub struct ExportArgs {
    /// Database file path
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
    /// Deduplication mode
    #[arg(long, value_enum, default_value = "full")]
    pub by: DupMode,
    /// Export to CSV file
    #[arg(long)]
    pub csv: Option<PathBuf>,
    /// Export to JSON file
    #[arg(long)]
    pub json: Option<PathBuf>,
    /// Minimum file size to consider
    #[arg(long)]
    pub min_size: Option<String>,
}

#[derive(Parser)]
pub struct StatsArgs {
    /// Database file path
    #[arg(long, default_value = "dedup.db")]
    pub db: PathBuf,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum DupMode {
    /// Match by full hash (exact content match)
    Full,
    /// Match by size + fast hash (potential duplicates)
    Fast,
}

fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    let (num_str, multiplier) = if s.ends_with("GB") {
        (&s[..s.len() - 2], 1024 * 1024 * 1024)
    } else if s.ends_with("MB") {
        (&s[..s.len() - 2], 1024 * 1024)
    } else if s.ends_with("KB") {
        (&s[..s.len() - 2], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    let num: u64 = num_str.trim().parse()?;
    Ok(num * multiplier)
}

fn create_progress_bar(total: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{msg} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

pub fn cmd_index(args: IndexArgs) -> Result<()> {
    println!("Indexing: {:?}", args.path);

    let mut db = Database::open(&args.db)?;
    db.init()?;

    let scanner = Scanner::new(args.workers);
    let entries = scanner.scan_directory(&args.path)?;

    let pb = create_progress_bar(entries.len() as u64, "Indexing files");

    for entry in entries {
        db.upsert_file(&entry, args.fast_hash, args.full_hash)?;
        pb.inc(1);
    }

    pb.finish_with_message("Indexing complete");
    println!("Indexed {} files", db.file_count()?);

    Ok(())
}

pub fn cmd_update(args: UpdateArgs) -> Result<()> {
    println!("Updating index: {:?}", args.path);

    let mut db = Database::open(&args.db)?;

    let scanner = Scanner::new(args.workers);
    let entries = scanner.scan_directory(&args.path)?;

    let pb = create_progress_bar(entries.len() as u64, "Updating files");

    let mut updated = 0;
    let mut skipped = 0;

    for entry in entries {
        if db.needs_update(&entry)? {
            db.upsert_file(&entry, args.fast_hash, args.full_hash)?;
            updated += 1;
        } else {
            db.touch_file(&entry)?;
            skipped += 1;
        }
        pb.inc(1);
    }

    pb.finish_with_message("Update complete");
    println!("Updated: {}, Skipped: {}", updated, skipped);

    Ok(())
}

pub fn cmd_dup(args: DupArgs) -> Result<()> {
    let db = Database::open(&args.db)?;

    let min_size = args
        .min_size
        .map(|s| parse_size(&s))
        .transpose()?
        .unwrap_or(0);

    let groups = match args.by {
        DupMode::Full => db.find_duplicates_by_full_hash(min_size)?,
        DupMode::Fast => db.find_duplicates_by_fast_hash(min_size)?,
    };

    if groups.is_empty() {
        println!("No duplicates found.");
        return Ok(());
    }

    println!("Found {} duplicate groups:\n", groups.len());

    for (i, group) in groups.iter().enumerate() {
        println!("Group {} ({} files, {} each):", i + 1, group.files.len(), bytesize::ByteSize(group.size));
        for file in &group.files {
            println!("  {}", file.path);
        }
        println!();
    }

    Ok(())
}

pub fn cmd_export(args: ExportArgs) -> Result<()> {
    let db = Database::open(&args.db)?;

    let min_size = args
        .min_size
        .map(|s| parse_size(&s))
        .transpose()?
        .unwrap_or(0);

    let groups = match args.by {
        DupMode::Full => db.find_duplicates_by_full_hash(min_size)?,
        DupMode::Fast => db.find_duplicates_by_fast_hash(min_size)?,
    };

    if let Some(csv_path) = args.csv {
        export_csv(&groups, &csv_path)?;
        println!("Exported to CSV: {:?}", csv_path);
    }

    if let Some(json_path) = args.json {
        export_json(&groups, &json_path)?;
        println!("Exported to JSON: {:?}", json_path);
    }

    Ok(())
}

pub fn cmd_stats(args: StatsArgs) -> Result<()> {
    let db = Database::open(&args.db)?;
    let stats = db.get_stats()?;

    println!("Database Statistics:");
    println!("  Total files: {}", stats.total_files);
    println!("  Total size: {}", bytesize::ByteSize(stats.total_size));
    println!("  Files with fast_hash: {}", stats.with_fast_hash);
    println!("  Files with full_hash: {}", stats.with_full_hash);
    println!("  Potential duplicate groups (fast): {}", stats.fast_dup_groups);
    println!("  Exact duplicate groups (full): {}", stats.full_dup_groups);

    Ok(())
}
