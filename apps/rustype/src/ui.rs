//! Persistent menu-bar button (NSStatusItem) for rustype.
//!
//! Left mouse down starts a dictation, release stops it — the icon works like
//! a push-to-talk button. Right click opens a small menu.

use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply, NSImage};
use objc2_foundation::NSString;
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopWindowTargetExtMacOS};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use rustype_core::config::{Config, Paths};
use rustype_core::hotkey::Mode;

use crate::answer_card::AnswerCard;
use crate::ask_bar::AskBar;
use crate::pill::Pill;
use crate::session::{AnswerPayload, UiCommand, UiStatus, STATUS_PROCESSING, STATUS_RECORDING};
use crate::translate_bar::TranslationBar;

pub enum UserEvent {
    Menu(MenuEvent),
    Status(UiStatus),
    Answer(AnswerPayload),
}

const IDLE_TOOLTIP: &str = "rustype · 按住图标说话";

fn icon_round_rect(cov: &mut [f32], n: usize, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
    let lo_x = (x0 + r).min(x1 - r);
    let hi_x = (x0 + r).max(x1 - r);
    let lo_y = (y0 + r).min(y1 - r);
    let hi_y = (y0 + r).max(y1 - r);
    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / 4.0;
            let py = (y as f32 + 0.5) / 4.0;
            let cx = px.max(lo_x).min(hi_x);
            let cy = py.max(lo_y).min(hi_y);
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= r * r {
                cov[y * n + x] = 1.0;
            }
        }
    }
}

fn icon_disc(cov: &mut [f32], n: usize, cx: f32, cy: f32, r: f32) {
    for y in 0..n {
        for x in 0..n {
            let px = (x as f32 + 0.5) / 4.0;
            let py = (y as f32 + 0.5) / 4.0;
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= r * r {
                cov[y * n + x] = 1.0;
            }
        }
    }
}

fn icon_arc(
    cov: &mut [f32],
    n: usize,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    a0: f32,
    a1: f32,
) {
    let steps = 90;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = (a0 + (a1 - a0) * t).to_radians();
        icon_disc(
            cov,
            n,
            cx + radius * angle.cos(),
            cy + radius * angle.sin(),
            width / 2.0,
        );
    }
}

fn mic_icon() -> Icon {
    // "Mic inside telegraph waves" as a monochrome template, 4x supersampled.
    const PT: usize = 22;
    const SS: usize = 4;
    let n = PT * SS;
    let mut cov = vec![0f32; n * n];

    // One clean signal arc per side keeps the menu bar mark legible at 22pt.
    icon_arc(&mut cov, n, 11.0, 10.6, 8.0, 1.05, -48.0, 48.0);
    icon_arc(&mut cov, n, 11.0, 10.6, 8.0, 1.05, 132.0, 228.0);
    icon_round_rect(&mut cov, n, 8.6, 3.2, 13.4, 11.6, 2.4);
    icon_arc(&mut cov, n, 11.0, 11.9, 3.3, 1.4, 18.0, 162.0);
    icon_round_rect(&mut cov, n, 10.4, 14.6, 11.6, 16.4, 0.6);
    icon_round_rect(&mut cov, n, 8.2, 16.2, 13.8, 17.4, 0.6);

    let mut rgba = vec![0u8; PT * PT * 4];
    for y in 0..PT {
        for x in 0..PT {
            let mut sum = 0.0;
            for dy in 0..SS {
                for dx in 0..SS {
                    sum += cov[(y * SS + dy) * n + x * SS + dx];
                }
            }
            let alpha = (sum / (SS * SS) as f32 * 255.0) as u8;
            let i = (y * PT + x) * 4;
            rgba[i..i + 4].copy_from_slice(&[255, 255, 255, alpha]);
        }
    }
    Icon::from_rgba(rgba, PT as u32, PT as u32).expect("valid icon")
}

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeAppDelegate"]
    struct AppDelegate;

    impl AppDelegate {
        /// A menu-bar-only app must not quit just because it has no windows /
        /// because the system asks it to terminate quietly.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _sender: &AnyObject) -> NSApplicationTerminateReply {
            log_line!("[rustype] ignored termination request (menu bar app)");
            NSApplicationTerminateReply::TerminateCancel
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn after_last_window(&self, _sender: &AnyObject) -> bool {
            false
        }
    }
);

/// Point AppKit at the bundle icon so system UI (Dock, mic menu, prompts)
/// does not fall back to a generic placeholder when launched from a terminal.
fn set_app_icon() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(contents) = exe.parent().and_then(|p| p.parent()) else {
        return;
    };
    let path = contents.join("Resources/icon.icns");
    if !path.exists() {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let ns = NSString::from_str(&path.to_string_lossy());
    if let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns) {
        unsafe { app.setApplicationIconImage(Some(&image)) };
    }
}

pub fn run(
    cmd_tx: Sender<UiCommand>,
    status_rx: Receiver<UiStatus>,
    answer_rx: Receiver<AnswerPayload>,
    config_path: PathBuf,
    level: Arc<AtomicU32>,
    cfg: Config,
    paths: Paths,
) -> ! {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Forward session status updates into the event loop.
    {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            for status in status_rx {
                let _ = proxy.send_event(UserEvent::Status(status));
            }
        });
    }
    {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            for payload in answer_rx {
                let _ = proxy.send_event(UserEvent::Answer(payload));
            }
        });
    }

    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }
    }));

    let mut tray: Option<TrayIcon> = None;
    let mut pill: Option<Pill> = None;
    let mut lang_bar: Option<TranslationBar> = None;
    let mut ask_bar: Option<AskBar> = None;
    let mut answer_card: Option<AnswerCard> = None;
    let mut toggle_item: Option<MenuItem> = None;
    let mut paste_item: Option<MenuItem> = None;
    let mut settings_item: Option<MenuItem> = None;
    let mut open_item: Option<MenuItem> = None;
    let mut access_item: Option<MenuItem> = None;
    let mut quit_item: Option<MenuItem> = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => {
                target.set_activation_policy_at_runtime(ActivationPolicy::Accessory);
                set_app_icon();
                if let Some(mtm) = MainThreadMarker::new() {
                    let delegate = AppDelegate::alloc(mtm);
                    let delegate: Retained<AppDelegate> =
                        unsafe { objc2::msg_send![delegate, init] };
                    let app = NSApplication::sharedApplication(mtm);
                    let _: () = unsafe { objc2::msg_send![&app, setDelegate: &*delegate] };
                    // NSApplication keeps an unretained delegate; leak it for
                    // the lifetime of the process.
                    std::mem::forget(delegate);
                }

                let menu = Menu::new();
                let toggle = MenuItem::new("开始 / 停止听写", true, None);
                let paste = MenuItem::new("粘贴上一条", true, None);
                let settings = MenuItem::new("设置…", true, None);
                let open = MenuItem::new("打开配置文件", true, None);
                let access = MenuItem::new("辅助功能授权…", true, None);
                let quit = MenuItem::new("退出 rustype", true, None);
                let _ = menu.append_items(&[
                    &toggle,
                    &paste,
                    &PredefinedMenuItem::separator(),
                    &settings,
                    &open,
                    &access,
                    &PredefinedMenuItem::separator(),
                    &quit,
                ]);

                match TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_tooltip(IDLE_TOOLTIP)
                    .with_icon(mic_icon())
                    .with_icon_as_template(true)
                    .build()
                {
                    Ok(t) => {
                        log_line!("[rustype] 菜单栏图标已就绪");
                        pill = Some(Pill::new(cmd_tx.clone(), level.clone()));
                        lang_bar = Some(TranslationBar::new(cmd_tx.clone()));
                        ask_bar = Some(AskBar::new());
                        answer_card = Some(AnswerCard::new());
                        log_line!("[rustype] 录音悬浮条已就绪");
                        tray = Some(t);
                        toggle_item = Some(toggle);
                        paste_item = Some(paste);
                        settings_item = Some(settings);
                        open_item = Some(open);
                        access_item = Some(access);
                        quit_item = Some(quit);
                    }
                    Err(err) => {
                        log_line!("[rustype] 无法创建菜单栏图标: {err}");
                        std::process::exit(1);
                    }
                }
            }
            Event::UserEvent(UserEvent::Status(status)) => {
                if let Some(pill) = &pill {
                    match status.state {
                        STATUS_RECORDING => {
                            pill.set_recording();
                            pill.show();
                        }
                        STATUS_PROCESSING => {
                            pill.set_thinking();
                            pill.show();
                        }
                        _ => pill.hide(),
                    }
                }
                if let Some(lang_bar) = &lang_bar {
                    if status.state == STATUS_RECORDING && status.mode == Mode::Translation {
                        lang_bar.show(&status.languages, &status.target);
                    } else {
                        lang_bar.hide();
                    }
                }
                if let Some(ask_bar) = &ask_bar {
                    if status.state == STATUS_RECORDING && status.mode == Mode::Command {
                        ask_bar.show();
                    } else {
                        ask_bar.hide();
                    }
                }
                if let Some(tray) = &tray {
                    match status.state {
                        STATUS_RECORDING => {
                            let _ = tray.set_title(Some("●"));
                            let _ = tray.set_tooltip(Some("rustype · 录音中"));
                        }
                        STATUS_PROCESSING => {
                            let _ = tray.set_title(Some("…"));
                            let _ = tray.set_tooltip(Some("rustype · 转写中…"));
                        }
                        _ => {
                            // tray-icon only forwards Some(_), so clear with an
                            // empty string rather than None.
                            let _ = tray.set_title(Some(""));
                            let _ = tray.set_tooltip(Some(IDLE_TOOLTIP));
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::Answer(payload)) => {
                if let Some(card) = &answer_card {
                    card.show(&payload.question, &payload.answer);
                }
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if let Some(item) = &toggle_item {
                    if event.id() == item.id() {
                        let _ = cmd_tx.send(UiCommand::Toggle);
                    }
                }
                if let Some(item) = &paste_item {
                    if event.id() == item.id() {
                        let _ = cmd_tx.send(UiCommand::PasteLast);
                    }
                }
                if let Some(item) = &settings_item {
                    if event.id() == item.id() {
                        let current = rustype_core::config::Config::load_or_create(&paths)
                            .unwrap_or_else(|_| cfg.clone());
                        crate::settings::open(current, paths.clone(), cmd_tx.clone());
                    }
                }
                if let Some(item) = &open_item {
                    if event.id() == item.id() {
                        let _ = std::process::Command::new("open")
                            .arg("-t")
                            .arg(&config_path)
                            .spawn();
                    }
                }
                if let Some(item) = &access_item {
                    if event.id() == item.id() {
                        let _ = std::process::Command::new("open")
                            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
                            .spawn();
                    }
                }
                if let Some(item) = &quit_item {
                    if event.id() == item.id() {
                        log_line!("[rustype] quit menu action");
                        std::process::exit(0);
                    }
                }
            }
            _ => {}
        }
    })
}
