use crate::backend;
use crate::data::format_file_size;
use crate::state::AppState;
use crate::store::DataStore;
use dioxus::prelude::*;

#[component]
pub fn ContentSpaces() -> Element {
    let state = use_context::<AppState>();
    let store = use_context::<DataStore>();
    let tick = store.revision;
    let mut spaces_data = use_signal(Vec::<SpaceWithStats>::new);
    let mut renaming_id = use_signal(|| Option::<i64>::None);
    let mut rename_value = use_signal(String::new);

    use_effect(move || {
        let _ = *tick.read();
        spawn(async move {
            let b = backend::current();
            let spaces = b.list_all_spaces().await.unwrap_or_default();
            let mut items = Vec::new();
            for s in &spaces {
                let stats = b
                    .space_stats(s.id)
                    .await
                    .unwrap_or(lore_core::db::SpaceStats {
                        page_count: 0,
                        note_count: 0,
                        file_count: 0,
                        file_size_bytes: 0,
                        pages_size_bytes: 0,
                    });
                items.push(SpaceWithStats {
                    id: s.id,
                    name: s.name.clone(),
                    deleted_at: s.deleted_at.clone(),
                    page_count: stats.page_count,
                    note_count: stats.note_count,
                    file_count: stats.file_count,
                    file_size_display: format_file_size(stats.file_size_bytes),
                    size_display: format_file_size(stats.pages_size_bytes),
                });
            }
            spaces_data.set(items);
        });
    });

    let active_space_id = *state.space_id.read();

    rsx! {
        section { class: "content-panel content-spaces",
            h2 { "Spaces" }
            div { class: "spaces-list",
                for space in spaces_data.read().iter() {
                    {
                        let sid = space.id;
                        let is_active = sid == active_space_id;
                        let is_deleted = space.deleted_at.is_some();
                        let is_renaming = *renaming_id.read() == Some(sid);
                        let sname = space.name.clone();
                        let deleted_at_display = space.deleted_at.as_ref()
                            .map(|d| d.chars().take(10).collect::<String>())
                            .unwrap_or_default();

                        let card_cls = if is_deleted {
                            "space-card deleted"
                        } else if is_active {
                            "space-card active"
                        } else {
                            "space-card"
                        };

                        rsx! {
                            div { key: "{sid}", class: "{card_cls}",
                                div { class: "space-card-header",
                                    if is_renaming {
                                        input {
                                            class: "inline-rename",
                                            r#type: "text",
                                            value: "{rename_value}",
                                            autofocus: true,
                                            oninput: move |evt| rename_value.set(evt.value()),
                                            onkeydown: move |evt| {
                                                if evt.key() == Key::Enter {
                                                    let name = rename_value.read().trim().to_string();
                                                    if !name.is_empty() {
                                                        let mut store = store;
                                                        let state = state;
                                                        let name = name.clone();
                                                        spawn(async move { store.rename_space(&state, sid, &name).await.ok(); });
                                                    }
                                                    renaming_id.set(None);
                                                } else if evt.key() == Key::Escape {
                                                    renaming_id.set(None);
                                                }
                                            },
                                        }
                                    } else {
                                        span { class: "space-card-name", "{space.name}" }
                                        if is_active {
                                            span { class: "space-card-badge", "active" }
                                        }
                                        if is_deleted {
                                            span { class: "space-card-badge deleted-badge", "deleted" }
                                        }
                                    }
                                }
                                if is_deleted {
                                    div { class: "space-card-deleted-info",
                                        "Deleted {deleted_at_display}. Permanently removed after 30 days."
                                    }
                                }
                                div { class: "space-card-stats",
                                    span { "{space.page_count} pages" }
                                    span { class: "sep", "·" }
                                    span { "{space.note_count} notes" }
                                    span { class: "sep", "·" }
                                    span { "{space.file_count} files ({space.file_size_display})" }
                                    span { class: "sep", "·" }
                                    span { "{space.size_display} archived" }
                                }
                                div { class: "space-card-actions",
                                    if is_deleted {
                                        button { class: "btn-sm",
                                            onclick: move |_| {
                                                let mut store = store;
                                                let state = state;
                                                spawn(async move { store.restore_space(&state, sid).await.ok(); });
                                            },
                                            "Restore"
                                        }
                                        button { class: "btn-sm btn-danger",
                                            onclick: move |_| {
                                                let mut store = store;
                                                let state = state;
                                                spawn(async move { store.delete_space_permanent(&state, sid).await.ok(); });
                                            },
                                            "Delete permanently"
                                        }
                                    } else {
                                        if !is_active {
                                            button { class: "btn-sm",
                                                onclick: move |_| {
                                                    let mut store = store;
                                                    let mut state = state;
                                                    spawn(async move { store.switch_space(&mut state, sid).await; });
                                                },
                                                "Switch to"
                                            }
                                        }
                                        button { class: "btn-sm",
                                            onclick: move |_| {
                                                rename_value.set(sname.clone());
                                                renaming_id.set(Some(sid));
                                            },
                                            "Rename"
                                        }
                                        if !is_active {
                                            button { class: "btn-sm btn-danger",
                                                onclick: move |_| {
                                                    let mut store = store;
                                                    let state = state;
                                                    spawn(async move { store.trash_space(&state, sid).await.ok(); });
                                                },
                                                "Delete"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct SpaceWithStats {
    id: i64,
    name: String,
    deleted_at: Option<String>,
    page_count: i64,
    note_count: i64,
    file_count: i64,
    file_size_display: String,
    size_display: String,
}
