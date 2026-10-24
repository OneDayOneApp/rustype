//! Floating recording pill: a non-activating panel with a live waveform and
//! cancel / confirm buttons, modelled on Typeless's floating bar.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::Arc;

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezierPath, NSColor, NSEvent, NSFont, NSGraphicsContext, NSLineCapStyle,
    NSPanel, NSScreen, NSTextAlignment, NSTextField, NSView, NSWindowCollectionBehavior,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSDefaultRunLoopMode, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSTimer,
};

use crate::session::UiCommand;

pub struct Pill {
    panel: Retained<NSPanel>,
    mode: Arc<AtomicU8>,
    label: Retained<NSTextField>,
}

const MODE_RECORDING: u8 = 0;
const MODE_THINKING: u8 = 1;

struct PillIvars {
    level: Arc<AtomicU32>,
    cmd_tx: Sender<UiCommand>,
    mode: Arc<AtomicU8>,
    ticks: AtomicU32,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RustypePillView"]
    #[ivars = PillIvars]
    struct PillView;

    impl PillView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            if self.ivars().mode.load(Ordering::Relaxed) == MODE_THINKING {
                draw_thinking(self);
            } else {
                draw_recording(self);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if self.ivars().mode.load(Ordering::Relaxed) == MODE_THINKING {
                return;
            }
            let point = event.locationInWindow();
            let width = self.bounds().size.width;
            let cmd = if point.x < 52.0 {
                UiCommand::Cancel
            } else if point.x > width - 52.0 {
                UiCommand::Stop
            } else {
                return;
            };
            let _ = self.ivars().cmd_tx.send(cmd);
        }

        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            self.ivars().ticks.fetch_add(1, Ordering::Relaxed);
            self.setNeedsDisplay(true);
        }
    }
);

const HEIGHT: f64 = 48.0;
const WIDTH: f64 = 224.0;

fn draw_recording(view: &PillView) {
    let bounds = view.bounds();
    let level = view.ivars().level.load(Ordering::Relaxed) as f64 / 1000.0;

    // Pill background.
    let pill =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, HEIGHT / 2.0, HEIGHT / 2.0);
    NSColor::colorWithRed_green_blue_alpha(0.09, 0.09, 0.10, 0.96).setFill();
    pill.fill();

    // Waveform bars in the centre.
    let bars = 9usize;
    let bar_w = 4.0;
    let gap = 5.0;
    let total = bars as f64 * bar_w + (bars - 1) as f64 * gap;
    let start_x = (WIDTH - total) / 2.0;
    let centre_y = HEIGHT / 2.0;
    NSColor::colorWithRed_green_blue_alpha(1.0, 1.0, 1.0, 0.92).setFill();
    for i in 0..bars {
        let phase = (i as f64 * 0.9).sin().abs();
        let h = 5.0 + 26.0 * (level * (0.45 + 0.55 * phase)).clamp(0.0, 1.0);
        let rect = NSRect::new(
            NSPoint::new(start_x + i as f64 * (bar_w + gap), centre_y - h / 2.0),
            NSSize::new(bar_w, h.max(4.0)),
        );
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, 2.0, 2.0);
        path.fill();
    }

    // Cancel (X) button.
    NSColor::colorWithRed_green_blue_alpha(1.0, 1.0, 1.0, 0.14).setFill();
    let circle = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(10.0, (HEIGHT - 28.0) / 2.0),
        NSSize::new(28.0, 28.0),
    ));
    circle.fill();

    NSColor::colorWithRed_green_blue_alpha(1.0, 1.0, 1.0, 0.85).setStroke();
    let x = NSBezierPath::bezierPath();
    x.moveToPoint(NSPoint::new(19.0, HEIGHT / 2.0 - 4.5));
    x.lineToPoint(NSPoint::new(28.0, HEIGHT / 2.0 + 4.5));
    x.moveToPoint(NSPoint::new(28.0, HEIGHT / 2.0 - 4.5));
    x.lineToPoint(NSPoint::new(19.0, HEIGHT / 2.0 + 4.5));
    x.setLineWidth(1.8);
    x.setLineCapStyle(NSLineCapStyle::Round);
    x.stroke();

    // Confirm (check) button.
    let accent = NSColor::colorWithRed_green_blue_alpha(1.0, 1.0, 1.0, 0.95);
    accent.setFill();
    let circle = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(
        NSPoint::new(WIDTH - 38.0, (HEIGHT - 28.0) / 2.0),
        NSSize::new(28.0, 28.0),
    ));
    circle.fill();

    NSColor::colorWithRed_green_blue_alpha(0.09, 0.09, 0.10, 1.0).setStroke();
    let check = NSBezierPath::bezierPath();
    check.moveToPoint(NSPoint::new(WIDTH - 31.0, HEIGHT / 2.0));
    check.lineToPoint(NSPoint::new(WIDTH - 25.0, HEIGHT / 2.0 - 5.0));
    check.lineToPoint(NSPoint::new(WIDTH - 17.0, HEIGHT / 2.0 + 5.5));
    check.setLineWidth(2.2);
    check.setLineCapStyle(NSLineCapStyle::Round);
    check.stroke();
}

fn draw_thinking(view: &PillView) {
    let bounds = view.bounds();
    let pill =
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, HEIGHT / 2.0, HEIGHT / 2.0);
    NSColor::colorWithRed_green_blue_alpha(0.09, 0.09, 0.10, 0.97).setFill();
    pill.fill();

    // A soft highlight sweeps across the pill, like a progress shimmer.
    let ticks = view.ivars().ticks.load(Ordering::Relaxed) as f64;
    let period = 80.0;
    let phase = (ticks % period) / period;
    let band_w = 90.0;
    let x = -band_w + phase * (WIDTH + band_w * 2.0);
    if let Some(ctx) = NSGraphicsContext::currentContext() {
        ctx.saveGraphicsState();
        pill.addClip();
        NSColor::colorWithRed_green_blue_alpha(1.0, 1.0, 1.0, 0.08).setFill();
        let band = NSRect::new(NSPoint::new(x, 0.0), NSSize::new(band_w, HEIGHT));
        NSBezierPath::bezierPathWithRect(band).fill();
        ctx.restoreGraphicsState();
    }
}

impl Pill {
    pub fn new(cmd_tx: Sender<UiCommand>, level: Arc<AtomicU32>) -> Self {
        let mtm = MainThreadMarker::new().expect("pill must be created on the main thread");
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
        panel.setLevel(3); // NSFloatingWindowLevel
        panel.setHasShadow(true);
        panel.setIgnoresMouseEvents(false);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setHidesOnDeactivate(false);

        let mode = Arc::new(AtomicU8::new(MODE_RECORDING));
        let view = PillView::alloc(mtm).set_ivars(PillIvars {
            level,
            cmd_tx,
            mode: mode.clone(),
            ticks: AtomicU32::new(0),
        });
        let view: Retained<PillView> = unsafe { msg_send![super(view), init] };
        panel.setContentView(Some(&view));

        let label = NSTextField::labelWithString(&NSString::from_str("Thinking"), mtm);
        label.setFont(Some(&NSFont::systemFontOfSize_weight(15.0, 0.3)));
        label.setTextColor(Some(&NSColor::colorWithRed_green_blue_alpha(
            1.0, 1.0, 1.0, 0.92,
        )));
        label.setAlignment(NSTextAlignment::Center);
        label.setFrame(NSRect::new(
            NSPoint::new(0.0, (HEIGHT - 20.0) / 2.0),
            NSSize::new(WIDTH, 20.0),
        ));
        label.setHidden(true);
        view.addSubview(&label);

        // 30 fps redraw timer on the main run loop.
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                1.0 / 30.0,
                &view,
                sel!(tick:),
                None,
                true,
            )
        };
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSDefaultRunLoopMode) };

        // Bottom centre of the active screen.
        if let Some(screen) = NSScreen::mainScreen(mtm) {
            let frame = screen.visibleFrame();
            let x = frame.origin.x + (frame.size.width - WIDTH) / 2.0;
            let y = frame.origin.y + 90.0;
            panel.setFrameOrigin(NSPoint::new(x, y));
        }

        Pill { panel, mode, label }
    }

    pub fn set_recording(&self) {
        self.mode.store(MODE_RECORDING, Ordering::Relaxed);
        self.label.setHidden(true);
    }

    pub fn set_thinking(&self) {
        self.mode.store(MODE_THINKING, Ordering::Relaxed);
        self.label.setHidden(false);
    }

    pub fn show(&self) {
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}
