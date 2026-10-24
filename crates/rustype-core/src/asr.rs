use serde::Deserialize;

use crate::config::Provider;
use crate::error::{Error, Result};

/// Minimal client for OpenAI-compatible `/audio/transcriptions` endpoints.
pub struct AsrClient {
    http: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
    language: String,
}

impl AsrClient {
    pub fn new(cfg: &Provider) -> Result<Self> {
        let api_key = cfg.resolved_api_key();
        if api_key.is_empty() {
            return Err(Error::Config(
                "no API key: set RUSTYPE_API_KEY, OPENAI_API_KEY or provider.api_key".into(),
            ));
        }
        let http = crate::http::client(cfg)?;
        Ok(Self {
            http,
            url: format!("{}/audio/transcriptions", cfg.resolved_base_url()),
            api_key,
            model: cfg.asr_model.clone(),
            language: cfg.language.clone(),
        })
    }

    pub fn transcribe_wav(&self, wav: Vec<u8>, prompt: Option<&str>) -> Result<String> {
        let part = reqwest::blocking::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")?;
        let mut form = reqwest::blocking::multipart::Form::new()
            .part("file", part)
            .text("model", self.model.clone())
            .text("response_format", "json");
        if !self.language.trim().is_empty() {
            form = form.text("language", self.language.trim().to_string());
        }
        if let Some(p) = prompt.filter(|p| !p.trim().is_empty()) {
            form = form.text("prompt", p.trim().to_string());
        }

        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()?;
        let status = resp.status();
        let body = resp.text()?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                body: truncate(&body, 800),
            });
        }
        parse_transcription(&body)
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
struct Transcription {
    text: String,
}

pub fn parse_transcription(body: &str) -> Result<String> {
    let parsed: Transcription = serde_json::from_str(body)?;
    Ok(parsed.text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_openai_shape() {
        let text = parse_transcription(r#"{"text":" hello world "}"#).unwrap();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn rejects_missing_text() {
        assert!(parse_transcription(r#"{"error":{"message":"bad"}}"#).is_err());
    }

    #[test]
    fn requires_api_key() {
        let _guard = crate::test_support::env_lock();
        let mut p = Provider::default();
        p.api_key = String::new();
        std::env::remove_var("RUSTYPE_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        assert!(matches!(AsrClient::new(&p), Err(Error::Config(_))));
    }
}
