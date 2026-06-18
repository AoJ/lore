mod cli;
mod print;

use std::io::{BufRead, BufReader};

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command};
use lore_core::{db, migrations, search, version};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db_path();

    match cli.command {
        Command::Add { urls, batch } => {
            let conn = db::open(&db_path)?;
            let mut count = 0u32;

            for raw in &urls {
                if add_one(&conn, raw, None) {
                    count += 1;
                }
            }

            if let Some(ref path) = batch {
                let file = std::fs::File::open(path)
                    .with_context(|| format!("opening {}", path.display()))?;
                for line in BufReader::new(file).lines() {
                    let line = line?;
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    let (url_str, title) = match trimmed.split_once('\t') {
                        Some((u, t)) => (u.trim(), Some(t.trim())),
                        None => (trimmed, None),
                    };
                    if add_one(&conn, url_str, title) {
                        count += 1;
                    }
                }
            }

            eprintln!("Added {} URLs", count);
        }
        Command::Import {
            dir,
            space,
            folder,
            dry_run,
        } => {
            let mut conn = db::open(&db_path)?;
            let spaces = db::list_all_spaces(&conn)?;
            let space_row = spaces
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(&space))
                .ok_or_else(|| {
                    let names: Vec<&str> = spaces.iter().map(|s| s.name.as_str()).collect();
                    anyhow::anyhow!(
                        "space '{}' not found. Available: {}",
                        space,
                        names.join(", ")
                    )
                })?;
            let root_folder = folder.unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "import".to_string())
            });
            let report = lore_core::import_md::import_markdown_dir(
                &mut conn,
                &dir,
                space_row.id,
                &root_folder,
                dry_run,
            )?;
            let prefix = if dry_run { "[dry-run] " } else { "" };
            eprintln!(
                "{}space '{}' / folder '{}': {} new, {} updated, {} unchanged",
                prefix,
                space_row.name,
                root_folder,
                report.inserted,
                report.updated,
                report.skipped
            );
            // Real imports abort (Err) on conflict; dry runs report them here.
            if !report.conflicts.is_empty() {
                eprintln!(
                    "{} conflict(s) (edited in lore, source differs):",
                    report.conflicts.len()
                );
                for c in &report.conflicts {
                    eprintln!("  {}", c);
                }
            }
        }
        Command::Search { query, limit } => {
            let conn = db::open(&db_path)?;
            // CLI searches across all spaces by default — pick the active one
            // so the result matches what the desktop app would show.
            let active = db::get_active_space(&conn)?;
            let hits = search::search_web_pages(&conn, &query, active.id, limit)?;
            print::search_hits(&query, &hits);
        }
        Command::List {
            category,
            status,
            domain,
            limit,
        } => {
            let conn = db::open(&db_path)?;
            let rows = search::list_pages_filtered(
                &conn,
                None,
                category.as_deref(),
                status.as_deref(),
                domain.as_deref(),
                limit,
            )?;
            print::list_rows(&rows);
        }
        Command::DbVersion => {
            // Open the file as a raw connection — don't run migrations or
            // refuse-on-newer logic, we just want to read the version.
            let conn = rusqlite::Connection::open(&db_path)
                .with_context(|| format!("opening database {}", db_path.display()))?;
            let current =
                migrations::current_version(&conn).context("reading PRAGMA user_version")?;
            println!("lore       {}", version::full());
            println!("DB path    {}", db_path.display());
            println!("DB version {}", current);
            println!("expected   {}", migrations::EXPECTED_VERSION);
            if current > migrations::EXPECTED_VERSION {
                println!("status     newer than this build (refused)");
            } else if current < migrations::EXPECTED_VERSION {
                println!("status     pending migrations");
            } else {
                println!("status     up to date");
            }
        }
        Command::Migrate => {
            // db::open() runs migrations as a side effect; this gives us
            // exactly what we want without spinning up the rest of the app.
            let _ = db::open(&db_path)?;
            println!(
                "DB at {} is now v{}",
                db_path.display(),
                migrations::EXPECTED_VERSION
            );
        }
    }

    Ok(())
}

/// `lore add` row handler — best-effort, prints status to stderr.
fn add_one(conn: &rusqlite::Connection, raw_url: &str, title: Option<&str>) -> bool {
    match db::archive_url(conn, raw_url, None, title, None) {
        Ok(outcome) => {
            eprintln!("[{}] {}", outcome.category, raw_url);
            true
        }
        Err(e) => {
            eprintln!("Invalid URL '{}': {}", raw_url, e);
            false
        }
    }
}
