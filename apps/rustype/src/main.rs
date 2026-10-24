#[macro_use]
mod logging;
mod answer_card;
mod ask_bar;
mod macos;
mod pill;
mod session;
mod settings;
mod translate_bar;
mod ui;

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rustype_core::audio::{encode_wav, parse_wav, resample, to_i16};
use rustype_core::config::{Config, Paths};
use rustype_core::hotkey::Shortcut;
use rustype_core::prompt::{self, Context as VoiceContext};
use rustype_core::{asr::AsrClient, llm::LlmClient};

use session::Session;

#[derive(Parser)]
#[command(
    name = "rustype",
    version,
    about = "A Rust reimplementation of Typeless-style AI dictation for macOS"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Listen for the dictation hotkey and insert transcribed text.
    Run {
        /// Do not create the menu-bar button (headless/launchd mode).
        #[arg(long)]
        no_tray: bool,
    },
    /// Transcribe an audio file (wav, 16-bit PCM or float32) and print the result.
    Transcribe { file: PathBuf },
    /// List available audio input devices.
    Devices,
    /// Check permissions, configuration and provider settings.
    Doctor,
    /// Print the configuration path and contents, optionally creating it.
    Config {
        #[arg(long)]
        init: bool,
    },
    /// Insert text into the frontmost application (tests the insertion path).
    Insert { text: String },
    /// Open the settings panel as a standalone window.
    Settings,
    /// Show the answer card with the given text (useful for testing).
    Answer { text: String },
    /// Preview the "ask anything" hint bar.
    Askbar,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    if let Err(err) = real_main() {
        log_line!("error: {err:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::application_support();
    match cli.command.unwrap_or(Command::Run { no_tray: false }) {
        Command::Run { no_tray } => run(paths, !no_tray),
        Command::Transcribe { file } => transcribe(paths, file),
        Command::Devices => devices(),
        Command::Doctor => doctor(paths),
        Command::Config { init } => config(paths, init),
        Command::Insert { text } => {
            let cfg = Config::load_or_create(&paths)?;
            macos::insert::insert_text(
                &text,
                &cfg.insert.method,
                cfg.insert.restore_clipboard,
                cfg.insert.restore_delay_ms,
            )
        }
        Command::Settings => open_settings(paths),
        Command::Answer { text } => show_answer(text),
        Command::Askbar => show_askbar(),
    }
}

fn run(paths: Paths, tray: bool) -> Result<()> {
    // A menu-bar-only app has no windows; without this macOS "automatic
    // termination" can kill it a few seconds after a Finder launch.
    {
        let process = objc2_foundation::NSProcessInfo::processInfo();
        process.disableAutomaticTermination(&objc2_foundation::NSString::from_str(
            "Rustype lives in the menu bar",
        ));
        process.disableSuddenTermination();
    }

    let cfg = Config::load_or_create(&paths)?;
    for warning in cfg.validate() {
        log_line!("[rustype] config warning: {warning}");
    }
    if !macos::permissions::accessibility_trusted() {
        macos::permissions::prompt_accessibility();
        log_line!("[rustype] menu bar is up; grant Accessibility to enable hotkeys");
    }

    let shortcuts = configured_shortcuts(&cfg);
    let events = macos::tap::spawn(shortcuts).context("installing the keyboard event tap")?;

    if cfg.hotkeys.mode == "hold" {
        log_line!(
            "[rustype] ready — hold {} to dictate, double-tap for hands-free",
            cfg.hotkeys.dictation
        );
    } else {
        log_line!(
            "[rustype] ready — tap {} to start, tap again to stop",
            cfg.hotkeys.dictation
        );
    }
    log_line!(
        "[rustype] {} = command, {} = translate, {} = paste last, Esc = cancel",
        cfg.hotkeys.command, cfg.hotkeys.translation, cfg.hotkeys.paste_last
    );

    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (status_tx, status_rx) = crossbeam_channel::unbounded();
    let (answer_tx, answer_rx) = crossbeam_channel::unbounded();
    let level = Arc::new(AtomicU32::new(0));

    if tray {
        let session_cfg = cfg.clone();
        let session_paths = paths.clone();
        let session_level = level.clone();
        std::thread::Builder::new()
            .name("rustype-session".into())
            .spawn(move || {
                let mut session = match Session::new(
                    session_cfg,
                    session_paths,
                    cmd_rx,
                    status_tx,
                    answer_tx,
                    session_level,
                ) {
                    Ok(session) => session,
                    Err(err) => {
                        log_line!("[rustype] session failed to start: {err:#}");
                        return;
                    }
                };
                loop {
                    match events.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(state) => session.handle_state(&state),
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                        Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                    }
                    session.tick();
                }
            })
            .context("spawning the session thread")?;

        ui::run(
            cmd_tx,
            status_rx,
            answer_rx,
            paths.config_file(),
            level,
            cfg.clone(),
            paths.clone(),
        );
    } else {
        let mut session = Session::new(cfg, paths, cmd_rx, status_tx, answer_tx, level)?;
        loop {
            match events.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(state) => session.handle_state(&state),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    bail!("event tap stopped")
                }
            }
            session.tick();
        }
    }
}

/// Every configured chord, used by the tap to know which character keys to
/// swallow and which right-hand modifiers to track.
fn configured_shortcuts(cfg: &Config) -> Vec<Shortcut> {
    [
        &cfg.hotkeys.dictation,
        &cfg.hotkeys.command,
        &cfg.hotkeys.translation,
        &cfg.hotkeys.paste_last,
    ]
    .into_iter()
    .filter_map(|spec| Shortcut::parse(spec).ok())
    .collect()
}

fn transcribe(paths: Paths, file: PathBuf) -> Result<()> {
    let cfg = Config::load_or_create(&paths)?;
    let bytes = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
    let (samples, rate, channels) = parse_wav(&bytes).context("parsing wav")?;
    let mono: Vec<f32> = if channels > 1 {
        rustype_core::audio::downmix(&samples, channels)
    } else {
        samples
    };
    let resampled = resample(&mono, rate, cfg.audio.sample_rate);
    let wav = encode_wav(&to_i16(&resampled), cfg.audio.sample_rate, 1);
    let asr = AsrClient::new(&cfg.provider)?;
    let transcript = asr.transcribe_wav(wav, None)?;
    let text = if cfg.llm.enabled {
        let messages = prompt::build_messages(
            &cfg,
            rustype_core::hotkey::Mode::Dictation,
            &transcript,
            &VoiceContext::default(),
        );
        LlmClient::new(&cfg.provider, &cfg.llm)?
            .complete(&messages)
            .unwrap_or(transcript)
    } else {
        transcript
    };
    println!("{text}");
    Ok(())
}

fn open_settings(paths: Paths) -> Result<()> {
    let cfg = Config::load_or_create(&paths)?;
    let (tx, _rx) = crossbeam_channel::unbounded();
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("settings must open on the main thread"))?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    // A standalone CLI process is not a bundled app; a Regular policy gives
    // the window a Dock presence and lets it come to the front.
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
    settings::open(cfg, paths, tx);
    app.run();
    Ok(())
}

fn show_answer(text: String) -> Result<()> {
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("answer card must open on the main thread"))?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
    let card = answer_card::AnswerCard::new();
    card.show("（预览）这段内容怎么理解？", &text);
    app.run();
    Ok(())
}

fn show_askbar() -> Result<()> {
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| anyhow::anyhow!("ask bar must open on the main thread"))?;
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
    let bar = ask_bar::AskBar::new();
    bar.show();
    app.run();
    Ok(())
}

fn devices() -> Result<()> {
    let devices = macos::audio::input_devices()?;
    if devices.is_empty() {
        println!("no input devices found");
    }
    for (i, name) in devices.iter().enumerate() {
        println!("{i}: {name}");
    }
    Ok(())
}

fn doctor(paths: Paths) -> Result<()> {
    let cfg = Config::load_or_create(&paths)?;
    println!("rustype {}", env!("CARGO_PKG_VERSION"));
    println!("config: {}", paths.config_file().display());
    println!("provider: {}", cfg.provider.resolved_base_url());
    println!("asr model: {}", cfg.provider.asr_model);
    println!(
        "llm model: {} (enabled: {})",
        cfg.provider.llm_model, cfg.llm.enabled
    );
    println!(
        "api key: {}",
        if cfg.provider.resolved_api_key().is_empty() {
            "MISSING"
        } else {
            "set"
        }
    );
    let trusted = macos::permissions::accessibility_trusted();
    println!(
        "accessibility: {}",
        if trusted { "granted" } else { "NOT granted" }
    );
    if trusted && std::env::var_os("TERM_PROGRAM").is_some() {
        println!(
            "note: when run from a terminal, macOS attributes Accessibility to the terminal; \
             a LaunchAgent binary needs its own permission entry"
        );
    }
    match macos::audio::input_devices() {
        Ok(devices) => {
            println!("input devices: {}", devices.len());
            for name in devices {
                println!("  - {name}");
            }
        }
        Err(err) => println!("input devices: error ({err:#})"),
    }
    let warnings = cfg.validate();
    if warnings.is_empty() {
        println!("config: ok");
    } else {
        for warning in warnings {
            println!("config warning: {warning}");
        }
    }
    Ok(())
}

fn config(paths: Paths, init: bool) -> Result<()> {
    if init {
        paths.ensure()?;
        Config::default().save(&paths.config_file())?;
        println!("wrote {}", paths.config_file().display());
    }
    let cfg = Config::load_or_create(&paths)?;
    println!("# {}", paths.config_file().display());
    print!("{}", toml::to_string_pretty(&cfg)?);
    Ok(())
}
