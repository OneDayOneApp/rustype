//! Guards against ASR hallucinations.
//!
//! Whisper-family models sometimes emit long, fluent text for silence or noise.
//! These helpers bound the transcript by the recording duration and bound the
//! LLM rewrite by the transcript length.

/// Maximum plausible transcript length for a recording.
///
/// 25 characters per second is far above normal speech (Chinese ~5/sec,
/// English ~4 words/sec) but still catches a model inventing hundreds of
/// characters from a few seconds of silence.
pub fn max_transcript_chars(duration_ms: u64) -> usize {
    let seconds = (duration_ms as f64 / 1000.0).max(0.3);
    (seconds * 25.0) as usize + 80
}

/// Returns `None` when the transcript is implausibly long for the audio.
pub fn plausible_transcript(transcript: &str, duration_ms: u64) -> Option<&str> {
    let chars = transcript.chars().count();
    if chars > max_transcript_chars(duration_ms) {
        None
    } else {
        Some(transcript)
    }
}

/// Accept an LLM rewrite only when it did not balloon beyond the input.
pub fn accept_cleanup<'a>(transcript: &str, cleaned: &'a str) -> Option<&'a str> {
    let input = transcript.chars().count();
    let output = cleaned.chars().count();
    let limit = input * 3 + 120;
    if output > limit {
        None
    } else {
        Some(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_silence_cannot_produce_a_page() {
        // 4 seconds of audio must not yield 850 characters.
        assert!(plausible_transcript(&"字".repeat(850), 4_000).is_none());
        assert!(plausible_transcript("你好，世界。", 4_000).is_some());
    }

    #[test]
    fn long_recording_allows_long_text() {
        let text = "a".repeat(600);
        assert!(plausible_transcript(&text, 60_000).is_some());
    }

    #[test]
    fn cleanup_cannot_balloon() {
        let raw = "hello world";
        let balloon = "x".repeat(500);
        assert!(accept_cleanup(raw, &balloon).is_none());
        assert!(accept_cleanup(raw, "Hello, world.").is_some());
    }
}

/// Rough intent check used to choose between "rewrite the selection" and
/// "answer a question about the selection". Kept intentionally simple.
pub fn looks_like_question(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    if t.ends_with('?') || t.ends_with('？') {
        return true;
    }
    const ZH: [&str; 13] = [
        "什么", "怎么", "为什么", "为何", "哪", "谁", "多少", "是否", "能不能", "可不可以", "吗",
        "如何", "解释",
    ];
    const EN: [&str; 10] = [
        "what ", "why ", "how ", "who ", "where ", "when ", "which ", "can you", "could you",
        "is it",
    ];
    ZH.iter().any(|w| t.contains(w)) || EN.iter().any(|w| t.contains(w))
}

/// Rough detection of "edit the selection" instructions, used to decide
/// whether a command should replace the selection instead of showing a card.
pub fn looks_like_edit_request(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    const ZH: [&str; 16] = [
        "改写", "改成", "改为", "改得", "改一下", "改一改", "修改", "润色", "缩短", "扩写",
        "翻译", "修正", "纠正", "替换", "总结", "概括",
    ];
    const EN: [&str; 9] = [
        "rewrite",
        "rephrase",
        "shorten",
        "expand",
        "translate",
        "fix",
        "correct",
        "summarize",
        "replace",
    ];
    ZH.iter().any(|w| t.contains(w)) || EN.iter().any(|w| t.contains(w))
}

#[cfg(test)]
mod question_tests {
    use super::{looks_like_edit_request, looks_like_question};

    #[test]
    fn detects_edit_requests() {
        assert!(looks_like_edit_request("把这段改得更正式"));
        assert!(looks_like_edit_request("rewrite this paragraph"));
        assert!(!looks_like_edit_request("搜索一下 Google"));
        assert!(!looks_like_edit_request("这段是什么意思"));
    }

    #[test]
    fn detects_questions() {
        assert!(looks_like_question("这段代码是什么意思？"));
        assert!(looks_like_question("what does this mean"));
        assert!(looks_like_question("帮我解释一下"));
        assert!(!looks_like_question("把这段改得更正式"));
        assert!(!looks_like_question("fix the typo"));
    }
}
