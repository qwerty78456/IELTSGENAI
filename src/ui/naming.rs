//! Download names, shared by both pages: `<test type>_<summary>_<time>`
//! (`export::naming`).
//!
//! Each page state carries a `DraftNaming`. Right after a script is written
//! the page asks Gemini for a five-word summary of the draft's topics
//! (`prefetch`); the summary is kept with the topics it sums up, so it is out
//! of date as soon as they change, and never flagged. The first download of
//! a draft takes the browser's time and fixes the stem for every other file
//! of that draft (`stem`), so its DOCX and WAV share a name. A step that
//! replaces a file of the draft (a new script, new questions, a new
//! recording) starts a new draft (`DraftNaming::new_draft`); a first
//! recording or first questions keep the name. A download never waits more
//! than ten seconds for a summary: without one the files are named after the
//! topics themselves.

use std::sync::atomic::{AtomicU32, Ordering};

use dioxus::prelude::*;
use uuid::Uuid;

use crate::application::naming::summarize_topics;
use crate::export::naming::{fallback_summary, file_name, file_stem, summary_slug};
use crate::ui::clock::now_stamp;
use crate::ui::components::audio_player::{download_bytes, download_text, download_url};
use crate::ui::jobs::sleep_ms;

const SUMMARY_POLL_MS: u32 = 200;
const SUMMARY_WAIT_MS: u32 = 10_000;
/// Summaries kept: a late reply for older topics (or an exam closed since)
/// lands beside the one in use instead of replacing it.
const SUMMARIES_KEPT: usize = 4;

/// Draft tokens for both pages: an old exam's late download can never fix
/// the stem of an exam opened since.
static DRAFTS: AtomicU32 = AtomicU32::new(1);

fn next_draft() -> u32 {
    DRAFTS.fetch_add(1, Ordering::Relaxed)
}

/// A summary and whether Gemini wrote it (otherwise it was made from the
/// topics after a failed request, and the next draft asks again).
#[derive(Debug, Clone)]
struct Summary {
    source: Vec<String>,
    words: String,
    from_gemini: bool,
}

/// How the downloads of the current draft are named.
#[derive(Debug, Clone)]
pub struct DraftNaming {
    /// The latest summaries, each with the topics it sums up, newest last.
    summaries: Vec<Summary>,
    /// The topics a summary is being asked for.
    asking: Option<Vec<String>>,
    /// The stem every file of this draft shares, fixed at its first download.
    stem: Option<String>,
    draft: u32,
    /// Files (role, extension) waiting for their name: asking for one again
    /// meanwhile (a second click) downloads nothing more.
    waiting: Vec<(Option<&'static str>, &'static str)>,
}

impl Default for DraftNaming {
    fn default() -> Self {
        Self {
            summaries: Vec::new(),
            asking: None,
            stem: None,
            draft: next_draft(),
            waiting: Vec::new(),
        }
    }
}

impl DraftNaming {
    /// A step that replaces a file of the draft has started: the next
    /// download takes a new time. The summary stays; it follows the topics.
    pub fn new_draft(&mut self) {
        self.stem = None;
        self.draft = next_draft();
    }

    fn summary_for(&self, source: &[String]) -> Option<&Summary> {
        self.summaries.iter().rev().find(|s| s.source == source)
    }

    fn keep(&mut self, summary: Summary) {
        self.summaries.retain(|s| s.source != summary.source);
        self.summaries.push(summary);
        if self.summaries.len() > SUMMARIES_KEPT {
            self.summaries.remove(0);
        }
    }
}

/// What a page names its downloads after.
#[derive(Debug, Clone, PartialEq)]
pub struct NameRequest {
    /// "IELTS-Listening-Part2" (`export::naming::test_type`).
    pub test_type: String,
    /// The theme and topics the summary sums up.
    pub source: Vec<String>,
    /// The saved exam the summary's spend is booked to.
    pub exam: Option<Uuid>,
}

/// What a download saves.
pub enum Content {
    Bytes(Vec<u8>, &'static str),
    Text(String),
    /// A file the server serves, downloaded straight from its URL.
    Link(String),
}

fn naming_of<S: AsRef<DraftNaming>>(state: &S) -> &DraftNaming {
    state.as_ref()
}

fn naming_mut<S: AsMut<DraftNaming>>(state: &mut S) -> &mut DraftNaming {
    state.as_mut()
}

fn has_topic(source: &[String]) -> bool {
    source.iter().any(|t| !t.trim().is_empty())
}

/// Asks Gemini to sum up `source` unless its summary is here or on its way.
/// The request runs as a task of its own, in the scope of the task that calls
/// this (the root for exam pipelines, the part page otherwise), so a download
/// that stops waiting never cancels a call Google bills.
pub fn prefetch<S>(mut state: Signal<S>, source: Vec<String>, exam: Option<Uuid>)
where
    S: AsRef<DraftNaming> + AsMut<DraftNaming> + 'static,
{
    if !has_topic(&source) {
        return;
    }
    {
        let s = state.peek();
        let naming = naming_of(&*s);
        let written = naming.summary_for(&source).is_some_and(|s| s.from_gemini);
        if written || naming.asking.as_ref() == Some(&source) {
            return;
        }
    }
    naming_mut(&mut *state.write()).asking = Some(source.clone());
    spawn(async move {
        let reply = summarize_topics(source.clone(), exam).await;
        let gemini = reply.ok().and_then(|line| summary_slug(&line));
        let from_gemini = gemini.is_some();
        let words = gemini.unwrap_or_else(|| fallback_summary(&source));
        let mut s = state.write();
        let naming = naming_mut(&mut *s);
        if naming.asking.as_ref() == Some(&source) {
            naming.asking = None;
        }
        naming.keep(Summary {
            source,
            words,
            from_gemini,
        });
    });
}

/// The stem of the draft's files: fixed at its first download, otherwise the
/// test type, the summary of `request.source` (waited for up to ten seconds,
/// else made from the topics) and the browser's time now.
pub async fn stem<S>(mut state: Signal<S>, request: NameRequest) -> String
where
    S: AsRef<DraftNaming> + AsMut<DraftNaming> + 'static,
{
    let (draft, fixed) = {
        let s = state.peek();
        let naming = naming_of(&*s);
        (naming.draft, naming.stem.clone())
    };
    if let Some(stem) = fixed {
        return stem;
    }
    let stamp = now_stamp();
    prefetch(state, request.source.clone(), request.exam);
    let mut waited_ms = 0;
    let words = loop {
        let ready = naming_of(&*state.peek())
            .summary_for(&request.source)
            .map(|s| s.words.clone());
        if let Some(words) = ready {
            break words;
        }
        if !has_topic(&request.source) || waited_ms >= SUMMARY_WAIT_MS {
            break fallback_summary(&request.source);
        }
        sleep_ms(SUMMARY_POLL_MS).await;
        waited_ms += SUMMARY_POLL_MS;
    };
    let stem = file_stem(&request.test_type, &words, &stamp);
    let mut s = state.write();
    let naming = naming_mut(&mut *s);
    if naming.draft != draft {
        // A new draft started meanwhile: this file is named, that draft is not.
        return stem;
    }
    naming.stem.get_or_insert(stem).clone()
}

/// Names one file of the draft and downloads it. The content was taken when
/// the download was asked for; only the name waits for the summary. The same
/// file asked for again while it waits (a second click, or a click while the
/// automatic download waits) is downloaded once.
pub async fn download<S>(
    mut state: Signal<S>,
    request: NameRequest,
    content: Content,
    role: Option<&'static str>,
    extension: &'static str,
) where
    S: AsRef<DraftNaming> + AsMut<DraftNaming> + 'static,
{
    let file = (role, extension);
    {
        let mut s = state.write();
        let naming = naming_mut(&mut *s);
        if naming.waiting.contains(&file) {
            return;
        }
        naming.waiting.push(file);
    }
    let stem = stem(state, request).await;
    naming_mut(&mut *state.write())
        .waiting
        .retain(|waiting| *waiting != file);
    let name = file_name(&stem, role, extension);
    match content {
        Content::Bytes(bytes, mime) => download_bytes(&bytes, mime, &name),
        Content::Text(text) => download_text(&text, &name),
        Content::Link(url) => download_url(&url, &name),
    }
}
