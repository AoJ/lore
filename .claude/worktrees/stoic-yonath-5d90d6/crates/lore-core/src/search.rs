use anyhow::Result;
use rusqlite::Connection;

pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<()> {
    let query = prepare_query(query);

    let mut stmt = conn.prepare(
        "SELECT wp.url, wp.title, wp.domain, wp.category,
                highlight(web_page_fts, 1, '>>>', '<<<') as snip
         FROM web_page_fts fts
         JOIN web_page_snapshot wps ON wps.id = fts.rowid
         JOIN web_page wp ON wp.id = wps.web_page_id
         WHERE web_page_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;

    let results: Vec<SearchResult> = stmt
        .query_map(rusqlite::params![&query, limit as i64], |row| {
            Ok(SearchResult {
                url: row.get(0)?,
                title: row.get(1)?,
                _domain: row.get(2)?,
                category: row.get(3)?,
                snippet: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    if results.is_empty() {
        println!("No results for \"{}\"", query);
        return Ok(());
    }

    println!("Found {} results for \"{}\":\n", results.len(), query);
    for (i, r) in results.iter().enumerate() {
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
            // Show first 300 chars of highlighted snippet
            let display = if snip.len() > 300 {
                format!("{}...", &snip[..300])
            } else {
                snip.clone()
            };
            // Show just the part around the first highlight
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

    Ok(())
}

pub fn list(
    conn: &Connection,
    category: Option<&str>,
    status: Option<&str>,
    domain: Option<&str>,
    limit: usize,
) -> Result<()> {
    let mut sql = String::from(
        "SELECT url, title, domain, category, status, created_at FROM web_page WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut param_idx = 1;

    if let Some(cat) = category {
        sql.push_str(&format!(" AND category = ?{}", param_idx));
        params.push(Box::new(cat.to_string()));
        param_idx += 1;
    }
    if let Some(st) = status {
        sql.push_str(&format!(" AND status = ?{}", param_idx));
        params.push(Box::new(st.to_string()));
        param_idx += 1;
    }
    if let Some(dom) = domain {
        sql.push_str(&format!(" AND domain LIKE ?{}", param_idx));
        params.push(Box::new(format!("%{}%", dom)));
        param_idx += 1;
    }

    sql.push_str(&format!(" ORDER BY created_at DESC LIMIT ?{}", param_idx));
    params.push(Box::new(limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(ListRow {
            url: row.get(0)?,
            title: row.get(1)?,
            domain: row.get(2)?,
            category: row.get(3)?,
            status: row.get(4)?,
        })
    })?;

    let mut count = 0;
    for row in rows {
        let row = row?;
        count += 1;
        println!(
            "[{}] [{}] {}",
            row.category,
            row.status,
            row.title.as_deref().unwrap_or("(no title)")
        );
        println!("  {} ({})", row.url, row.domain);
    }

    if count == 0 {
        println!("No pages found matching the filters.");
    } else {
        eprintln!("\n{} entries shown", count);
    }

    Ok(())
}

struct SearchResult {
    url: String,
    title: Option<String>,
    _domain: String,
    category: String,
    snippet: Option<String>,
}

struct ListRow {
    url: String,
    title: Option<String>,
    domain: String,
    category: String,
    status: String,
}

/// Auto-append * to simple queries for prefix matching.
/// Leaves FTS5 operators untouched.
fn prepare_query(query: &str) -> String {
    let trimmed = query.trim();
    // If it already has FTS5 operators, leave as-is
    if trimmed.contains('*')
        || trimmed.contains('"')
        || trimmed.contains(" AND ")
        || trimmed.contains(" OR ")
        || trimmed.contains(" NOT ")
        || trimmed.contains(':')
    {
        return trimmed.to_string();
    }
    // Add * to each word for prefix matching
    trimmed
        .split_whitespace()
        .map(|w| format!("{}*", w))
        .collect::<Vec<_>>()
        .join(" ")
}
