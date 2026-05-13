mod cli;

use std::io::{BufRead, BufReader};

use anyhow::{Context, Result};
use clap::Parser;
use cli::{Cli, Command};
use lore_core::{db, migrations, rules, search, version};
use url::Url;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db_path();

    match cli.command {
        Command::Add { urls, batch } => {
            let conn = db::open(&db_path)?;
            let classification_rules = db::load_rules(&conn)?;
            let mut count = 0u32;

            for raw in &urls {
                if add_url(&conn, raw, None, &classification_rules)? {
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
                    if add_url(&conn, url_str, title, &classification_rules)? {
                        count += 1;
                    }
                }
            }

            eprintln!("Added {} URLs", count);
        }
        Command::Search { query, limit } => {
            let conn = db::open(&db_path)?;
            search::search(&conn, &query, limit)?;
        }
        Command::List {
            category,
            status,
            domain,
            limit,
        } => {
            let conn = db::open(&db_path)?;
            search::list(
                &conn,
                category.as_deref(),
                status.as_deref(),
                domain.as_deref(),
                limit,
            )?;
        }
        Command::DbVersion => {
            // Open the file as a raw connection — don't run migrations or
            // refuse-on-newer logic, we just want to read the version.
            let conn = rusqlite::Connection::open(&db_path)
                .with_context(|| format!("opening database {}", db_path.display()))?;
            let current = migrations::current_version(&conn)
                .context("reading PRAGMA user_version")?;
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
            println!("DB at {} is now v{}", db_path.display(), migrations::EXPECTED_VERSION);
        }
    }

    Ok(())
}

fn add_url(
    conn: &rusqlite::Connection,
    raw_url: &str,
    title: Option<&str>,
    classification_rules: &[db::ClassificationRule],
) -> Result<bool> {
    let parsed = match Url::parse(raw_url) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("Invalid URL '{}': {}", raw_url, e);
            return Ok(false);
        }
    };

    let normalized = rules::normalize_url(&parsed);
    let domain = parsed.host_str().unwrap_or("unknown").to_string();
    let category = rules::classify(&parsed, classification_rules);

    let status = if category == "archive" {
        "queued"
    } else {
        "skipped"
    };

    let id = db::insert_web_page(
        conn,
        &db::NewWebPage {
            url: raw_url,
            url_normalized: &normalized,
            title,
            domain: &domain,
            category: &category,
            status,
            source: None,
            space_id: None,
        },
    )?;

    if id > 0 {
        eprintln!("[{}] {}", category, raw_url);
        Ok(true)
    } else {
        eprintln!("[exists] {}", raw_url);
        Ok(false)
    }
}
