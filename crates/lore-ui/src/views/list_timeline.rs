use dioxus::prelude::*;
use crate::store::DataStore;
use std::collections::HashMap;

#[component]
pub fn ListTimeline() -> Element {
    let store = use_context::<DataStore>();
    let heatmap = store.heatmap.read();

    // Build lookup map
    let day_counts: HashMap<&str, i64> = heatmap.iter()
        .map(|(day, count)| (day.as_str(), *count))
        .collect();

    // Generate 91 days grid (13 weeks × 7 days)
    // Start from today, go back 90 days
    let today = chrono::Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let mut weeks: Vec<Vec<(String, i64)>> = Vec::new();

    // Align to start of week (Monday)
    let days_since_monday = today.format("%u").to_string().parse::<i64>().unwrap_or(1) - 1; // 1=Mon..7=Sun
    let grid_end = today;
    let grid_start = grid_end - chrono::Duration::days(30 + days_since_monday);

    let mut current = grid_start;
    let mut week: Vec<(String, i64)> = Vec::new();

    while current <= grid_end {
        let day_str = current.format("%Y-%m-%d").to_string();
        let count = day_counts.get(day_str.as_str()).copied().unwrap_or(0);
        week.push((day_str, count));

        if week.len() == 7 {
            weeks.push(week.clone());
            week.clear();
        }
        current += chrono::Duration::days(1);
    }
    if !week.is_empty() {
        weeks.push(week);
    }

    // Transpose: we want columns=weeks, rows=days (Mon..Sun)
    // weeks[col][row] = (date, count)

    let max_count = heatmap.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", "Timeline" }
            div { class: "heatmap-container",
                div { class: "heatmap-labels",
                    div { class: "heatmap-label", "Mon" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "Wed" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "Fri" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "Sun" }
                }
                div { class: "heatmap-grid",
                    for week in weeks.iter() {
                        div { class: "heatmap-week",
                            for (day, count) in week.iter() {
                                {
                                    let level = if *count == 0 { 0 }
                                        else if *count <= max_count / 4 { 1 }
                                        else if *count <= max_count / 2 { 2 }
                                        else if *count <= max_count * 3 / 4 { 3 }
                                        else { 4 };
                                    let is_future = day.as_str() > today_str.as_str();
                                    let cls = if is_future {
                                        "heatmap-cell future".to_string()
                                    } else {
                                        format!("heatmap-cell level-{}", level)
                                    };
                                    rsx! {
                                        div {
                                            class: "{cls}",
                                            title: "{day}: {count} activities",
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "heatmap-legend",
                span { "Less" }
                div { class: "heatmap-cell level-0" }
                div { class: "heatmap-cell level-1" }
                div { class: "heatmap-cell level-2" }
                div { class: "heatmap-cell level-3" }
                div { class: "heatmap-cell level-4" }
                span { "More" }
            }
        }
    }
}
