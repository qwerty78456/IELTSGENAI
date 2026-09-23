use dioxus::prelude::*;

use crate::domain::ValidationIssue;

/// Validator findings, one line each; renders nothing when there are none.
#[component]
pub fn IssueList(issues: Vec<ValidationIssue>) -> Element {
    if issues.is_empty() {
        return rsx! {};
    }
    rsx! {
        ul { class: "issue-list",
            for issue in issues.iter() {
                li { class: "issue-item", "{issue.display()}" }
            }
        }
    }
}
