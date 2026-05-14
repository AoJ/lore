use crate::state::AppState;
use crate::store::DataStore;
use dioxus::prelude::*;
use std::collections::HashMap;

#[component]
pub fn ListTimeline() -> Element {
    let mut state = use_context::<AppState>();
    let mut store = use_context::<DataStore>();
    let heatmap = store.heatmap.read();

    let day_counts: HashMap<&str, i64> = heatmap
        .iter()
        .map(|(day, count)| (day.as_str(), *count))
        .collect();

    let today = chrono::Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let days_since_monday = today.format("%u").to_string().parse::<i64>().unwrap_or(1) - 1;
    let grid_end = today;
    let grid_start = grid_end - chrono::Duration::days(30 + days_since_monday);

    let mut weeks: Vec<Vec<(String, i64)>> = Vec::new();
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

    let max_count = heatmap.iter().map(|(_, c)| *c).max().unwrap_or(1).max(1);
    let selected_day = store.timeline_selected_day.read().clone();

    rsx! {
        div { class: "list-panel",
            h2 { class: "list-title", "Timeline" }

            // Heatmap grid
            div { class: "heatmap-container",
                div { class: "heatmap-labels",
                    div { class: "heatmap-label", "Mo" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "We" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "Fr" }
                    div { class: "heatmap-label", "" }
                    div { class: "heatmap-label", "Su" }
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
                                    let is_selected = selected_day.as_deref() == Some(day.as_str());
                                    let cls = if is_future {
                                        "heatmap-cell future".to_string()
                                    } else if is_selected {
                                        format!("heatmap-cell level-{} selected", level)
                                    } else {
                                        format!("heatmap-cell level-{}", level)
                                    };
                                    let day_clone = day.clone();
                                    rsx! {
                                        div {
                                            class: "{cls}",
                                            title: "{day}: {count} activities",
                                            onclick: move |_| {
                                                if !is_future {
                                                    let mut store = store;
                                                    let day = day_clone.clone();
                                                    spawn(async move { store.select_timeline_day(&state, &day).await; });
                                                }
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Legend
            div { class: "heatmap-legend",
                span { "Less" }
                div { class: "heatmap-cell level-0" }
                div { class: "heatmap-cell level-1" }
                div { class: "heatmap-cell level-2" }
                div { class: "heatmap-cell level-3" }
                div { class: "heatmap-cell level-4" }
                span { "More" }
            }

            // Day detail (items for selected day)
            if let Some(ref day) = *store.timeline_selected_day.read() {
                div { class: "timeline-day-header", "{day}" }
            }

            div { class: "list-items",
                if store.timeline_selected_day.read().is_none() {
                    div { class: "empty-state", "Click a day to see activities." }
                }

                // Notes for selected day
                if !store.timeline_day_notes.read().is_empty() {
                    div { class: "search-group-header", "Notes ({store.timeline_day_notes.read().len()})" }
                    for note in store.timeline_day_notes.read().iter() {
                        {
                            let id = note.id;
                            let title = if note.title.is_empty() { "Untitled note".to_string() } else { note.title.clone() };
                            rsx! {
                                div { class: "list-item",
                                    onclick: move |_| {
                                        state.selected.set(crate::state::Selected::Note(id));
                                    },
                                    div { class: "list-item-title", "{title}" }
                                    div { class: "list-item-date", "{note.updated_at}" }
                                }
                            }
                        }
                    }
                }

                // Pages for selected day
                if !store.timeline_day_pages.read().is_empty() {
                    div { class: "search-group-header", "Pages ({store.timeline_day_pages.read().len()})" }
                    for (page_id, page_title) in store.timeline_day_pages.read().iter() {
                        {
                            let pid = *page_id;
                            rsx! {
                                div { class: "list-item",
                                    onclick: move |_| {
                                        state.selected.set(crate::state::Selected::Page(pid));
                                    },
                                    div { class: "list-item-title", "{page_title}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
