//! Native settings panel for the rustype configuration file.

use std::cell::RefCell;

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSApplication, NSBackingStoreType, NSButton, NSFont, NSPopUpButton, NSSecureTextField,
    NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};
use rustype_core::config::{Config, Paths};

use crate::session::UiCommand;

const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 694.0;
const ROW_H: f64 = 34.0;
const FIRST_ROW_Y: f64 = 644.0;

struct Form {
    cfg: Config,
    paths: Paths,
    tx: Sender<UiCommand>,
    base_url: Retained<NSTextField>,
    api_key: Retained<NSSecureTextField>,
    asr_model: Retained<NSTextField>,
    llm_model: Retained<NSTextField>,
    proxy: Retained<NSTextField>,
    trigger: Retained<NSPopUpButton>,
    dictation: Retained<NSTextField>,
    command: Retained<NSTextField>,
    translation: Retained<NSTextField>,
    paste_last: Retained<NSTextField>,
    target_language: Retained<NSTextField>,
    translation_languages: Retained<NSTextField>,
    device: Retained<NSPopUpButton>,
    device_names: Vec<String>,
    insert_method: Retained<NSPopUpButton>,
    keep_warm: Retained<NSButton>,
    llm_enabled: Retained<NSButton>,
    _controller: Retained<SettingsController>,
}

thread_local! {
    static FORM: RefCell<Option<Form>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeSettingsController"]
    struct SettingsController;

    impl SettingsController {
        #[unsafe(method(save:))]
        fn save(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(save_form_inner);
        }

        #[unsafe(method(cancel:))]
        fn cancel(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(|| {
                WINDOW.with(|window| {
                    if let Some(window) = window.borrow().as_ref() {
                        window.orderOut(None);
                    }
                });
            });
        }
    }
);

fn any_ref<T: objc2::Message + 'static>(obj: &T) -> &AnyObject {
    unsafe { &*(obj as *const T as *const AnyObject) }
}

fn label(text: &str, x: f64, y: f64, w: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    label.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 20.0)));
    label
}

fn text_field(value: &str, x: f64, y: f64, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(370.0, 24.0)));
    field
}

fn open_form(cfg: Config, paths: Paths, tx: Sender<UiCommand>) {
    let mtm = MainThreadMarker::new().expect("settings must open on the main thread");

    if let Some(window) = WINDOW.with(|w| w.borrow().clone()) {
        FORM.with(|form| {
            if let Some(form) = form.borrow_mut().as_mut() {
                form.cfg = cfg;
                form.paths = paths;
                form.tx = tx;
            }
        });
        refresh();
        activate(&window);
        return;
    }

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str("rustype 设置"));
    unsafe { window.setReleasedWhenClosed(false) };
    window.center();
    let content = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
    );
    window.setContentView(Some(&content));

    let controller = SettingsController::alloc(mtm);
    let controller: Retained<SettingsController> = unsafe { objc2::msg_send![controller, init] };
    let target = any_ref(&*controller);

    let save_button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("保存"),
            Some(target),
            Some(sel!(save:)),
            mtm,
        )
    };
    save_button.setFrame(NSRect::new(
        NSPoint::new(355.0, 24.0),
        NSSize::new(90.0, 32.0),
    ));
    save_button.setKeyEquivalent(&NSString::from_str("\r"));

    let cancel_button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str("取消"),
            Some(target),
            Some(sel!(cancel:)),
            mtm,
        )
    };
    cancel_button.setFrame(NSRect::new(
        NSPoint::new(450.0, 24.0),
        NSSize::new(90.0, 32.0),
    ));

    let mut y = FIRST_ROW_Y;
    let mut row = |title: &str| {
        let l = label(title, 20.0, y + 2.0, 130.0, mtm);
        content.addSubview(&l);
        let y2 = y;
        y -= ROW_H;
        y2
    };

    let y_base = row("服务地址");
    let base_url = text_field(&cfg.provider.base_url, 160.0, y_base, mtm);
    content.addSubview(&base_url);

    let y_key = row("API Key");
    let api_key = NSSecureTextField::new(mtm);
    api_key.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    api_key.setStringValue(&NSString::from_str(&cfg.provider.resolved_api_key()));
    api_key.setFrame(NSRect::new(
        NSPoint::new(160.0, y_key),
        NSSize::new(370.0, 24.0),
    ));
    content.addSubview(&api_key);

    let y_asr = row("转写模型");
    let asr_model = text_field(&cfg.provider.asr_model, 160.0, y_asr, mtm);
    content.addSubview(&asr_model);

    let y_llm = row("润色模型");
    let llm_model = text_field(&cfg.provider.llm_model, 160.0, y_llm, mtm);
    content.addSubview(&llm_model);

    let y_proxy = row("代理（可空）");
    let proxy = text_field(&cfg.provider.proxy, 160.0, y_proxy, mtm);
    content.addSubview(&proxy);

    let y_trigger = row("触发方式");
    let trigger = NSPopUpButton::new(mtm);
    let trigger_titles = [
        NSString::from_str("点击切换（tap）"),
        NSString::from_str("按住说话（hold）"),
    ];
    let trigger_refs: Vec<&NSString> = trigger_titles.iter().map(|t| &**t).collect();
    trigger.addItemsWithTitles(&NSArray::from_slice(&trigger_refs));
    trigger.setFrame(NSRect::new(
        NSPoint::new(160.0, y_trigger),
        NSSize::new(200.0, 26.0),
    ));
    content.addSubview(&trigger);

    let y_dict = row("听写快捷键");
    let dictation = text_field(&cfg.hotkeys.dictation, 160.0, y_dict, mtm);
    content.addSubview(&dictation);

    let y_cmd = row("提问/命令快捷键");
    let command = text_field(&cfg.hotkeys.command, 160.0, y_cmd, mtm);
    content.addSubview(&command);

    let y_trans = row("翻译快捷键");
    let translation = text_field(&cfg.hotkeys.translation, 160.0, y_trans, mtm);
    content.addSubview(&translation);

    let y_paste = row("粘贴上一条");
    let paste_last = text_field(&cfg.hotkeys.paste_last, 160.0, y_paste, mtm);
    content.addSubview(&paste_last);

    let y_target = row("翻译目标语言");
    let target_language = text_field(&cfg.translation_target, 160.0, y_target, mtm);
    content.addSubview(&target_language);

    let y_langs = row("翻译语言列表");
    let translation_languages = text_field(
        &cfg.translation_languages.join(", "),
        160.0,
        y_langs,
        mtm,
    );
    translation_languages.setPlaceholderString(Some(&NSString::from_str(
        "留空 = 全部；例：English (United States), Chinese (Simplified)",
    )));
    content.addSubview(&translation_languages);

    let y_device = row("麦克风");
    let device = NSPopUpButton::new(mtm);
    let mut device_names = vec!["系统默认".to_string()];
    if let Ok(devices) = crate::macos::audio::input_devices() {
        device_names.extend(devices);
    }
    let titles: Vec<Retained<NSString>> = device_names
        .iter()
        .map(|name| NSString::from_str(name))
        .collect();
    let title_refs: Vec<&NSString> = titles.iter().map(|t| &**t).collect();
    device.addItemsWithTitles(&NSArray::from_slice(&title_refs));
    device.setFrame(NSRect::new(
        NSPoint::new(160.0, y_device),
        NSSize::new(370.0, 26.0),
    ));
    content.addSubview(&device);

    let y_method = row("写回方式");
    let insert_method = NSPopUpButton::new(mtm);
    let method_titles = [
        NSString::from_str("粘贴（paste）"),
        NSString::from_str("合成键入（type）"),
    ];
    let method_refs: Vec<&NSString> = method_titles.iter().map(|t| &**t).collect();
    insert_method.addItemsWithTitles(&NSArray::from_slice(&method_refs));
    insert_method.setFrame(NSRect::new(
        NSPoint::new(160.0, y_method),
        NSSize::new(200.0, 26.0),
    ));
    content.addSubview(&insert_method);

    let y_warm = row("常驻麦克风");
    let keep_warm = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str("减少启动延迟"),
            None,
            None,
            mtm,
        )
    };
    keep_warm.setFrame(NSRect::new(
        NSPoint::new(160.0, y_warm),
        NSSize::new(260.0, 22.0),
    ));
    content.addSubview(&keep_warm);

    let y_enabled = row("启用润色");
    let llm_enabled = unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str("调用 LLM 清理文本"),
            None,
            None,
            mtm,
        )
    };
    llm_enabled.setFrame(NSRect::new(
        NSPoint::new(160.0, y_enabled),
        NSSize::new(260.0, 22.0),
    ));
    content.addSubview(&llm_enabled);

    let hint = label(
        "保存后立即生效；翻译语言留空表示显示全部内置语言。",
        20.0,
        80.0,
        400.0,
        mtm,
    );
    hint.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    hint.setTextColor(Some(&objc2_app_kit::NSColor::secondaryLabelColor()));
    content.addSubview(&hint);

    content.addSubview(&save_button);
    content.addSubview(&cancel_button);

    FORM.with(|form| {
        *form.borrow_mut() = Some(Form {
            cfg,
            paths,
            tx,
            base_url,
            api_key,
            asr_model,
            llm_model,
            proxy,
            dictation,
            command,
            translation,
            paste_last,
            target_language,
            translation_languages,
            device,
            device_names,
            insert_method,
            trigger,
            keep_warm,
            llm_enabled,
            _controller: controller,
        });
    });
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));

    refresh_values();
    activate(&window);
}

fn activate(window: &NSWindow) {
    window.orderFrontRegardless();
    window.makeKeyAndOrderFront(None);
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

fn refresh() {
    refresh_values();
}

fn refresh_values() {
    FORM.with(|form| {
        if let Some(form) = form.borrow().as_ref() {
            let cfg = &form.cfg;
            form.base_url
                .setStringValue(&NSString::from_str(&cfg.provider.base_url));
            form.api_key
                .setStringValue(&NSString::from_str(&cfg.provider.resolved_api_key()));
            form.asr_model
                .setStringValue(&NSString::from_str(&cfg.provider.asr_model));
            form.llm_model
                .setStringValue(&NSString::from_str(&cfg.provider.llm_model));
            form.proxy
                .setStringValue(&NSString::from_str(&cfg.provider.proxy));
            form.trigger
                .selectItemAtIndex(if cfg.hotkeys.mode == "hold" { 1 } else { 0 });
            form.dictation
                .setStringValue(&NSString::from_str(&cfg.hotkeys.dictation));
            form.command
                .setStringValue(&NSString::from_str(&cfg.hotkeys.command));
            form.translation
                .setStringValue(&NSString::from_str(&cfg.hotkeys.translation));
            form.paste_last
                .setStringValue(&NSString::from_str(&cfg.hotkeys.paste_last));
            form.target_language
                .setStringValue(&NSString::from_str(&cfg.translation_target));
            form.translation_languages.setStringValue(&NSString::from_str(
                &cfg.translation_languages.join(", "),
            ));

            let index = if cfg.audio.input_device.trim().is_empty() {
                0
            } else {
                form.device_names
                    .iter()
                    .position(|name| name == &cfg.audio.input_device)
                    .unwrap_or(0)
            };
            form.device.selectItemAtIndex(index as isize);
            form.insert_method
                .selectItemAtIndex(if cfg.insert.method == "type" { 1 } else { 0 });
            form.keep_warm
                .setState(if cfg.audio.keep_mic_warm { 1 } else { 0 });
            form.llm_enabled
                .setState(if cfg.llm.enabled { 1 } else { 0 });
        }
    });
}

fn save_form_inner() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let mut error: Option<String> = None;
    let mut save_error: Option<String> = None;
    let mut saved = false;

    {
        FORM.with(|form| {
            let mut slot = form.borrow_mut();
            let Some(form) = slot.as_mut() else {
                return;
            };
            let mut cfg = form.cfg.clone();
            cfg.provider.base_url = form.base_url.stringValue().to_string();
            cfg.provider.api_key = form.api_key.stringValue().to_string();
            cfg.provider.asr_model = form.asr_model.stringValue().to_string();
            cfg.provider.llm_model = form.llm_model.stringValue().to_string();
            cfg.provider.proxy = form.proxy.stringValue().to_string();
            cfg.hotkeys.mode = if form.trigger.indexOfSelectedItem() == 1 {
                "hold".into()
            } else {
                "toggle".into()
            };
            cfg.hotkeys.dictation = form.dictation.stringValue().to_string();
            cfg.hotkeys.command = form.command.stringValue().to_string();
            cfg.hotkeys.translation = form.translation.stringValue().to_string();
            cfg.hotkeys.paste_last = form.paste_last.stringValue().to_string();
            cfg.translation_target = form.target_language.stringValue().to_string();
            cfg.translation_languages = form
                .translation_languages
                .stringValue()
                .to_string()
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect();
            cfg.insert.method = if form.insert_method.indexOfSelectedItem() == 1 {
                "type".into()
            } else {
                "paste".into()
            };
            cfg.audio.keep_mic_warm = form.keep_warm.state() == 1;
            cfg.llm.enabled = form.llm_enabled.state() == 1;

            let index = form.device.indexOfSelectedItem();
            cfg.audio.input_device = if index <= 0 {
                String::new()
            } else {
                form.device_names
                    .get(index as usize)
                    .cloned()
                    .unwrap_or_default()
            };

            let warnings = cfg.validate();
            if !warnings.is_empty() {
                error = Some(warnings.join("\n"));
                return;
            }

            let result = form
                .paths
                .ensure()
                .and_then(|_| cfg.save(&form.paths.config_file()));
            if let Err(err) = result {
                save_error = Some(err.to_string());
                return;
            }

            form.cfg = cfg.clone();
            let _ = form.tx.send(UiCommand::Reload(Box::new(cfg)));
            saved = true;
        });
    }

    if saved {
        refresh_values();
        WINDOW.with(|w| {
            if let Some(window) = w.borrow().as_ref() {
                window.orderOut(None);
            }
        });
    }

    if let Some(message) = error {
        alert(mtm, "配置校验失败", &message);
    } else if let Some(message) = save_error {
        alert(mtm, "保存失败", &message);
    }
}

fn alert(mtm: MainThreadMarker, title: &str, body: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(body));
    alert.runModal();
}

pub fn open(cfg: Config, paths: Paths, tx: Sender<UiCommand>) {
    open_form(cfg, paths, tx);
}
