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

    // Non-zero exit codes:
    //   1 — at least one page ended up as `failed` (no data stored)
    //   2 — Chrome blew up and we degraded to HTTP fallback (data stored,
    //       but missing JS/screenshot — user probably wants to know)
    // Both states would otherwise hide under exit 0 + a cheerful "Done" line.
    let exit_code = if let Some(url) = cli.url {
        match archive::archive_url(&conn, &url)? {
            archive::ArchiveOutcome::Ok => 0,
            archive::ArchiveOutcome::Degraded => 2,
            archive::ArchiveOutcome::Failed => 1,
        }
    } else {
        let summary = archive::archive_queued(&conn, cli.limit)?;
        if summary.failed > 0 {
            1
        } else if summary.degraded > 0 {
            2
        } else {
            0
        }
    };

    std::process::exit(exit_code);
}
