use dioxus::prelude::*;
use uuid::Uuid;

use crate::application::audio::JobStatus;
use crate::application::exams::ExamSummary;
use crate::ui::clock::local_date_time;

/// The saved exams on this server: open one, or delete it after a second click.
#[component]
pub fn ExamLibrary(
    exams: Vec<ExamSummary>,
    current: Uuid,
    busy: bool,
    onopen: EventHandler<Uuid>,
    ondelete: EventHandler<Uuid>,
) -> Element {
    let mut confirming = use_signal(|| None::<Uuid>);
    if exams.is_empty() {
        return rsx! {
            p { class: "muted", "Nothing saved yet." }
        };
    }
    rsx! {
        div { class: "exam-library-wrap",
            table { class: "exam-library",
                thead {
                    tr {
                        th { "Title" }
                        th { "Format" }
                        th { "Progress" }
                        th { "Recording" }
                        th { "Updated" }
                        th {}
                    }
                }
                tbody {
                    for exam in exams.iter() {
                        {
                            let id = exam.id;
                            let is_open = id == current;
                            let pending = confirming() == Some(id);
                            let recording = match exam.recording {
                                None => "none",
                                Some(JobStatus::Completed) => "ready",
                                Some(JobStatus::Pending | JobStatus::Processing) => "running",
                                Some(JobStatus::Failed) => "failed",
                            };
                            rsx! {
                                tr { class: if is_open { "open" } else { "" }, key: "{id}",
                                    td {
                                        "{exam.title}"
                                        if is_open { span { class: "muted", " (open)" } }
                                    }
                                    td { "{exam.format.format().name}" }
                                    td { "scripts {exam.parts_with_script}/{exam.parts_total}, questions {exam.parts_complete}/{exam.parts_total}" }
                                    td { "{recording}" }
                                    td { "{local_date_time(exam.updated_at_secs)}" }
                                    td {
                                        div { class: "row-actions",
                                            if pending {
                                                button {
                                                    class: "download-button danger small",
                                                    disabled: busy,
                                                    onclick: move |_| {
                                                        confirming.set(None);
                                                        ondelete.call(id);
                                                    },
                                                    "Confirm delete"
                                                }
                                                button {
                                                    class: "download-button secondary small",
                                                    onclick: move |_| confirming.set(None),
                                                    "Cancel"
                                                }
                                            } else {
                                                button {
                                                    class: "download-button info small",
                                                    disabled: busy || is_open,
                                                    onclick: move |_| onopen.call(id),
                                                    "Open"
                                                }
                                                button {
                                                    class: "download-button secondary small",
                                                    disabled: busy,
                                                    onclick: move |_| confirming.set(Some(id)),
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
}
