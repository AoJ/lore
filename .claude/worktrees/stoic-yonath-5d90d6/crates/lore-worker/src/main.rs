mod archive;
mod render;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "lore-worker", about = "Archive worker for lore")]
struct Cli {
    #[arg(long, env = "LORE_DB")]
    db: String,

    /// Specific URL to archive
    url: Option<String>,

    /// Max pages to process from queue
    #[arg(long, default_value = "10")]
    limit: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = std::path::PathBuf::from(&cli.db);
    let conn = lore_core::db::open(&db_path)?;

    if let Some(url) = cli.url {
        archive::archive_url(&conn, &url)?;
    } else {
        archive::archive_queued(&conn, cli.limit)?;
    }

    Ok(())
}
