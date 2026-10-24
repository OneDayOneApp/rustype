use serde::Deserialize;

use crate::config::{Llm, Provider};
use crate::error::{Error, Result};
use crate::prompt::Message;

/// OpenAI-compatible chat-completions client used for cleanup/command/translation.
pub struct LlmClient {
    http: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
    temperature: f64,
}

impl LlmClient {
    pub fn new(provider: &Provider, llm: &Llm) -> Result<Self> {
        let api_key = provider.resolved_api_key();
        if api_key.is_empty() {
            return Err(Error::Config(
                "no API key: set RUSTYPE_API_KEY, OPENAI_API_KEY or provider.api_key".into(),
            ));
        }
        let http = crate::http::client(provider)?;
        Ok(Self {
            http,
            url: format!("{}/chat/completions", provider.resolved_base_url()),
            api_key,
            model: provider.llm_model.clone(),
            temperature: llm.temperature,
        })
    }

    pub fn complete(&self, messages: &[Message]) -> Result<String> {
        let body = serde_json::json!({
            "model": self.model,
            "temperature": self.temperature,
            "messages": messages,
        });
        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                body: truncate(&text, 800),
            });
        }
        parse_chat(&text)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

pub fn parse_chat(body: &str) -> Result<String> {
    let parsed: ChatResponse = serde_json::from_str(body)?;
    let content = parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| Error::Api("chat response had no choices".into()))?;
    Ok(clean_model_output(&content))
}

/// Models sometimes wrap the answer in quotes or a code fence despite the
/// instructions; strip the common cases without touching real formatting.
pub fn clean_model_output(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    if text.starts_with("```") && text.ends_with("```") && text.len() > 6 {
        let inner = &text[3..text.len() - 3];
        let inner = inner.strip_prefix("text").unwrap_or(inner);
        text = inner.trim().to_string();
    }
    if text.len() >= 2 {
        let bytes = text.as_bytes();
        let quoted = (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'');
        if quoted {
            text = text[1..text.len() - 1].trim().to_string();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_first_choice() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"Hello there."}}]}"#;
        assert_eq!(parse_chat(body).unwrap(), "Hello there.");
    }

    #[test]
    fn strips_quotes_and_fences() {
        assert_eq!(clean_model_output("\"hi\""), "hi");
        assert_eq!(clean_model_output("```text\nhi\n```"), "hi");
        assert_eq!(clean_model_output("plain"), "plain");
    }

    #[test]
    fn errors_without_choices() {
        assert!(parse_chat(r#"{"choices":[]}"#).is_err());
    }
}
