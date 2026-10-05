//! Speech markup: what a line may carry besides the words a listener hears.
//!
//! Gemini 3.8 TTS reads its text aloud character by character, except inline
//! tags in angle brackets (`<sigh>`), which it performs as sounds, and, in a
//! two-voice request, listener backchannels between pipes (`|mhm|`). Anything
//! else, a `[note]` or a `(laughs)`, is spoken. This module is the one grammar
//! for that markup:
//! - `display_text`: the words a listener hears (transcripts, grounding, word
//!   counts);
//! - `speech_text`: what the speech model gets (known tags kept, everything
//!   that would be read aloud by mistake dropped);
//! - `markup_problems`: what the teacher should look at.
//!
//! The scanner: `<x>` is a tag when x is 1-32 ASCII letters, spaces, hyphens
//! or apostrophes, starting and ending with a letter (so `5 < 6 and 7 > 3`
//! stays text); `[x]` on one line is a note; `|x|` is a backchannel when x
//! holds a letter and neither starts nor ends with a space; any other `|` is a
//! stray pipe.

/// Inline tags Google documents for Gemini 3.8 TTS (prompting guide, updated
/// 2026-10-01), spelling variants included. Used to recognise markup: only
/// `EXAM_SPEECH_TAGS` are ever asked for.
pub const SPEECH_TAGS: &[&str] = &[
    "argh",
    "breath",
    "heavy breath",
    "exhales",
    "cackle",
    "cheer",
    "chuckle",
    "chuckles",
    "cough",
    "cry",
    "gasp",
    "giggle",
    "groan",
    "growl",
    "grunt",
    "grr",
    "hiss",
    "laugh",
    "laughter",
    "moan",
    "pant",
    "pff",
    "phew",
    "scream",
    "shout",
    "shriek",
    "sigh",
    "sighs",
    "sneeze",
    "snicker",
    "snort",
    "sob",
    "throat-clearing",
    "tsk",
    "whimper",
    "whispers",
    "whispering",
    "yawn",
    "short pause",
    "long pause",
];

/// The tags a script may ask for (`docs/voices.md`, gate G3): performed and
/// never read aloud in the 2026-10-05 probes, when placed mid-sentence. Add
/// one only after measuring it with `tools/voice_lab.py`.
pub const EXAM_SPEECH_TAGS: [&str; 4] = ["sigh", "cough", "laugh", "chuckle"];

/// Documented tags the probes heard read aloud at least once (gate G3). They
/// are recognised but never sent, so the listener never hears "long pause".
pub const READ_ALOUD_TAGS: [&str; 3] = ["long pause", "whispers", "whispering"];

/// A turn shorter than this many words should carry one tag at most.
pub const SHORT_TURN_WORDS: usize = 40;

const MAX_TAG_CHARS: usize = 32;
const MAX_NOTE_CHARS: usize = 80;
const MAX_BACKCHANNEL_CHARS: usize = 40;

/// The documented tag a name stands for: lower case, inner spaces collapsed,
/// and a plural whose singular is a tag ("laughs") read as the singular. The
/// plural of an exam tag is always read as the exam tag ("sighs", "chuckles"
/// are documented too but were not measured): models often write them.
pub fn speech_tag(name: &str) -> Option<&'static str> {
    let name = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let singular = name.strip_suffix('s');
    if let Some(singular) = singular
        && let Some(tag) = EXAM_SPEECH_TAGS.iter().copied().find(|t| *t == singular)
    {
        return Some(tag);
    }
    let find = |candidate: &str| SPEECH_TAGS.iter().copied().find(|tag| *tag == candidate);
    find(&name).or_else(|| singular.and_then(find))
}

/// The words a listener hears: tags, notes, backchannels and stray pipes
/// removed; one space between words and none before punctuation where
/// markup was taken out. What transcripts print and grounding compares.
pub fn display_text(text: &str) -> String {
    let kept = rebuild(text, |piece| {
        (piece.kind == Kind::Words).then(|| piece.text.to_string())
    });
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text sent to the speech model: documented tags kept in their
/// lower-case form (except `READ_ALOUD_TAGS`), unknown `<tags>`, `[notes]`
/// and stray pipes dropped, `|backchannels|` kept only when `backchannels`
/// is true (a two-voice request on a model that performs them). Text without
/// markup comes back unchanged, trimmed.
pub fn speech_text(text: &str, backchannels: bool) -> String {
    rebuild(text, |piece| match piece.kind {
        Kind::Words => Some(piece.text.to_string()),
        Kind::Tag => performed_tag(piece.inner()).map(|tag| format!("<{tag}>")),
        Kind::Backchannel if backchannels => Some(piece.text.to_string()),
        Kind::Backchannel | Kind::Note | Kind::StrayPipe => None,
    })
}

/// A documented tag that is not read aloud: what `speech_text` keeps.
fn performed_tag(name: &str) -> Option<&'static str> {
    speech_tag(name).filter(|tag| !READ_ALOUD_TAGS.contains(tag))
}

/// One whitespace-separated piece of a line, markup kept whole
/// (`<short pause>`, `|oh really?|`). `spoken` when it holds a word a
/// listener hears; a tag, a backchannel or a lone dash does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'a> {
    pub text: &'a str,
    pub spoken: bool,
}

/// The tokens of `text`, in order. Joined with single spaces they give the
/// text back with its whitespace collapsed.
pub fn tokens(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    // Start offset and whether a spoken character was seen.
    let mut current: Option<(usize, bool)> = None;
    let mut offset = 0;
    for piece in scan(text) {
        let start = offset;
        offset += piece.text.len();
        if piece.kind != Kind::Words {
            current.get_or_insert((start, false));
            continue;
        }
        for (i, c) in piece.text.char_indices() {
            let at = start + i;
            if c.is_whitespace() {
                if let Some((from, spoken)) = current.take() {
                    tokens.push(Token {
                        text: &text[from..at],
                        spoken,
                    });
                }
            } else {
                current.get_or_insert((at, false)).1 |= c.is_alphanumeric();
            }
        }
    }
    if let Some((from, spoken)) = current {
        tokens.push(Token {
            text: &text[from..],
            spoken,
        });
    }
    tokens
}

/// Words a listener hears in `text`; markup never counts.
pub fn spoken_words(text: &str) -> usize {
    tokens(text).iter().filter(|t| t.spoken).count()
}

/// Teacher-readable problems with the markup of one turn: what is left out
/// of the recording, what may be read aloud, tags the exam recordings were
/// not tested with, a tag opening the turn (often dropped or turned into a
/// breath) and too many tags in a short turn. A `[FILL ...]` note is not
/// listed: it is a gap, which `validate_passage` reports as an error.
pub fn markup_problems(text: &str) -> Vec<String> {
    let pieces = scan(text);
    let mut problems = Vec::new();
    let mut tags = 0;
    for piece in &pieces {
        match piece.kind {
            Kind::Tag => match speech_tag(piece.inner()) {
                None => problems.push(format!(
                    "{} is not a speech tag; it is left out of the recording",
                    piece.text
                )),
                Some(tag) if READ_ALOUD_TAGS.contains(&tag) => problems.push(format!(
                    "<{tag}> is read aloud by the speech model; it is left out of the recording"
                )),
                Some(tag) => {
                    tags += 1;
                    if !EXAM_SPEECH_TAGS.contains(&tag) {
                        problems.push(format!(
                            "<{tag}> has not been tested for exam recordings; listen to it, or use one of {}",
                            exam_tag_list()
                        ));
                    }
                }
            },
            Kind::Note => {
                // The same test as the gap check in `validate_passage`.
                if !piece.text.starts_with("[FILL") {
                    problems.push(format!(
                        "{} is a note, not speech; it is left out of the recording",
                        piece.text
                    ));
                }
            }
            Kind::StrayPipe => {
                problems.push("A lone \"|\" is left out of the recording".to_string())
            }
            Kind::Backchannel => {}
            Kind::Words => {
                for direction in stage_directions(piece.text) {
                    problems.push(format!(
                        "{direction} looks like a stage direction and would be read aloud; write a speech tag such as <laugh> mid-sentence instead"
                    ));
                }
            }
        }
    }
    let opening = pieces
        .iter()
        .find(|p| !(p.kind == Kind::Words && p.text.trim().is_empty()));
    if let Some(piece) = opening
        && piece.kind == Kind::Tag
        && let Some(tag) = performed_tag(piece.inner())
    {
        problems.push(format!(
            "<{tag}> opens the turn, where the speech model often drops it or turns it into a breath; put it mid-sentence"
        ));
    }
    let words = spoken_words(text);
    if tags > 1 && words < SHORT_TURN_WORDS {
        problems.push(format!(
            "{tags} speech tags in a turn of {words} words; one tag every few turns is enough"
        ));
    }
    problems
}

/// "<sigh>, <cough>, <laugh> or <chuckle>".
pub fn exam_tag_list() -> String {
    let tags: Vec<String> = EXAM_SPEECH_TAGS.iter().map(|t| format!("<{t}>")).collect();
    match tags.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} or {last}", rest.join(", ")),
        _ => tags.concat(),
    }
}

/// "(laughs)" or "*sighs*" in spoken text: a sound written the way older
/// models took it, which 3.8 reads aloud.
fn stage_directions(words: &str) -> Vec<&str> {
    let mut found = Vec::new();
    for (open, close) in [('(', ')'), ('*', '*')] {
        let mut rest = words;
        while let Some(start) = rest.find(open) {
            let after = &rest[start + 1..];
            let Some(end) = after.find(close) else {
                break;
            };
            let inner = &after[..end];
            if inner.len() <= MAX_TAG_CHARS && speech_tag(inner.trim()).is_some() {
                found.push(&rest[start..start + 1 + end + 1]);
            }
            rest = &after[end + 1..];
        }
    }
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Words,
    Tag,
    Note,
    Backchannel,
    StrayPipe,
}

/// A stretch of a line: words, or one piece of markup with its delimiters.
#[derive(Debug, Clone, Copy)]
struct Piece<'a> {
    kind: Kind,
    text: &'a str,
}

impl<'a> Piece<'a> {
    /// The markup without its delimiters.
    fn inner(&self) -> &'a str {
        match self.kind {
            Kind::Tag | Kind::Note | Kind::Backchannel => &self.text[1..self.text.len() - 1],
            Kind::Words | Kind::StrayPipe => self.text,
        }
    }
}

/// `text` as consecutive pieces covering all of it.
fn scan(text: &str) -> Vec<Piece<'_>> {
    let bytes = text.as_bytes();
    let mut pieces = Vec::new();
    let mut words_from = 0;
    let mut i = 0;
    while i < bytes.len() {
        let found = match bytes[i] {
            b'<' => closing(text, i, '>', MAX_TAG_CHARS)
                .filter(|&end| is_tag_name(&text[i + 1..end - 1]))
                .map(|end| (Kind::Tag, end)),
            b'[' => closing(text, i, ']', MAX_NOTE_CHARS)
                .filter(|&end| !text[i + 1..end - 1].contains('['))
                .map(|end| (Kind::Note, end)),
            b'|' => Some(
                closing(text, i, '|', MAX_BACKCHANNEL_CHARS)
                    .filter(|&end| is_backchannel(&text[i + 1..end - 1]))
                    .map_or((Kind::StrayPipe, i + 1), |end| (Kind::Backchannel, end)),
            ),
            _ => None,
        };
        match found {
            Some((kind, end)) => {
                if words_from < i {
                    pieces.push(Piece {
                        kind: Kind::Words,
                        text: &text[words_from..i],
                    });
                }
                pieces.push(Piece {
                    kind,
                    text: &text[i..end],
                });
                i = end;
                words_from = end;
            }
            None => i += 1,
        }
    }
    if words_from < text.len() {
        pieces.push(Piece {
            kind: Kind::Words,
            text: &text[words_from..],
        });
    }
    pieces
}

/// The end (exclusive) of the markup opened at `open`: the first `close`
/// after it on the same line, at most `max` bytes further.
fn closing(text: &str, open: usize, close: char, max: usize) -> Option<usize> {
    let rest = &text[open + 1..];
    let at = rest.find(close)?;
    (at <= max && !rest[..at].contains('\n')).then_some(open + 1 + at + 1)
}

fn is_tag_name(name: &str) -> bool {
    let starts_and_ends_with_letter = name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.ends_with(|c: char| c.is_ascii_alphabetic());
    starts_and_ends_with_letter
        && name
            .chars()
            .all(|c| c.is_ascii_alphabetic() || matches!(c, ' ' | '-' | '\''))
}

fn is_backchannel(inner: &str) -> bool {
    !inner.is_empty()
        && inner.chars().any(char::is_alphabetic)
        && !inner.starts_with(char::is_whitespace)
        && !inner.ends_with(char::is_whitespace)
}

/// Punctuation that sits right after a word.
const CLOSING_PUNCTUATION: [char; 8] = [',', '.', ';', ':', '!', '?', '\u{2026}', ')'];

/// The pieces `keep` returns, joined. Where markup was removed the seam is
/// tidied: one space at most, none before punctuation, no comma doubled or
/// left at the start. Kept pieces elsewhere are copied as they are, so text
/// without removed markup comes back unchanged (trimmed).
fn rebuild<'a>(text: &'a str, keep: impl Fn(&Piece<'a>) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    // Markup was removed since the last kept text, and whether whitespace sat
    // around it.
    let mut seam = false;
    let mut space = false;
    for piece in scan(text) {
        let Some(kept) = keep(&piece) else {
            seam = true;
            continue;
        };
        if !seam {
            out.push_str(&kept);
            continue;
        }
        let mut rest = kept.trim_start();
        space |= rest.len() < kept.len();
        if rest.is_empty() {
            continue;
        }
        let before = out.trim_end().len();
        space |= before < out.len();
        out.truncate(before);
        let mut dropped = false;
        if rest.starts_with([',', ';', ':'])
            && (out.is_empty() || out.ends_with([',', ';', ':', '.', '!', '?']))
        {
            rest = rest[1..].trim_start();
            dropped = true;
            if rest.is_empty() {
                continue;
            }
        }
        let glued = rest.starts_with(CLOSING_PUNCTUATION);
        let between_words =
            out.ends_with(char::is_alphanumeric) && rest.starts_with(char::is_alphanumeric);
        if !out.is_empty() && !glued && (space || dropped || between_words) {
            out.push(' ');
        }
        out.push_str(rest);
        seam = false;
        space = false;
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_text_drops_tags_backchannels_and_notes() {
        assert_eq!(
            display_text("Well <sigh>, I suppose |mhm| so [music]."),
            "Well, I suppose so."
        );
        assert_eq!(display_text("<laugh> Yes, of course."), "Yes, of course.");
        assert_eq!(display_text("<sigh>, well, fine"), "well, fine");
        assert_eq!(display_text("Yes, <sigh>, I see"), "Yes, I see");
        assert_eq!(display_text("Fine. <laugh>, so"), "Fine. so");
        assert_eq!(
            display_text("It was <short pause> fine | really"),
            "It was fine really"
        );
        assert_eq!(display_text("Well<sigh>I agree"), "Well I agree");
        assert_eq!(display_text("[music]"), "");
        // No markup: only whitespace is tidied.
        assert_eq!(
            display_text("  Hello,   how are\tyou? "),
            "Hello, how are you?"
        );
    }

    #[test]
    fn comparison_signs_and_numbers_are_not_tags() {
        for text in [
            "It is under 5 < 6 and 7 > 3.",
            "Prices <£5 or >£10.",
            "Rooms <10 m2 and > 3 m2 cost less.",
        ] {
            let pieces = scan(text);
            assert!(
                pieces.iter().all(|p| p.kind != Kind::Tag),
                "{text}: {pieces:?}"
            );
        }
        assert_eq!(display_text("It is under 5 < 6."), "It is under 5 < 6.");
        assert_eq!(
            speech_text("It is under 5 < 6.", true),
            "It is under 5 < 6."
        );
    }

    #[test]
    fn speech_tag_accepts_documented_names_variants_and_plurals() {
        assert_eq!(speech_tag("sigh"), Some("sigh"));
        assert_eq!(speech_tag("Short  Pause"), Some("short pause"));
        assert_eq!(speech_tag("laughs"), Some("laugh"));
        assert_eq!(speech_tag("laughter"), Some("laughter"));
        // Plurals of exam tags are the exam tag, documented or not.
        assert_eq!(speech_tag("chuckles"), Some("chuckle"));
        assert_eq!(speech_tag("Sighs"), Some("sigh"));
        assert_eq!(speech_tag("coughs"), Some("cough"));
        assert_eq!(speech_tag("whispers"), Some("whispers"));
        assert!(markup_problems("Well, I <sighs> suppose so.").is_empty());
        assert_eq!(speech_tag("throat-clearing"), Some("throat-clearing"));
        assert_eq!(speech_tag("smirk"), None);
        assert_eq!(speech_tag("smiles warmly"), None);
    }

    #[test]
    fn exam_tags_are_documented_and_never_read_aloud() {
        for tag in EXAM_SPEECH_TAGS {
            assert!(SPEECH_TAGS.contains(&tag), "{tag}");
            assert!(!READ_ALOUD_TAGS.contains(&tag), "{tag}");
        }
        for tag in READ_ALOUD_TAGS {
            assert!(SPEECH_TAGS.contains(&tag), "{tag}");
        }
        assert_eq!(exam_tag_list(), "<sigh>, <cough>, <laugh> or <chuckle>");
    }

    #[test]
    fn speech_text_keeps_known_tags_in_canonical_form() {
        let text = "Well <Sigh> I <smirk> think [pause] so <Short  Pause> yes |mhm| ok <laughs>.";
        assert_eq!(
            speech_text(text, false),
            "Well <sigh> I think so <short pause> yes ok <laugh>."
        );
        assert_eq!(
            speech_text(text, true),
            "Well <sigh> I think so <short pause> yes |mhm| ok <laugh>."
        );
        // Measured as read aloud: never sent.
        assert_eq!(
            speech_text("Wait <long pause> what? <whispers> Quiet.", true),
            "Wait what? Quiet."
        );
        // A stray pipe is never sent; a backchannel only when asked for.
        assert_eq!(
            speech_text("So | then |oh really?| yes", false),
            "So then yes"
        );
        assert_eq!(
            speech_text("So | then |oh really?| yes", true),
            "So then |oh really?| yes"
        );
        // Text without markup is unchanged, so earlier takes are reused.
        let plain = "Good morning, how can I help?  Take a seat.";
        assert_eq!(speech_text(plain, true), plain);
        assert_eq!(speech_text(plain, false), plain);
        // Idempotent.
        let once = speech_text(text, true);
        assert_eq!(speech_text(&once, true), once);
    }

    #[test]
    fn tokens_keep_markup_whole_and_count_only_words() {
        let tokens = tokens("Well, <short pause> I |oh really?| see. <sigh>, then");
        let texts: Vec<&str> = tokens.iter().map(|t| t.text).collect();
        assert_eq!(
            texts,
            [
                "Well,",
                "<short pause>",
                "I",
                "|oh really?|",
                "see.",
                "<sigh>,",
                "then"
            ]
        );
        let spoken: Vec<bool> = tokens.iter().map(|t| t.spoken).collect();
        assert_eq!(spoken, [true, false, true, false, true, false, true]);
        assert_eq!(spoken_words("Well, <short pause> I |oh really?| see."), 3);
        assert_eq!(spoken_words("one two three"), 3);
        assert!(super::tokens("   ").is_empty());
    }

    #[test]
    fn markup_problems_name_what_would_be_read_aloud_or_dropped() {
        let problems = |text: &str| markup_problems(text);
        assert!(problems("Well, I did ask <sigh> but nobody called back.").is_empty());
        assert!(problems("Plain words only, with 5 < 6.").is_empty());

        let unknown = problems("It was fine <smirk> really.");
        assert_eq!(unknown.len(), 1, "{unknown:?}");
        assert!(unknown[0].starts_with("<smirk> is not a speech tag"));

        let read_aloud = problems("Wait <long pause> for it.");
        assert!(read_aloud[0].contains("read aloud"), "{read_aloud:?}");

        let note = problems("And then [music] we start.");
        assert_eq!(
            note,
            ["[music] is a note, not speech; it is left out of the recording"]
        );
        // A gap is reported by validate_passage, not twice.
        assert!(problems("The code is [FILL IN] today.").is_empty());

        let untested = problems("It was so funny <giggle> honestly.");
        assert!(untested[0].contains("has not been tested"), "{untested:?}");

        let opening = problems("<laugh> Yes, that was me.");
        assert!(opening[0].contains("opens the turn"), "{opening:?}");

        let crowded = problems("Oh <sigh> no, <cough> sorry.");
        assert!(
            crowded
                .iter()
                .any(|p| p.starts_with("2 speech tags in a turn of 3 words")),
            "{crowded:?}"
        );
        let long_turn = format!("Oh <sigh> {} <cough> sorry.", "word ".repeat(50));
        assert!(problems(&long_turn).is_empty());

        let directions = problems("Well (laughs) I never *sighs* did.");
        assert_eq!(directions.len(), 2, "{directions:?}");
        assert!(directions[0].starts_with("(laughs) looks like a stage direction"));
        // A parenthesis that is content is left alone.
        assert!(problems("Ask for Mrs Brown (that's me) at the desk.").is_empty());

        assert_eq!(
            problems("So | then"),
            ["A lone \"|\" is left out of the recording"]
        );
        assert!(problems("So |mhm| then").is_empty());
    }
}
