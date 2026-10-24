use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use rustype_core::audio::{duration_ms, encode_wav, resample, to_i16, trim_silence, voiced_ms};
use rustype_core::config::{Config, Paths};
use rustype_core::guard;
use rustype_core::history::{self, HistoryEntry};
use rustype_core::hotkey::{InputState, Mode};
use rustype_core::prompt::{self, Context as VoiceContext};
use rustype_core::state::{Action, Engine};
use rustype_core::{asr::AsrClient, llm::LlmClient};

use crate::macos::audio::Recorder;
use crate::macos::{context, insert};

/// Commands coming from the menu-bar button.
#[derive(Debug, Clone)]
pub enum UiCommand {
    Toggle,
    Stop,
    Cancel,
    PasteLast,
    Reload(Box<Config>),
    SetTargetLanguage(String),
}

#[derive(Debug, Clone)]
pub struct AnswerPayload {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone)]
pub struct UiStatus {
    pub state: u8,
    pub mode: Mode,
    pub target: String,
    pub languages: Vec<String>,
}

pub const STATUS_IDLE: u8 = 0;
pub const STATUS_RECORDING: u8 = 1;
pub const STATUS_PROCESSING: u8 = 2;

pub struct Session {
    cfg: Config,
    paths: Paths,
    engine: Engine,
    mode: Mode,
    recorder: Option<Recorder>,
    recording: bool,
    started: Option<Instant>,
    last_context: VoiceContext,
    processing: Arc<AtomicBool>,
    last_text: Option<String>,
    level: Arc<std::sync::atomic::AtomicU32>,
    result_tx: Sender<Result<(String, u64, bool, String), String>>,
    result_rx: Receiver<Result<(String, u64, bool, String), String>>,
    cmd_rx: crossbeam_channel::Receiver<UiCommand>,
    status_tx: crossbeam_channel::Sender<UiStatus>,
    answer_tx: crossbeam_channel::Sender<AnswerPayload>,
}

impl Session {
    pub fn new(
        cfg: Config,
        paths: Paths,
        cmd_rx: crossbeam_channel::Receiver<UiCommand>,
        status_tx: crossbeam_channel::Sender<UiStatus>,
    answer_tx: crossbeam_channel::Sender<AnswerPayload>,
        level: Arc<std::sync::atomic::AtomicU32>,
    ) -> Result<Self> {
        let engine = Engine::new(&cfg.hotkeys)?;
        let (result_tx, result_rx) = channel();
        let last_text = if cfg.history_enabled {
            history::last_text(&paths.history_file())
        } else {
            None
        };
        Ok(Self {
            cfg,
            paths,
            engine,
            mode: Mode::Dictation,
            recorder: None,
            recording: false,
            started: None,
            last_context: VoiceContext::default(),
            processing: Arc::new(AtomicBool::new(false)),
            last_text,
            level,
            result_tx,
            result_rx,
            cmd_rx,
            status_tx,
            answer_tx,
        })
    }

    fn set_status(&self, state: u8) {
        let _ = self.status_tx.send(UiStatus {
            state,
            mode: self.mode,
            target: self.cfg.translation_target.clone(),
            languages: self.cfg.translation_languages.clone(),
        });
    }

    fn handle_ui_command(&mut self, cmd: UiCommand) {
        match cmd {
            UiCommand::Stop => {
                if self.recording {
                    self.finish();
                }
            }
            UiCommand::Cancel => self.cancel(),
            UiCommand::Toggle => {
                if self.recording {
                    self.finish();
                } else {
                    self.start(Mode::Dictation, false);
                }
            }
            UiCommand::PasteLast => self.paste_last(),
            UiCommand::SetTargetLanguage(language) => {
                if !language.trim().is_empty() {
                    self.cfg.translation_target = language;
                    if let Err(err) = self
                        .paths
                        .ensure()
                        .and_then(|_| self.cfg.save(&self.paths.config_file()))
                    {
                        log_line!("[rustype] could not persist target language: {err}");
                    }
                    log_line!(
                        "[rustype] translation target → {}",
                        self.cfg.translation_target
                    );
                    self.set_status(if self.recording {
                        STATUS_RECORDING
                    } else {
                        STATUS_IDLE
                    });
                }
            }
            UiCommand::Reload(cfg) => {
                if self.recording {
                    self.cancel();
                }
                self.cfg = *cfg;
                match Engine::new(&self.cfg.hotkeys) {
                    Ok(engine) => self.engine = engine,
                    Err(err) => log_line!("[rustype] invalid hotkeys after reload: {err}"),
                }
                // Reopen the microphone so device/warmth settings apply.
                self.recorder = None;
                log_line!("[rustype] configuration reloaded");
            }
        }
    }

    pub fn handle_state(&mut self, state: &InputState) {
        // A recording started from the menu-bar button is not tracked by the
        // key engine, so Esc must be handled here as well.
        if self.recording && state.escape_down() {
            self.cancel();
        }
        let actions = self.engine.update(state, Instant::now());
        for action in actions {
            match action {
                Action::Start { mode, hands_free } => self.start(mode, hands_free),
                Action::SwitchMode(mode) => {
                    if self.recording {
                        self.mode = mode;
                        log_line!("[rustype] mode → {}", mode.as_str());
                        self.set_status(STATUS_RECORDING);
                    }
                }
                Action::Stop => self.finish(),
                Action::Cancel => self.cancel(),
                Action::PasteLast => self.paste_last(),
            }
        }
    }

    fn start(&mut self, mode: Mode, hands_free: bool) {
        if self.recording {
            return;
        }
        if self.processing.load(Ordering::SeqCst) {
            log_line!("[rustype] still processing the previous dictation");
            return;
        }
        self.mode = mode;
        self.last_context = context::capture();

        if self.recorder.is_none() {
            match Recorder::open(&self.cfg.audio.input_device, self.level.clone()) {
                Ok(rec) => {
                    log_line!(
                        "[rustype] microphone opened: {} @ {} Hz",
                        rec.device_name, rec.sample_rate
                    );
                    self.recorder = Some(rec);
                }
                Err(err) => {
                    log_line!("[rustype] microphone error: {err:#}");
                    play_sound("Basso");
                    self.set_status(STATUS_IDLE);
                    return;
                }
            }
        }

        if let Some(rec) = &self.recorder {
            rec.begin();
            let _ = hands_free;
            log_line!("[rustype] ● recording ({})", mode.as_str());
            self.recording = true;
            self.started = Some(Instant::now());
            self.set_status(STATUS_RECORDING);
            if self.cfg.audio.play_sounds {
                play_sound("Tink");
            }
        }
    }

    /// Stop the recorder and hand the audio to a worker thread.
    fn finish(&mut self) {
        if !self.recording {
            return;
        }
        self.recording = false;
        self.started = None;
        self.engine.abort_recording();
        if self.cfg.audio.play_sounds {
            play_sound("Pop");
        }
        let Some(rec) = &self.recorder else {
            return;
        };
        let (samples, rate) = rec.end();
        if !self.cfg.audio.keep_mic_warm {
            self.recorder = None;
        }
        let recorded_ms = duration_ms(samples.len(), rate);
        log_line!(
            "[rustype] ■ stopped — {:.2}s captured",
            recorded_ms as f64 / 1000.0
        );
        if recorded_ms < self.cfg.audio.min_duration_ms {
            log_line!(
                "[rustype] discarded: shorter than {} ms",
                self.cfg.audio.min_duration_ms
            );
            self.set_status(STATUS_IDLE);
            return;
        }

        let trimmed = trim_silence(&samples, rate, self.cfg.audio.silence_rms, 120);
        if trimmed.is_empty() {
            log_line!("[rustype] discarded: silence");
            self.set_status(STATUS_IDLE);
            return;
        }
        let gate = self.cfg.audio.silence_rms.max(0.0025);
        let voiced = voiced_ms(&trimmed, rate, gate, 20);
        if voiced < 120 {
            log_line!(
                "[rustype] discarded: no speech (voiced {voiced} ms, threshold {gate:.4})"
            );
            self.set_status(STATUS_IDLE);
            return;
        }
        log_line!(
            "[rustype] speech detected: {voiced} ms voiced ({:.2}s captured)",
            recorded_ms as f64 / 1000.0
        );
        self.set_status(STATUS_PROCESSING);
        let target = self.cfg.audio.sample_rate;
        let resampled = resample(&trimmed, rate, target);
        let wav = encode_wav(&to_i16(&resampled), target, 1);
        let processed_ms = duration_ms(resampled.len(), target);

        let mode = self.mode;
        let ctx = self.last_context.clone();
        let cfg = self.cfg.clone();
        let tx = self.result_tx.clone();
        let speech_ms = processed_ms;
        let flag = self.processing.clone();

        flag.store(true, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("rustype-worker".into())
            .spawn(move || {
                let result = process(cfg, mode, wav, ctx, speech_ms)
                    .map(|(text, ask, question)| (text, processed_ms, ask, question))
                    .map_err(|e| format!("{e:#}"));
                let _ = tx.send(result);
                flag.store(false, Ordering::SeqCst);
            })
            .expect("spawn worker");
    }

    fn cancel(&mut self) {
        self.engine.abort_recording();
        if self.recording {
            self.recording = false;
            if let Some(rec) = &self.recorder {
                let _ = rec.end();
            }
            if !self.cfg.audio.keep_mic_warm {
                self.recorder = None;
            }
            log_line!("[rustype] cancelled");
            if self.cfg.audio.play_sounds {
                play_sound("Basso");
            }
        }
        self.started = None;
        self.set_status(STATUS_IDLE);
    }

    pub fn paste_last(&mut self) {
        let text = self
            .last_text
            .clone()
            .or_else(|| history::last_text(&self.paths.history_file()));
        match text {
            Some(text) => {
                let cfg = &self.cfg.insert;
                if let Err(e) = insert::insert_text(
                    &text,
                    &cfg.method,
                    cfg.restore_clipboard,
                    cfg.restore_delay_ms,
                ) {
                    log_line!("[rustype] paste failed: {e:#}");
                }
            }
            None => log_line!("[rustype] no previous transcript"),
        }
    }

    /// Called regularly by the main loop.
    pub fn tick(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            self.handle_ui_command(cmd);
        }

        if let Some(started) = self.started {
            if self.recording
                && started.elapsed().as_millis() as u64 > self.cfg.audio.max_duration_ms
            {
                log_line!("[rustype] maximum duration reached");
                self.finish();
            }
        }

        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                Ok((text, duration_ms, ask, question)) => {
                    self.deliver(text, duration_ms, ask, question)
                }
                Err(err) => {
                    log_line!("[rustype] transcription failed: {err}");
                    if self.cfg.audio.play_sounds {
                        play_sound("Basso");
                    }
                    self.set_status(STATUS_IDLE);
                }
            }
        }
    }

    fn deliver(&mut self, text: String, duration_ms: u64, ask: bool, question: String) {
        self.set_status(STATUS_IDLE);
        let text = text.trim().to_string();
        if text.is_empty() {
            log_line!("[rustype] empty transcript");
            return;
        }
        if ask {
            log_line!("[rustype] answer card ({} chars)", text.chars().count());
            let _ = self.answer_tx.send(AnswerPayload {
                question,
                answer: text.clone(),
            });
        }
        let cfg = &self.cfg.insert;
        match insert::insert_text(
            &text,
            &cfg.method,
            cfg.restore_clipboard,
            cfg.restore_delay_ms,
        ) {
            Ok(()) => log_line!("[rustype] inserted {} chars", text.chars().count()),
            Err(err) => {
                log_line!("[rustype] insertion failed: {err:#}");
                println!("{text}");
            }
        }
        self.last_text = Some(text.clone());
        if self.cfg.history_enabled {
            let entry = HistoryEntry::new(
                self.mode.as_str(),
                &self.last_context.app_name,
                duration_ms,
                &text,
            );
            if let Err(e) = history::append(&self.paths.history_file(), &entry) {
                log_line!("[rustype] history write failed: {e}");
            }
        }
    }
}

fn process(
    cfg: Config,
    mode: Mode,
    wav: Vec<u8>,
    ctx: VoiceContext,
    duration_ms: u64,
) -> Result<(String, bool, String)> {
    let asr = AsrClient::new(&cfg.provider)?;
    let transcript = asr
        .transcribe_wav(wav, None)
        .context("speech-to-text request failed")?;
    if transcript.trim().is_empty() {
        return Ok((String::new(), false, String::new()));
    }
    let has_selection = !ctx.selected_text.trim().is_empty();
    let question = guard::looks_like_question(&transcript);
    let edit = guard::looks_like_edit_request(&transcript);
    // Ask anything is the default for command mode; only a clear edit
    // instruction with a selection replaces the selection in place.
    let ask = mode == Mode::Command && (!has_selection || question || !edit);
    if guard::plausible_transcript(&transcript, duration_ms).is_none() {
        return Err(anyhow::anyhow!(
            "ASR returned {} chars for {:.1}s of audio; treated as hallucination",
            transcript.chars().count(),
            duration_ms as f64 / 1000.0
        ));
    }
    if !cfg.llm.enabled {
        return Ok((transcript.clone(), ask, transcript));
    }
    let messages = prompt::build_messages(&cfg, mode, &transcript, &ctx);
    match LlmClient::new(&cfg.provider, &cfg.llm)?.complete(&messages) {
        Ok(text) if !text.trim().is_empty() => Ok((
            guard::accept_cleanup(&transcript, &text)
                .map(str::to_string)
                .unwrap_or_else(|| transcript.clone()),
            ask,
            transcript,
        )),
        Ok(_) => Ok((transcript.clone(), ask, transcript)),
        Err(err) => {
            log_line!("[rustype] cleanup failed, using raw transcript: {err:#}");
            Ok((transcript.clone(), ask, transcript))
        }
    }
}

pub fn play_sound(name: &str) {
    let path = format!("/System/Library/Sounds/{name}.aiff");
    if std::path::Path::new(&path).exists() {
        let _ = std::process::Command::new("afplay")
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}
