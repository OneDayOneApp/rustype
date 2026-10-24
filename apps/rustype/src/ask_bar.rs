//! "Ask anything" hint shown above the recording pill in command mode.

use objc2::rc::Retained;
use objc2::{define_class, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameDarkAqua, NSBackingStoreType,
    NSBezierPath, NSColor, NSFont, NSGraphicsContext, NSPanel, NSShadow, NSTextAlignment,
    NSTextField, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

const WIDTH: f64 = 240.0;
const HEIGHT: f64 = 44.0;

pub struct AskBar {
    panel: Retained<NSPanel>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypeAskBarView"]
    struct AskBarView;

    impl AskBarView {
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
            let shadow = NSShadow::new();
            shadow.setShadowColor(Some(&NSColor::colorWithRed_green_blue_alpha(
                0.20, 0.85, 0.55, 0.45,
            )));
            shadow.setShadowBlurRadius(10.0);
            shadow.setShadowOffset(NSSize::new(0.0, 0.0));
            shadow.set();
            NSColor::colorWithRed_green_blue_alpha(0.07, 0.12, 0.10, 0.97).setFill();
            shape.fill();
            NSColor::colorWithRed_green_blue_alpha(0.30, 0.85, 0.55, 0.35).setStroke();
            shape.setLineWidth(1.0);
            shape.stroke();
            if let Some(ctx) = &ctx {
                ctx.restoreGraphicsState();
            }
        }
    }
);

impl AskBar {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("ask bar must be created on the main thread");
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
        panel.setIgnoresMouseEvents(true);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setHidesOnDeactivate(false);
        if let Some(dark) = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }) {
            panel.setAppearance(Some(&dark));
        }

        let view = AskBarView::alloc(mtm);
        let view: Retained<AskBarView> = unsafe { objc2::msg_send![view, init] };
        view.setFrame(rect);
        panel.setContentView(Some(&view));

        let label = NSTextField::labelWithString(&NSString::from_str("询问任何问题"), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize(14.0)));
        label.setTextColor(Some(&NSColor::colorWithRed_green_blue_alpha(
            1.0, 1.0, 1.0, 0.95,
        )));
        label.setAlignment(NSTextAlignment::Center);
        label.setFrame(NSRect::new(
            NSPoint::new(0.0, (HEIGHT - 20.0) / 2.0),
            NSSize::new(WIDTH, 20.0),
        ));
        view.addSubview(&label);

        if let Some(screen) = objc2_app_kit::NSScreen::mainScreen(mtm) {
            let frame = screen.visibleFrame();
            let x = frame.origin.x + (frame.size.width - WIDTH) / 2.0;
            let y = frame.origin.y + 90.0 + 48.0 + 10.0;
            panel.setFrameOrigin(NSPoint::new(x, y));
        }

        AskBar { panel }
    }

    pub fn show(&self) {
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}
