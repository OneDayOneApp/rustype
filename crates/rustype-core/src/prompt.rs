use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::hotkey::Mode;

pub const DEFAULT_DICTATION: &str = "\
You are Rustype, a dictation engine. You receive a raw speech-to-text transcript \
from the user's microphone and return the text the user intended to type.

Rules:
- Remove filler words (um, uh, like, you know), false starts and repeated words.
- When the speaker corrects themselves, keep only the final version.
- Add natural punctuation, capitalisation and paragraph breaks.
- If the speaker clearly dictates structure (a list, an email, a title), format it.
- Preserve the speaker's language, wording, names and numbers exactly.
- Never answer questions, never add facts, never explain. Output only the final text.";

pub const DEFAULT_COMMAND: &str = "\
You are Rustype in command mode. The user holds a shortcut and speaks.

If selected text is provided, apply the spoken instruction to it.
If no selected text is provided, treat the transcript as a question or a request \
and answer it directly.

Rules:
- If the instruction asks to rewrite, shorten, expand, fix, translate or restyle, \
  return only the replacement text with no preamble.
- If the instruction is a question, answer it accurately and concisely; say so \
  plainly when the answer is unknown.
- Keep names, numbers and facts unless the instruction changes them.
- Never mention the instruction or the selection. Output only the result.";

pub const DEFAULT_TRANSLATION: &str = "\
You are Rustype in translation mode. Translate the transcript into the requested \
target language. Preserve tone, formatting, names and numbers. Do not add \
commentary. Output only the translation.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

/// Ambient context captured from the frontmost application.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub app_name: String,
    pub window_title: String,
    pub selected_text: String,
}

impl Context {
    fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        if !self.app_name.trim().is_empty() {
            parts.push(format!("application: {}", self.app_name.trim()));
        }
        if !self.window_title.trim().is_empty() {
            parts.push(format!("window: {}", self.window_title.trim()));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("; "))
        }
    }
}

/// Build the chat messages for one dictation.
pub fn build_messages(cfg: &Config, mode: Mode, transcript: &str, ctx: &Context) -> Vec<Message> {
    let base = match mode {
        Mode::Dictation => cfg.prompts.dictation.as_str(),
        Mode::Command => cfg.prompts.command.as_str(),
        Mode::Translation => cfg.prompts.translation.as_str(),
    };

    let mut system = base.to_string();
    if mode == Mode::Translation && !cfg.translation_target.trim().is_empty() {
        system.push_str(&format!(
            "\n\nTarget language: {}.",
            cfg.translation_target.trim()
        ));
    }
    if let Some(desc) = ctx.describe() {
        system.push_str(&format!("\n\nContext — {desc}."));
    }
    if !cfg.dictionary.is_empty() {
        system.push_str(&format!(
            "\n\nPreferred spellings (personal dictionary): {}.",
            cfg.dictionary.join(", ")
        ));
    }

    let mut user = String::new();
    if mode == Mode::Command && !ctx.selected_text.trim().is_empty() {
        let selected = truncate_chars(ctx.selected_text.trim(), 6000);
        user.push_str("<selected_text>\n");
        user.push_str(selected);
        user.push_str("\n</selected_text>\n\n<instruction>\n");
        user.push_str(transcript.trim());
        user.push_str("\n</instruction>");
    } else if mode == Mode::Translation {
        user.push_str(transcript.trim());
    } else {
        user.push_str(transcript.trim());
    }

    vec![Message::system(system), Message::user(user)]
}

pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        let mut c = Config::default();
        c.translation_target = "Japanese".into();
        c.dictionary = vec!["Rustype".into(), "Kubernetes".into()];
        c
    }

    #[test]
    fn dictation_prompt_has_dictionary_and_context() {
        let ctx = Context {
            app_name: "Slack".into(),
            window_title: "#general".into(),
            selected_text: String::new(),
        };
        let msgs = build_messages(&cfg(), Mode::Dictation, "hello there", &ctx);
        assert_eq!(msgs.len(), 2);
        assert!(msgs[0].content.contains("dictation engine"));
        assert!(msgs[0].content.contains("Rustype, Kubernetes"));
        assert!(msgs[0].content.contains("application: Slack"));
        assert_eq!(msgs[1].content, "hello there");
    }

    #[test]
    fn command_prompt_embeds_selection() {
        let ctx = Context {
            selected_text: "teh cat sat".into(),
            ..Default::default()
        };
        let msgs = build_messages(&cfg(), Mode::Command, "fix the typo", &ctx);
        assert!(msgs[1]
            .content
            .contains("<selected_text>\nteh cat sat\n</selected_text>"));
        assert!(msgs[1]
            .content
            .contains("<instruction>\nfix the typo\n</instruction>"));
    }

    #[test]
    fn command_without_selection_answers_questions() {
        let msgs = build_messages(&cfg(), Mode::Command, "what is the capital of France", &Context::default());
        assert!(msgs[0].content.contains("If no selected text is provided"));
        assert!(!msgs[1].content.contains("<selected_text>"));
        assert_eq!(msgs[1].content, "what is the capital of France");
    }

    #[test]
    fn translation_names_target() {
        let msgs = build_messages(
            &cfg(),
            Mode::Translation,
            "good morning",
            &Context::default(),
        );
        assert!(msgs[0].content.contains("Target language: Japanese"));
        assert_eq!(msgs[1].content, "good morning");
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        assert_eq!(truncate_chars("日本語テキスト", 3), "日本語");
        assert_eq!(truncate_chars("abc", 10), "abc");
    }
}
