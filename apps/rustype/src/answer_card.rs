//! Typeless-style answer panel: a light card with the question, the answer,
//! copy buttons and a close button.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType,
    NSBezierPath, NSBorderType, NSButton, NSColor, NSFont, NSPanel, NSScrollView, NSTextField,
    NSTextView, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const WIDTH: f64 = 720.0;
const HEIGHT: f64 = 640.0;

pub struct AnswerCard {
    panel: Retained<NSPanel>,
    question_label: Retained<NSTextField>,
    answer_view: Retained<NSTextView>,
    _controller: Retained<CardController>,
}

thread_local! {
    static QUESTION: RefCell<String> = const { RefCell::new(String::new()) };
    static ANSWER: RefCell<String> = const { RefCell::new(String::new()) };
    static PANEL: RefCell<Option<Retained<NSPanel>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeAnswerView"]
    struct CardView;

    impl CardView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let shape = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.bounds(),
                16.0,
                16.0,
            );
            NSColor::windowBackgroundColor().setFill();
            shape.fill();
            NSColor::separatorColor().setStroke();
            shape.setLineWidth(1.0);
            shape.stroke();

            // Two hairlines: under the header and under the question.
            for y in [586.0, 526.0] {
                NSColor::separatorColor().setFill();
                NSRect::new(NSPoint::new(0.0, y), NSSize::new(WIDTH, 1.0));
                let line = NSBezierPath::bezierPathWithRect(NSRect::new(
                    NSPoint::new(0.0, y),
                    NSSize::new(WIDTH, 1.0),
                ));
                line.fill();
            }
        }
    }
);

define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeAnswerController"]
    struct CardController;

    impl CardController {
        #[unsafe(method(copyQuestion:))]
        fn copy_question(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(|| copy_text(QUESTION.with(|q| q.borrow().clone())));
        }

        #[unsafe(method(copyAnswer:))]
        fn copy_answer(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(|| copy_text(ANSWER.with(|a| a.borrow().clone())));
        }

        #[unsafe(method(closeCard:))]
        fn close_card(&self, _sender: &AnyObject) {
            let _ = std::panic::catch_unwind(|| {
                PANEL.with(|p| {
                    if let Some(panel) = p.borrow().as_ref() {
                        panel.orderOut(None);
                    }
                });
            });
        }
    }
);

fn copy_text(text: String) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text);
    }
}

fn any_ref<T: objc2::Message + 'static>(obj: &T) -> &AnyObject {
    unsafe { &*(obj as *const T as *const AnyObject) }
}

/// Strip the most common Markdown markers while keeping line structure and
/// lists readable in a plain NSTextView.
fn tidy_markdown(input: &str) -> String {
    let mut out = String::new();
    for raw in input.lines() {
        let mut line = raw.trim_end().to_string();
        for prefix in ["### ", "## ", "# "] {
            if let Some(rest) = line.strip_prefix(prefix) {
                line = rest.to_string();
            }
        }
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            line = format!("• {}", rest);
        }
        line = line.replace("**", "").replace('`', "");
        out.push_str(&line);
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn label(text: &str, font: Retained<NSFont>, color: Retained<NSColor>, frame: NSRect, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&font));
    label.setTextColor(Some(&color));
    label.setFrame(frame);
    label
}

impl AnswerCard {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("answer card must be created on the main thread");
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, HEIGHT)),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setLevel(3);
        panel.setHasShadow(true);
        panel.setIgnoresMouseEvents(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setHidesOnDeactivate(false);
        // Follow the system appearance: the panel uses dynamic colors
        // (windowBackgroundColor / labelColor).

        let content = CardView::alloc(mtm);
        let content: Retained<CardView> = unsafe { objc2::msg_send![content, init] };
        content.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WIDTH, HEIGHT),
        ));
        panel.setContentView(Some(&content));

        let controller = CardController::alloc(mtm);
        let controller: Retained<CardController> = unsafe { objc2::msg_send![controller, init] };
        let target = any_ref(&*controller);

        // Header.
        let app_icon = label(
            "🎙",
            NSFont::systemFontOfSize(18.0),
            NSColor::labelColor(),
            NSRect::new(NSPoint::new(18.0, 598.0), NSSize::new(28.0, 24.0)),
            mtm,
        );
        content.addSubview(&app_icon);
        let title = label(
            "Rustype",
            NSFont::boldSystemFontOfSize(15.0),
            NSColor::labelColor(),
            NSRect::new(NSPoint::new(50.0, 600.0), NSSize::new(200.0, 20.0)),
            mtm,
        );
        content.addSubview(&title);

        let close = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("✕"),
                Some(target),
                Some(sel!(closeCard:)),
                mtm,
            )
        };
        close.setBordered(false);
        close.setFont(Some(&NSFont::systemFontOfSize(15.0)));
        close.setFrame(NSRect::new(
            NSPoint::new(WIDTH - 44.0, 598.0),
            NSSize::new(28.0, 24.0),
        ));
        content.addSubview(&close);

        // Question.
        let mic = label(
            "🎤",
            NSFont::systemFontOfSize(14.0),
            NSColor::secondaryLabelColor(),
            NSRect::new(NSPoint::new(18.0, 548.0), NSSize::new(22.0, 20.0)),
            mtm,
        );
        content.addSubview(&mic);

        let question_label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        question_label.setFont(Some(&NSFont::systemFontOfSize(13.0)));
        question_label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        question_label.setFrame(NSRect::new(
            NSPoint::new(46.0, 534.0),
            NSSize::new(560.0, 42.0),
        ));
        content.addSubview(&question_label);

        let copy_question = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("复制"),
                Some(target),
                Some(sel!(copyQuestion:)),
                mtm,
            )
        };
        copy_question.setBordered(false);
        copy_question.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        copy_question.setFrame(NSRect::new(
            NSPoint::new(WIDTH - 74.0, 546.0),
            NSSize::new(56.0, 22.0),
        ));
        content.addSubview(&copy_question);

        // Answer header.
        let answer_label = label(
            "✎ 回答",
            NSFont::boldSystemFontOfSize(14.0),
            NSColor::labelColor(),
            NSRect::new(NSPoint::new(18.0, 492.0), NSSize::new(120.0, 22.0)),
            mtm,
        );
        content.addSubview(&answer_label);

        let copy_answer = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("复制"),
                Some(target),
                Some(sel!(copyAnswer:)),
                mtm,
            )
        };
        copy_answer.setBordered(false);
        copy_answer.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        copy_answer.setFrame(NSRect::new(
            NSPoint::new(WIDTH - 74.0, 490.0),
            NSSize::new(56.0, 22.0),
        ));
        content.addSubview(&copy_answer);

        // Scrollable answer.
        let answer_view = NSTextView::new(mtm);
        answer_view.setEditable(false);
        answer_view.setSelectable(true);
        answer_view.setDrawsBackground(false);
        answer_view.setTextContainerInset(NSSize::new(12.0, 12.0));
        answer_view.setFont(Some(&NSFont::systemFontOfSize(13.5)));
        answer_view.setTextColor(Some(&NSColor::labelColor()));
        answer_view.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(WIDTH - 48.0, 1400.0),
        ));

        let scroll = NSScrollView::new(mtm);
        scroll.setFrame(NSRect::new(
            NSPoint::new(16.0, 16.0),
            NSSize::new(WIDTH - 32.0, 462.0),
        ));
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setDrawsBackground(false);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&answer_view));
        content.addSubview(&scroll);

        if let Some(screen) = objc2_app_kit::NSScreen::mainScreen(mtm) {
            let frame = screen.visibleFrame();
            let x = frame.origin.x + (frame.size.width - WIDTH) / 2.0;
            let y = frame.origin.y + (frame.size.height - HEIGHT) / 2.0 + 20.0;
            panel.setFrameOrigin(NSPoint::new(x, y));
        }

        PANEL.with(|p| *p.borrow_mut() = Some(panel.clone()));

        AnswerCard {
            panel,
            question_label,
            answer_view,
            _controller: controller,
        }
    }

    pub fn show(&self, question: &str, answer: &str) {
        QUESTION.with(|q| *q.borrow_mut() = question.to_string());
        let tidy = tidy_markdown(answer);
        ANSWER.with(|a| *a.borrow_mut() = tidy.clone());
        self.question_label
            .setStringValue(&NSString::from_str(question));
        self.answer_view.setString(&NSString::from_str(&tidy));
        self.answer_view
            .setFont(Some(&NSFont::systemFontOfSize(13.5)));
        self.answer_view.setTextColor(Some(&NSColor::labelColor()));
        self.panel.orderFrontRegardless();
    }
}
