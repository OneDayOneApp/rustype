//! Translation target selector shown above the recording pill.

use std::cell::RefCell;

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSBackingStoreType,
    NSBezierPath, NSColor, NSFont, NSGraphicsContext, NSPanel, NSPopUpButton, NSShadow,
    NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

use crate::session::UiCommand;

#[rustfmt::skip]
pub const LANGUAGES: &[(&str, &str)] = &[
    ("中文（简体）", "Chinese (Simplified)"),
    ("中文（繁體）", "Chinese (Traditional)"),
    ("英语（美国）", "English (United States)"),
    ("英语（英国）", "English (United Kingdom)"),
    ("日本語", "Japanese"),
    ("한국어", "Korean"),
    ("Français", "French"),
    ("Deutsch", "German"),
    ("Español", "Spanish"),
    ("Português", "Portuguese"),
    ("Русский", "Russian"),
    ("Italiano", "Italian"),
    ("العربية", "Arabic"),
    ("हिन्दी", "Hindi"),
    ("ไทย", "Thai"),
    ("Tiếng Việt", "Vietnamese"),
];

const WIDTH: f64 = 300.0;
const HEIGHT: f64 = 48.0;

pub struct TranslationBar {
    panel: Retained<NSPanel>,
    popup: Retained<NSPopUpButton>,
    _controller: Retained<LangController>,
}

struct BarState {
    cmd_tx: Sender<UiCommand>,
    popup: Retained<NSPopUpButton>,
    values: Vec<String>,
}

thread_local! {
    static BAR: RefCell<Option<BarState>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeLangBarView"]
    struct BarView;

    impl BarView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let shape = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                bounds,
                HEIGHT / 2.0,
                HEIGHT / 2.0,
            );
            let ctx = NSGraphicsContext::currentContext();
            if let Some(ctx) = &ctx {
                ctx.saveGraphicsState();
            }
            // Soft blue glow around the bar.
            let shadow = NSShadow::new();
            let accent = NSColor::controlAccentColor()
                .colorWithAlphaComponent(0.55);
            shadow.setShadowColor(Some(&accent));
            shadow.setShadowBlurRadius(10.0);
            shadow.setShadowOffset(NSSize::new(0.0, 0.0));
            shadow.set();
            NSColor::colorWithRed_green_blue_alpha(0.09, 0.10, 0.13, 0.97).setFill();
            shape.fill();
            NSColor::controlAccentColor()
                .colorWithAlphaComponent(0.35)
                .setStroke();
            shape.setLineWidth(1.0);
            shape.stroke();
            if let Some(ctx) = &ctx {
                ctx.restoreGraphicsState();
            }
        }
    }
);

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeLangController"]
    struct LangController;

    impl LangController {
        #[unsafe(method(changed:))]
        fn changed(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(|| {
                BAR.with(|bar| {
                    if let Some(bar) = bar.borrow().as_ref() {
                        let index = bar.popup.indexOfSelectedItem();
                        if let Some(value) = bar.values.get(index as usize) {
                            let _ = bar
                                .cmd_tx
                                .send(UiCommand::SetTargetLanguage(value.clone()));
                        }
                    }
                });
            });
        }
    }
);

fn any_ref<T: objc2::Message + 'static>(obj: &T) -> &AnyObject {
    unsafe { &*(obj as *const T as *const AnyObject) }
}

/// Built-in languages filtered by the configuration; an empty/unknown
/// configuration falls back to the full list, and the current target is always
/// present so the popup can display it.
fn pick_languages(
    configured: &[String],
    target: &str,
) -> Vec<(String, String)> {
    let all = || -> Vec<(String, String)> {
        LANGUAGES
            .iter()
            .map(|(display, value)| (display.to_string(), value.to_string()))
            .collect()
    };
    let mut selected: Vec<(String, String)> = if configured.is_empty() {
        all()
    } else {
        configured
            .iter()
            .filter_map(|wanted| {
                LANGUAGES
                    .iter()
                    .find(|(_, value)| value.eq_ignore_ascii_case(wanted.trim()))
                    .map(|(display, value)| (display.to_string(), value.to_string()))
            })
            .collect()
    };
    if selected.is_empty() {
        selected = all();
    }
    if !selected.iter().any(|(_, value)| value == target) {
        // Keep custom/legacy targets (e.g. "English" from an older config)
        // selectable instead of silently showing the first language.
        let display = LANGUAGES
            .iter()
            .find(|(_, value)| *value == target)
            .map(|(display, _)| *display)
            .unwrap_or(target);
        selected.push((display.to_string(), target.to_string()));
    }
    selected
}

impl TranslationBar {
    pub fn new(cmd_tx: Sender<UiCommand>) -> Self {
        let mtm =
            MainThreadMarker::new().expect("translation bar must be created on the main thread");
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT));
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            rect,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setLevel(3);
        panel.setHasShadow(false);
        panel.setIgnoresMouseEvents(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setHidesOnDeactivate(false);
        if let Some(dark) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }) {
            panel.setAppearance(Some(&dark));
        }

        let view = BarView::alloc(mtm);
        let view: Retained<BarView> = unsafe { objc2::msg_send![view, init] };
        panel.setContentView(Some(&view));

        let label = NSTextField::labelWithString(&NSString::from_str("翻译为"), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        label.setTextColor(Some(&NSColor::colorWithRed_green_blue_alpha(
            1.0, 1.0, 1.0, 0.92,
        )));
        label.setFrame(NSRect::new(
            NSPoint::new(18.0, (HEIGHT - 20.0) / 2.0),
            NSSize::new(60.0, 20.0),
        ));
        view.addSubview(&label);

        let popup = NSPopUpButton::new(mtm);
        let titles: Vec<Retained<NSString>> = LANGUAGES
            .iter()
            .map(|(display, _)| NSString::from_str(display))
            .collect();
        let refs: Vec<&NSString> = titles.iter().map(|t| &**t).collect();
        popup.addItemsWithTitles(&NSArray::from_slice(&refs));
        popup.setFrame(NSRect::new(
            NSPoint::new(78.0, (HEIGHT - 28.0) / 2.0),
            NSSize::new(WIDTH - 96.0, 28.0),
        ));
        view.addSubview(&popup);

        let controller = LangController::alloc(mtm);
        let controller: Retained<LangController> = unsafe { objc2::msg_send![controller, init] };
        unsafe {
            popup.setTarget(Some(any_ref(&*controller)));
            popup.setAction(Some(sel!(changed:)));
        }

        if let Some(screen) = objc2_app_kit::NSScreen::mainScreen(mtm) {
            let frame = screen.visibleFrame();
            let x = frame.origin.x + (frame.size.width - WIDTH) / 2.0;
            let y = frame.origin.y + 90.0 + HEIGHT + 10.0;
            panel.setFrameOrigin(NSPoint::new(x, y));
        }

        BAR.with(|bar| {
            *bar.borrow_mut() = Some(BarState {
                cmd_tx,
                popup: popup.clone(),
                values: LANGUAGES
                    .iter()
                    .map(|(_, value)| value.to_string())
                    .collect(),
            });
        });

        TranslationBar {
            panel,
            popup,
            _controller: controller,
        }
    }

    pub fn show(&self, configured: &[String], target: &str) {
        let selected = pick_languages(configured, target);

        let titles: Vec<Retained<NSString>> = selected
            .iter()
            .map(|(display, _)| NSString::from_str(display))
            .collect();
        let refs: Vec<&NSString> = titles.iter().map(|t| &**t).collect();
        self.popup.removeAllItems();
        self.popup.addItemsWithTitles(&NSArray::from_slice(&refs));
        BAR.with(|bar| {
            if let Some(bar) = bar.borrow_mut().as_mut() {
                bar.values = selected.iter().map(|(_, value)| value.clone()).collect();
            }
        });

        let index = selected
            .iter()
            .position(|(_, value)| value == target)
            .unwrap_or(0);
        self.popup.selectItemAtIndex(index as isize);
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_uses_all_languages() {
        assert_eq!(pick_languages(&[], "English (United States)").len(), LANGUAGES.len());
    }

    #[test]
    fn config_filters_in_order() {
        let configured = vec![
            "Chinese (Simplified)".to_string(),
            "Japanese".to_string(),
        ];
        let picked = pick_languages(&configured, "Japanese");
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].1, "Chinese (Simplified)");
        assert_eq!(picked[1].1, "Japanese");
    }

    #[test]
    fn unknown_config_falls_back_and_keeps_target() {
        let configured = vec!["Klingon".to_string()];
        let picked = pick_languages(&configured, "German");
        assert!(picked.iter().any(|(_, value)| value == "German"));
        assert!(picked.len() > 1);
    }
}
