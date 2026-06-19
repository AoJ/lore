use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lore", version, about = "Personal knowledge management")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Path to SQLite database
    #[arg(long, global = true, env = "LORE_DB")]
    pub db: Option<PathBuf>,
}

impl Cli {
    pub fn db_path(&self) -> PathBuf {
        if let Some(ref p) = self.db {
            return p.clone();
        }
        let data_dir = dirs_path();
        data_dir.join("lore.db")
    }
}

fn dirs_path() -> PathBuf {
    if let Some(dir) = dirs::data_local_dir() {
        let p = dir.join("lore");
        std::fs::create_dir_all(&p).ok();
        return p;
    }
    PathBuf::from(".")
}

#[derive(Subcommand)]
pub enum Command {
    /// Add URLs to the archive queue
    Add {
        /// URLs to add
        urls: Vec<String>,
        /// Read URLs from file (one per line, optionally URL<TAB>TITLE)
        #[arg(long)]
        batch: Option<PathBuf>,
    },
    /// Import a folder of markdown files as notes (idempotent — re-import syncs
    /// changed files, skips unchanged, errors on notes edited in lore)
    Import {
        /// Directory of `.md` files to import (subfolders become note folders)
        dir: PathBuf,
        /// Space to import into, by name (required)
        #[arg(long)]
        space: String,
        /// Top-level folder name (default: the import directory's name)
        #[arg(long)]
        folder: Option<String>,
        /// Trash imported notes under this folder whose source file is gone
        #[arg(long)]
        prune: bool,
        /// Preview what would change without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Export a space's notes as a folder of markdown files (inverse of import)
    Export {
        /// Output directory (created if missing; existing files overwritten)
        dir: PathBuf,
        /// Space to export, by name (required)
        #[arg(long)]
        space: String,
        /// Limit to a folder subtree, by name (default: whole space)
        #[arg(long)]
        folder: Option<String>,
        /// Report what would be written without writing anything
        #[arg(long)]
        dry_run: bool,
    },
    /// Full-text search across archived pages
    Search {
        /// Search query (FTS5 syntax)
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },
    /// List pages with filters
    List {
        /// Filter by category
        #[arg(short, long)]
        category: Option<String>,
        /// Filter by status
        #[arg(short, long)]
        status: Option<String>,
        /// Filter by domain (partial match)
        #[arg(short, long)]
        domain: Option<String>,
        /// Maximum results
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },
    /// Print DB schema version (current and expected by this build)
    DbVersion,
    /// Apply pending migrations and exit (no UI, no seed work beyond what
    /// db::open already does)
    Migrate,
}
