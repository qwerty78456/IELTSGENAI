//! Download file names: `<test type>_<summary>_<DD-MM-YYYY_HH-MM-SS>.<ext>`.
//!
//! "IELTS-Listening-Part1_Booking-A-Hotel-Room-Online_06-10-2026_14-32-05.docx".
//! The test type comes from the format's name (and the part, for a single
//! part), the summary from Gemini or, failing that, from the topics, the time
//! from the browser's clock. Every name is ASCII letters, digits, `-` and `_`,
//! so it is valid on Windows, macOS and Linux alike.

use crate::domain::ExamFormat;

/// Words kept from a summary.
const SUMMARY_WORDS: usize = 5;
/// Characters kept from one summary word.
const WORD_CHARS: usize = 20;
/// Used when neither Gemini nor the topics give a word.
const NO_SUMMARY: &str = "Draft";

/// A local wall-clock time, as the browser reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalStamp {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalStamp {
    /// Day first, the way dates are written in Vietnam: "06-10-2026_14-32-05".
    pub fn label(&self) -> String {
        format!(
            "{:02}-{:02}-{:04}_{:02}-{:02}-{:02}",
            self.day, self.month, self.year, self.hour, self.minute, self.second
        )
    }
}

/// Letters with diacritics and the ASCII letter they stand for: every
/// Vietnamese vowel with its tone marks, đ, and a few common Western letters.
const FOLDS: [(&str, char); 20] = [
    ("àáảãạăằắẳẵặâầấẩẫậäåā", 'a'),
    ("ÀÁẢÃẠĂẰẮẲẴẶÂẦẤẨẪẬÄÅĀ", 'A'),
    ("èéẻẽẹêềếểễệëē", 'e'),
    ("ÈÉẺẼẸÊỀẾỂỄỆËĒ", 'E'),
    ("ìíỉĩịïī", 'i'),
    ("ÌÍỈĨỊÏĪ", 'I'),
    ("òóỏõọôồốổỗộơờớởỡợöøō", 'o'),
    ("ÒÓỎÕỌÔỒỐỔỖỘƠỜỚỞỠỢÖØŌ", 'O'),
    ("ùúủũụưừứửữựüū", 'u'),
    ("ÙÚỦŨỤƯỪỨỬỮỰÜŪ", 'U'),
    ("ỳýỷỹỵÿ", 'y'),
    ("ỲÝỶỸỴŸ", 'Y'),
    ("đ", 'd'),
    ("Đ", 'D'),
    ("ç", 'c'),
    ("Ç", 'C'),
    ("ñ", 'n'),
    ("Ñ", 'N'),
    ("ß", 's'),
    ("ẞ", 'S'),
];

fn fold(c: char) -> char {
    if c.is_ascii() {
        return c;
    }
    FOLDS
        .iter()
        .find(|(letters, _)| letters.contains(c))
        .map(|(_, ascii)| *ascii)
        .unwrap_or(c)
}

/// The ASCII words of `text`: diacritics folded ("Hà Nội" → Ha, Noi),
/// anything that is not a letter or a digit separates words.
pub fn slug_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars().map(fold) {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// "IELTS-Listening" for an exam, "IELTS-Listening-Part2" for one part.
pub fn test_type(format: &ExamFormat, part: Option<u8>) -> String {
    let name = slug_words(&format.name).join("-");
    match part {
        Some(number) => format!("{name}-Part{number}"),
        None => name,
    }
}

/// At most 20 characters, the first one upper case.
fn capitalised(word: &str) -> String {
    word.chars()
        .take(WORD_CHARS)
        .enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect()
}

/// Gemini's summary as a name part: its first five words, each capitalised
/// and at most 20 characters, joined by `-`. `None` when it has no word.
pub fn summary_slug(reply: &str) -> Option<String> {
    let words: Vec<String> = slug_words(reply)
        .iter()
        .take(SUMMARY_WORDS)
        .map(|w| capitalised(w))
        .collect();
    (!words.is_empty()).then(|| words.join("-"))
}

/// Words a fallback summary skips.
const STOP_WORDS: [&str; 24] = [
    "a", "an", "the", "of", "to", "in", "on", "for", "and", "or", "about", "with", "at", "by",
    "from", "how", "are", "is", "was", "be", "their", "his", "her", "its",
];

/// A summary made without Gemini: the first five words of the topics that are
/// not stop words, or "Draft".
pub fn fallback_summary(source: &[String]) -> String {
    let words: Vec<String> = source
        .iter()
        .flat_map(|text| slug_words(text))
        .filter(|w| !STOP_WORDS.contains(&w.to_ascii_lowercase().as_str()))
        .take(SUMMARY_WORDS)
        .map(|w| capitalised(&w))
        .collect();
    if words.is_empty() {
        NO_SUMMARY.to_string()
    } else {
        words.join("-")
    }
}

/// The part every file of one draft shares.
pub fn file_stem(test_type: &str, summary: &str, stamp: &LocalStamp) -> String {
    format!("{test_type}_{summary}_{}", stamp.label())
}

/// A file of the draft: "{stem}.docx", or "{stem}_transcript.md" for a
/// second file with the same extension.
pub fn file_name(stem: &str, role: Option<&str>, extension: &str) -> String {
    match role {
        Some(role) => format!("{stem}_{role}.{extension}"),
        None => format!("{stem}.{extension}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FormatId;

    const STAMP: LocalStamp = LocalStamp {
        year: 2026,
        month: 10,
        day: 6,
        hour: 9,
        minute: 5,
        second: 7,
    };

    #[test]
    fn stamp_label_is_day_first_with_seconds() {
        assert_eq!(STAMP.label(), "06-10-2026_09-05-07");
    }

    #[test]
    fn vietnamese_letters_fold_to_ascii() {
        assert_eq!(slug_words("Hà Nội đông đúc"), ["Ha", "Noi", "dong", "duc"]);
        assert_eq!(slug_words("ĐẠI HỌC Ưu Tiên"), ["DAI", "HOC", "Uu", "Tien"]);
        assert_eq!(slug_words("café, naïve"), ["cafe", "naive"]);
    }

    #[test]
    fn slug_splits_on_punctuation() {
        assert_eq!(
            slug_words("  \"Booking: a hotel/room\" (online)... 2026!"),
            ["Booking", "a", "hotel", "room", "online", "2026"]
        );
        assert!(slug_words("— … ?!").is_empty());
    }

    #[test]
    fn summary_keeps_five_capped_words() {
        assert_eq!(
            summary_slug("hotel booking for a conference trip in May").as_deref(),
            Some("Hotel-Booking-For-A-Conference")
        );
        assert_eq!(
            summary_slug("Pneumonoultramicroscopicsilicovolcanoconiosis Study").as_deref(),
            Some("Pneumonoultramicrosc-Study")
        );
        assert_eq!(summary_slug("\"...\""), None);
        assert_eq!(summary_slug(""), None);
    }

    #[test]
    fn fallback_skips_stop_words() {
        let source =
            vec!["A radio interview about how city councils are handling a heatwave".to_string()];
        assert_eq!(
            fallback_summary(&source),
            "Radio-Interview-City-Councils-Handling"
        );
        let several = vec!["Cities".to_string(), "the museum tour".to_string()];
        assert_eq!(fallback_summary(&several), "Cities-Museum-Tour");
        assert_eq!(fallback_summary(&[]), "Draft");
        assert_eq!(fallback_summary(&["the of !".to_string()]), "Draft");
    }

    #[test]
    fn test_type_comes_from_format_and_part() {
        let ielts = FormatId::IeltsListening.format();
        let hsg = FormatId::HsgNational.format();
        assert_eq!(test_type(&ielts, None), "IELTS-Listening");
        assert_eq!(test_type(&hsg, None), "HSG-Quoc-gia-Listening");
        assert_eq!(test_type(&ielts, Some(2)), "IELTS-Listening-Part2");
    }

    #[test]
    fn roles_suffix_the_shared_stem() {
        let stem = file_stem(
            "IELTS-Listening-Part1",
            "Booking-A-Hotel-Room-Online",
            &STAMP,
        );
        assert_eq!(
            stem,
            "IELTS-Listening-Part1_Booking-A-Hotel-Room-Online_06-10-2026_09-05-07"
        );
        assert_eq!(file_name(&stem, None, "docx"), format!("{stem}.docx"));
        assert_eq!(file_name(&stem, None, "wav"), format!("{stem}.wav"));
        assert_eq!(
            file_name(&stem, Some("transcript"), "md"),
            format!("{stem}_transcript.md")
        );
    }

    #[test]
    fn stems_are_windows_safe() {
        let hsg = FormatId::HsgNational.format();
        let reply =
            "Ảnh/hưởng: <của> \"biến\" đổi | khí? hậu* \\ trên-toàn-cầu-và-những-hệ-quả-lâu-dài";
        let words = summary_slug(&format!("{} {}", "x".repeat(80), reply)).unwrap();
        let stem = file_stem(&test_type(&hsg, Some(4)), &words, &STAMP);
        let name = file_name(&stem, Some("transcript"), "md");
        assert!(
            name.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
            "{name}"
        );
        assert!(name.len() <= 170, "{} characters: {name}", name.len());
    }
}
