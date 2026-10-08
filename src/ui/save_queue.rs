//! Session-only save state. No UI runtime, timers or network: callers drive it.
use crate::application::exams::{SaveOutcome, SaveRequest, SavedExam};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveStatus {
    Ready,
    Sending,
    Failed(String),
    Conflict,
    Deleted,
    Invalid(String),
}

#[derive(Clone)]
pub struct Draft {
    pub snapshot: SavedExam,
    pub sequence: u64,
    pub acknowledged: u64,
    pub armed: bool,
    pub status: SaveStatus,
    pub pending: Option<(u64, SaveRequest)>,
    pub deleting: bool,
    pub warning: Option<String>,
}

impl Draft {
    pub fn dirty(&self) -> bool {
        self.sequence != self.acknowledged || self.pending.is_some()
    }
}

#[derive(Clone, Default)]
pub struct SaveQueue {
    pub drafts: HashMap<Uuid, Draft>,
    pub deleted: HashSet<Uuid>,
    pub navigation: u64,
    pub library_epoch: u64,
}

impl SaveQueue {
    pub fn invalidate_open(&mut self) -> u64 {
        self.navigation = self.navigation.wrapping_add(1);
        self.navigation
    }
    pub fn accepts_open(&self, token: u64) -> bool {
        self.navigation == token
    }
    pub fn stage(&mut self, snapshot: SavedExam, armed: bool) {
        let id = snapshot.exam.id;
        if self.deleted.contains(&id) && !self.drafts.contains_key(&id) {
            return;
        }
        let draft = self.drafts.entry(id).or_insert_with(|| Draft {
            snapshot: snapshot.clone(),
            sequence: 0,
            acknowledged: 0,
            armed,
            status: SaveStatus::Ready,
            pending: None,
            deleting: false,
            warning: None,
        });
        // Ignore server-derived metadata when deciding whether content changed.
        let mut next = snapshot;
        next.revision = draft.snapshot.revision;
        next.created_at_secs = draft.snapshot.created_at_secs;
        next.updated_at_secs = draft.snapshot.updated_at_secs;
        if next != draft.snapshot || draft.sequence == 0 {
            draft.sequence += 1;
            draft.snapshot = next;
            if matches!(draft.status, SaveStatus::Invalid(_)) {
                draft.status = SaveStatus::Ready;
            }
        }
        draft.armed |= armed;
    }
    pub fn begin(&mut self, id: Uuid, retry: bool) -> Option<SaveRequest> {
        let draft = self.drafts.get_mut(&id)?;
        if draft.deleting || !draft.armed || !draft.dirty() {
            return None;
        }
        match &draft.status {
            SaveStatus::Ready => {}
            SaveStatus::Failed(_) if retry => {}
            _ => return None,
        }
        let (_, request) = draft.pending.get_or_insert_with(|| {
            (
                draft.sequence,
                SaveRequest {
                    saved: draft.snapshot.clone(),
                    expected_revision: draft.snapshot.revision,
                    mutation_id: Uuid::new_v4(),
                },
            )
        });
        draft.status = SaveStatus::Sending;
        Some(request.clone())
    }
    pub fn finish(&mut self, id: Uuid, result: Result<SaveOutcome, String>) {
        let Some(draft) = self.drafts.get_mut(&id) else {
            return;
        };
        match result {
            Ok(SaveOutcome::Saved {
                summary,
                recording_job,
                warning,
            }) => {
                let Some((sequence, request)) = draft.pending.take() else {
                    return;
                };
                draft.acknowledged = sequence;
                draft.snapshot.revision = summary.revision;
                draft.snapshot.created_at_secs = summary.created_at_secs;
                draft.snapshot.updated_at_secs = summary.updated_at_secs;
                if draft.snapshot.recording_job == request.saved.recording_job {
                    draft.snapshot.recording_job = recording_job;
                    if draft.snapshot.recording_job.is_none() {
                        draft.snapshot.recording = None;
                    }
                }
                draft.warning = warning;
                draft.status = SaveStatus::Ready;
            }
            Ok(SaveOutcome::Conflict { .. }) => {
                draft.pending = None;
                draft.status = SaveStatus::Conflict;
            }
            Ok(SaveOutcome::Deleted) => {
                self.deleted.insert(id);
                draft.pending = None;
                draft.status = SaveStatus::Deleted;
            }
            Ok(SaveOutcome::Invalid { message }) => {
                draft.pending = None;
                draft.status = SaveStatus::Invalid(message);
            }
            Err(message) => draft.status = SaveStatus::Failed(message),
        }
        self.library_epoch = self.library_epoch.wrapping_add(1);
    }
    pub fn overwrite(&mut self, id: Uuid, revision: i64) {
        if let Some(draft) = self.drafts.get_mut(&id)
            && draft.status == SaveStatus::Conflict
            && !draft.deleting
        {
            draft.snapshot.revision = revision;
            draft.pending = None;
            draft.status = SaveStatus::Ready;
            draft.armed = true;
        }
    }
    pub fn forget_deleted(&mut self, id: Uuid) {
        self.drafts.remove(&id);
        self.deleted.insert(id);
        self.library_epoch = self.library_epoch.wrapping_add(1);
    }
    pub fn evict_clean(&mut self, active: Uuid) {
        self.drafts
            .retain(|id, d| *id == active || d.dirty() || d.deleting || d.warning.is_some());
    }
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn needs_warning(&self) -> bool {
        self.drafts.values().any(Draft::dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::exams::ExamSummary;
    fn snapshot(title: &str) -> SavedExam {
        let mut saved: SavedExam = serde_json::from_str(include_str!(
            "../application/fixtures/saved_exam_0_7_1.json"
        ))
        .unwrap();
        saved.exam.id = Uuid::new_v4();
        saved.exam.title = title.into();
        saved.recording_job = None;
        saved.recording = None;
        saved.revision = 1;
        saved
    }
    fn acknowledge(q: &mut SaveQueue, id: Uuid, revision: i64) {
        let d = &q.drafts[&id];
        q.finish(
            id,
            Ok(SaveOutcome::Saved {
                summary: ExamSummary {
                    id,
                    revision,
                    title: d.snapshot.exam.title.clone(),
                    format: d.snapshot.exam.format.id,
                    parts_total: 4,
                    parts_with_script: 1,
                    parts_complete: 0,
                    recording: None,
                    created_at_secs: 10,
                    updated_at_secs: 20,
                },
                recording_job: None,
                warning: None,
            }),
        );
    }
    #[test]
    fn switching_exams_keeps_latest_snapshot_and_single_flight() {
        let mut q = SaveQueue::default();
        let mut a = snapshot("A");
        let id = a.exam.id;
        q.stage(a.clone(), true);
        let first = q.begin(id, false).unwrap();
        a.exam.title = "A edited during save".into();
        q.stage(a.clone(), true);
        q.stage(snapshot("B"), false);
        assert!(q.begin(id, false).is_none());
        assert_eq!(q.drafts[&id].snapshot.exam.title, a.exam.title);
        acknowledge(&mut q, id, 2);
        assert!(q.drafts[&id].dirty());
        let second = q.begin(id, false).unwrap();
        assert_eq!(second.saved.exam.title, a.exam.title);
        assert_eq!(second.expected_revision, 2);
        assert_ne!(first.mutation_id, second.mutation_id);
        acknowledge(&mut q, id, 3);
        assert!(!q.drafts[&id].dirty());
    }
    #[test]
    fn uncertain_save_retries_identical_payload_before_newer_edits() {
        let mut q = SaveQueue::default();
        let mut a = snapshot("A");
        let id = a.exam.id;
        q.stage(a.clone(), true);
        let first = q.begin(id, false).unwrap();
        q.finish(id, Err("offline".into()));
        a.exam.title = "newer".into();
        q.stage(a, true);
        assert!(q.begin(id, false).is_none());
        assert_eq!(q.begin(id, true), Some(first));
        acknowledge(&mut q, id, 2);
        assert_eq!(q.begin(id, false).unwrap().saved.exam.title, "newer");
    }
    #[test]
    fn conflict_pauses_only_its_exam_and_overwrite_uses_confirmed_revision() {
        let mut q = SaveQueue::default();
        let a = snapshot("A");
        let id = a.exam.id;
        let b = snapshot("B");
        let bid = b.exam.id;
        q.stage(a, true);
        q.begin(id, false).unwrap();
        q.finish(id, Ok(SaveOutcome::Conflict { revision: 2 }));
        assert!(q.begin(id, true).is_none());
        q.stage(b, true);
        assert!(q.begin(bid, false).is_some());
        q.overwrite(id, 3);
        assert_eq!(q.begin(id, false).unwrap().expected_revision, 3);
        q.finish(id, Ok(SaveOutcome::Conflict { revision: 4 }));
        assert_eq!(q.drafts[&id].status, SaveStatus::Conflict);
        assert!(q.drafts[&id].dirty());
    }
    #[test]
    fn unsaved_unarmed_failed_and_deleted_drafts_survive_eviction() {
        let mut q = SaveQueue::default();
        let a = snapshot("A");
        let id = a.exam.id;
        q.stage(a, false);
        assert!(q.needs_warning());
        assert!(q.begin(id, false).is_none());
        q.evict_clean(Uuid::new_v4());
        assert!(q.drafts.contains_key(&id));
        q.drafts.get_mut(&id).unwrap().armed = true;
        q.begin(id, false).unwrap();
        q.finish(id, Err("offline".into()));
        q.evict_clean(Uuid::new_v4());
        assert_eq!(q.drafts[&id].snapshot.exam.title, "A");
        q.begin(id, true).unwrap();
        q.finish(id, Ok(SaveOutcome::Deleted));
        assert!(q.begin(id, true).is_none());
        q.evict_clean(Uuid::new_v4());
        assert!(q.drafts.contains_key(&id));
    }
    #[test]
    fn delete_pause_waits_for_save_and_success_cannot_be_resurrected() {
        let mut q = SaveQueue::default();
        let a = snapshot("A");
        let id = a.exam.id;
        q.stage(a.clone(), true);
        q.begin(id, false).unwrap();
        q.drafts.get_mut(&id).unwrap().deleting = true;
        acknowledge(&mut q, id, 2);
        assert!(q.begin(id, true).is_none());
        q.forget_deleted(id);
        q.stage(a, true);
        assert!(!q.drafts.contains_key(&id));
        assert!(!q.needs_warning());
    }
    #[test]
    fn only_latest_navigation_can_apply_success_or_error() {
        let mut q = SaveQueue::default();
        let b = q.invalidate_open();
        let c = q.invalidate_open();
        assert!(!q.accepts_open(b));
        assert!(q.accepts_open(c));
        for _ in ["new", "format", "edit", "delete"] {
            let pending = q.invalidate_open();
            q.invalidate_open();
            assert!(!q.accepts_open(pending));
        }
    }
    #[test]
    fn missing_audio_ack_preserves_a_newer_recording_reference() {
        let mut q = SaveQueue::default();
        let mut a = snapshot("A");
        let id = a.exam.id;
        a.recording_job = Some("old".into());
        q.stage(a.clone(), true);
        q.begin(id, false).unwrap();
        a.recording_job = Some("new".into());
        q.stage(a, true);
        acknowledge(&mut q, id, 2);
        assert_eq!(q.drafts[&id].snapshot.recording_job.as_deref(), Some("new"));
        assert!(q.drafts[&id].dirty());
    }
}
