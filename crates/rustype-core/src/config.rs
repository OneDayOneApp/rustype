use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::prompt;

/// Where the app keeps its configuration and history when `dirs` is not
/// consulted (the binary passes an explicit application-support path).
#[derive(Debug, Clone)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `~/Library/Application Support/Rustype` on macOS. `RUSTYPE_HOME`
    /// overrides the location, which is useful for portable installs and tests.
    pub fn application_support() -> Self {
        if let Some(dir) = std::env::var_os("RUSTYPE_HOME") {
            return Self::new(PathBuf::from(dir));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        Self::new(home.join("Library/Application Support/Rustype"))
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.dir.join("history.jsonl")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub provider: Provider,
    pub llm: Llm,
    pub audio: Audio,
    pub insert: Insert,
    pub hotkeys: Hotkeys,
    pub prompts: Prompts,
    pub dictionary: Vec<String>,
    /// Target language for translation mode, e.g. `"English"`.
    pub translation_target: String,
    /// Languages shown in the translation selector (values such as
    /// `"English (United States)"`). Empty = all built-in languages.
    pub translation_languages: Vec<String>,
    /// Keep a local JSONL history of transcripts.
    pub history_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::default(),
            llm: Llm::default(),
            audio: Audio::default(),
            insert: Insert::default(),
            hotkeys: Hotkeys::default(),
            prompts: Prompts::default(),
            dictionary: Vec::new(),
            translation_target: "English (United States)".to_string(),
            translation_languages: Vec::new(),
            history_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    /// OpenAI-compatible base URL (no trailing slash needed).
    pub base_url: String,
    /// Prefer `RUSTYPE_API_KEY` / `OPENAI_API_KEY`; this field is a fallback.
    pub api_key: String,
    pub asr_model: String,
    pub llm_model: String,
    /// ISO-639-1 hint for the ASR model; empty = auto-detect.
    pub language: String,
    pub timeout_secs: u64,
    /// Optional HTTP(S) proxy, e.g. "http://127.0.0.1:7890". Empty falls back
    /// to HTTPS_PROXY / ALL_PROXY from the environment.
    pub proxy: String,
}

impl Default for Provider {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            asr_model: "whisper-1".into(),
            llm_model: "gpt-4o-mini".into(),
            language: String::new(),
            timeout_secs: 60,
            proxy: String::new(),
        }
    }
}

impl Provider {
    /// Resolve the API key from the environment before the config file.
    pub fn resolved_api_key(&self) -> String {
        for var in ["RUSTYPE_API_KEY", "OPENAI_API_KEY"] {
            if let Ok(v) = std::env::var(var) {
                if !v.trim().is_empty() {
                    return v.trim().to_string();
                }
            }
        }
        self.api_key.trim().to_string()
    }

    /// Proxy from the config file, else the usual environment variables.
    pub fn resolved_proxy(&self) -> String {
        if !self.proxy.trim().is_empty() {
            return self.proxy.trim().to_string();
        }
        for var in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
            if let Ok(value) = std::env::var(var) {
                if !value.trim().is_empty() {
                    return value.trim().to_string();
                }
            }
        }
        String::new()
    }

    pub fn resolved_base_url(&self) -> String {
        std::env::var("RUSTYPE_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| self.base_url.clone())
            .trim_end_matches('/')
            .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Llm {
    /// When disabled, the raw ASR transcript is inserted directly.
    pub enabled: bool,
    pub temperature: f64,
}

impl Default for Llm {
    fn default() -> Self {
        Self {
            enabled: true,
            temperature: 0.2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    /// Working sample rate sent to the ASR provider.
    pub sample_rate: u32,
    /// Recordings shorter than this are discarded.
    pub min_duration_ms: u64,
    /// Hard cap for a single dictation.
    pub max_duration_ms: u64,
    /// Input device name substring; empty = system default.
    pub input_device: String,
    /// RMS below this is considered silence when trimming.
    pub silence_rms: f32,
    pub play_sounds: bool,
    /// Keep the input stream open between dictations to remove device startup
    /// latency. While warm, the microphone indicator stays on.
    pub keep_mic_warm: bool,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            min_duration_ms: 300,
            max_duration_ms: 300_000,
            input_device: String::new(),
            silence_rms: 0.004,
            play_sounds: true,
            keep_mic_warm: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Insert {
    /// `"paste"` (clipboard + Cmd/Ctrl+V) or `"type"` (synthesized key events).
    pub method: String,
    /// Put the previous clipboard contents back after pasting.
    pub restore_clipboard: bool,
    /// Wait this long after pasting before restoring the clipboard.
    pub restore_delay_ms: u64,
}

impl Default for Insert {
    fn default() -> Self {
        Self {
            method: "paste".into(),
            restore_clipboard: true,
            restore_delay_ms: 250,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    /// `"toggle"` = tap to start, tap again to stop (default);
    /// `"hold"` = push-to-talk, release to stop.
    pub mode: String,
    pub dictation: String,
    pub command: String,
    pub translation: String,
    pub paste_last: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        // Right Option is free while Typeless (or any other Fn-based tool) is
        // running, and holding Option alone types nothing. The chords add a
        // second modifier so no character key ever leaks.
        Self {
            mode: "toggle".into(),
            dictation: "right-alt".into(),
            command: "right-alt+space".into(),
            translation: "right-alt+shift".into(),
            paste_last: "ctrl+alt+v".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prompts {
    pub dictation: String,
    pub command: String,
    pub translation: String,
}

impl Default for Prompts {
    fn default() -> Self {
        Self {
            dictation: prompt::DEFAULT_DICTATION.to_string(),
            command: prompt::DEFAULT_COMMAND.to_string(),
            translation: prompt::DEFAULT_TRANSLATION.to_string(),
        }
    }
}

impl Config {
    pub fn load_or_create(paths: &Paths) -> std::io::Result<Self> {
        let path = paths.config_file();
        if path.exists() {
            return Self::load(&path);
        }
        paths.ensure()?;
        let cfg = Config::default();
        cfg.save(&path)?;
        Ok(cfg)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid config {}: {e}", path.display()),
            )
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).expect("config is serialisable");
        std::fs::write(path, text)
    }

    /// Validate cross-field constraints; returns human readable warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.audio.sample_rate < 8_000 || self.audio.sample_rate > 48_000 {
            out.push(format!(
                "audio.sample_rate={} is outside 8000..=48000",
                self.audio.sample_rate
            ));
        }
        if self.hotkeys.mode != "toggle" && self.hotkeys.mode != "hold" {
            out.push(format!(
                "hotkeys.mode={:?} must be \"toggle\" or \"hold\"",
                self.hotkeys.mode
            ));
        }
        if self.insert.method != "paste" && self.insert.method != "type" {
            out.push(format!(
                "insert.method={:?} must be \"paste\" or \"type\"",
                self.insert.method
            ));
        }
        if self.llm.temperature < 0.0 || self.llm.temperature > 2.0 {
            out.push("llm.temperature must be within 0.0..=2.0".into());
        }
        for (name, spec) in [
            ("dictation", &self.hotkeys.dictation),
            ("command", &self.hotkeys.command),
            ("translation", &self.hotkeys.translation),
            ("paste_last", &self.hotkeys.paste_last),
        ] {
            if let Err(e) = crate::hotkey::Shortcut::parse(spec) {
                out.push(format!("hotkeys.{name}: {e}"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let dir = std::env::temp_dir().join(format!("rustype-cfg-{}", std::process::id()));
        let paths = Paths::new(&dir);
        let cfg = Config::load_or_create(&paths).unwrap();
        assert!(paths.config_file().exists());
        let loaded = Config::load(&paths.config_file()).unwrap();
        assert_eq!(loaded.audio.sample_rate, 16_000);
        assert_eq!(loaded.hotkeys.dictation, "right-alt");
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_overrides_api_key() {
        let _guard = crate::test_support::env_lock();
        std::env::set_var("RUSTYPE_API_KEY", "  test-key  ");
        let p = Provider::default();
        assert_eq!(p.resolved_api_key(), "test-key");
        std::env::remove_var("RUSTYPE_API_KEY");
    }
}
