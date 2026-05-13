//! CLI-side formatting for search/list results. Kept here (not in core) so
//! lore-core stays free of stdout/print concerns.

use lore_core::db::WebPageRow;
use lore_core::search::WebPageHit;

pub fn search_hits(query: &str, hits: &[WebPageHit]) {
    if hits.is_empty() {
        println!("No results for \"{}\"", query);
        return;
    }

    println!("Found {} results for \"{}\":\n", hits.len(), query);
    for (i, r) in hits.iter().enumerate() {
        println!(
            "{}. {} [{}]",
            i + 1,
            r.title.as_deref().unwrap_or("(no title)"),
            r.category
        );
        println!("   {}", r.url);
        if let Some(ref snip) = r.snippet
            && !snip.is_empty()
        {
            let display = if snip.len() > 300 {
                format!("{}...", &snip[..300])
            } else {
                snip.clone()
            };
            if let Some(pos) = display.find(">>>") {
                let start = pos.saturating_sub(80);
                let end = (pos + 200).min(display.len());
                println!("   ...{}...", &display[start..end]);
            } else {
                println!("   {}", &display[..display.len().min(200)]);
            }
        }
        println!();
    }
}

pub fn list_rows(rows: &[WebPageRow]) {
    if rows.is_empty() {
        println!("No pages found matching the filters.");
        return;
    }
    for row in rows {
        println!(
            "[{}] [{}] {}",
            row.category,
            row.status,
            row.title.as_deref().unwrap_or("(no title)")
        );
        println!("  ({})", row.domain);
    }
    eprintln!("\n{} entries shown", rows.len());
}
