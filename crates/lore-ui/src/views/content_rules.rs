use dioxus::prelude::*;
use crate::data;
use crate::texts;

#[component]
pub fn ContentRules() -> Element {
    let rules = use_signal(|| data::load_rules().unwrap_or_default());

    rsx! {
        section { class: "content-panel content-rules",
            h2 { {texts::CONTENT_RULES_TITLE} }
            table {
                thead {
                    tr {
                        th { {texts::COL_PATTERN} }
                        th { {texts::COL_MATCH_TYPE} }
                        th { {texts::COL_CATEGORY} }
                        th { {texts::COL_NOTE} }
                    }
                }
                tbody {
                    for rule in rules.read().iter() {
                        tr {
                            td { "{rule.pattern}" }
                            td { "{rule.match_type}" }
                            td { "{rule.category}" }
                            td { "{rule.note}" }
                        }
                    }
                }
            }
        }
    }
}
