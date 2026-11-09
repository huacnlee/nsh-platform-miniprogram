//! A community-maintained NSH platform: Native Shell apps in WeChat mini
//! programs. It is also the worked example of NSH's platform extension
//! points (NSH's `docs/dev/platforms.md`): a backend built only on NSH's
//! public engine API, outside NSH's repository.
//!
//! How it fits together:
//!
//! - The engine ([`nsh_engine::LocalEngine`]: the core, layout, the
//!   reactive graph and the app's Rust components) runs in WebAssembly in
//!   the Mini Program's logic layer, driven by its timers ([`Runtime`]).
//! - Each commit's [`nsh_core::MountBatch`] becomes JSON operations
//!   ([`encode`]) that the runtime `runtime/nsh-mp.js` applies to a tree of
//!   plain objects and hands to the render layer with `setData`; a
//!   recursive component (`runtime/components/nsh-node`) draws each node at
//!   the frame the engine laid out.
//! - Text is measured with the host's fonts and broken into lines here
//!   ([`text`]); the render layer draws the lines where they were placed.
//! - Taps come back as `onPress` / `onPressIn` / `onPressOut` events, and
//!   scroll views report their offsets, through [`EngineControl`].
//!
//! The platform reports `platform.os` as `"miniprogram"`
//! ([`Os::External`]), a phone ([`Idiom::Phone`]).
//!
//! A runner crate (the CLI writes it) starts an app with [`app!`] and links
//! this crate for `wasm32-unknown-unknown`, where [`ffi`] exports the
//! functions `nsh-mp.js` calls.

pub mod encode;
#[cfg(target_arch = "wasm32")]
pub mod ffi;
pub mod text;

use std::sync::{Arc, Mutex};

use nsh_core::{
    EventKind, EventPayload, Points, Size,
    element::{Registry, Root},
    geometry::point,
    platform::{ColorScheme, Idiom, Os},
};
use nsh_core::MountBatch;
use nsh_engine::{App, EngineConfig, EngineControl, Environment, LocalEngine, MountSink};
use serde_json::Value;

use crate::text::{Measure, MiniProgramTextEngine};

/// The name this platform reports in `platform.os` and is selected with
/// (`nsh run --target miniprogram`).
pub const OS_NAME: &str = "miniprogram";

/// What the Mini Program reports about the device at start
/// (`wx.getWindowInfo()`, `wx.getAppBaseInfo()`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Device {
    /// The page's size in px (points).
    pub width: f32,
    /// The page's height.
    pub height: f32,
    /// Device pixels per px.
    pub pixel_ratio: f32,
    /// The system shows dark mode.
    pub dark: bool,
}

impl Default for Device {
    fn default() -> Self {
        Device {
            width: 375.0,
            height: 667.0,
            pixel_ratio: 2.0,
            dark: false,
        }
    }
}

impl Device {
    /// The environment the engine runs the app in.
    pub fn environment(&self) -> Environment {
        Environment {
            window: Size::new(Points(self.width), Points(self.height)),
            color_scheme: match self.dark {
                true => ColorScheme::Dark,
                false => ColorScheme::Light,
            },
            pixel_ratio: self.pixel_ratio,
            os: Os::External(OS_NAME),
            idiom: Idiom::Phone,
            ..Environment::default()
        }
    }
}

/// What the engine hands the Mini Program between two pumps.
#[derive(Debug, Default)]
struct Outbox {
    /// Each commit's operations ([`encode::batch`]), in order.
    batches: Vec<Value>,
    /// The engine asked for a display frame.
    frame: bool,
    /// Errors of the app, as text.
    errors: Vec<String>,
}

/// The engine's [`MountSink`]: encodes each commit into the outbox.
struct Sink(Arc<Mutex<Outbox>>);

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let ops = encode::batch(&batch);
        if ops.as_array().is_some_and(|ops| !ops.is_empty()) {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).batches.push(ops);
        }
    }

    fn request_frame(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).frame = true;
    }

    fn report_error(&self, message: &str) {
        log::error!("{message}");
        let mut outbox = self.0.lock().unwrap_or_else(|e| e.into_inner());
        outbox.errors.push(message.to_owned());
    }
}

/// What a pump leaves for the Mini Program to do.
#[derive(Debug, Default, PartialEq)]
pub struct Pumped {
    /// The operations of the commits it made, one array per commit.
    pub batches: Vec<Value>,
    /// Call [`Runtime::frame`] at the next display frame.
    pub frame: bool,
    /// Pump again in this many milliseconds (`None`: only on input).
    pub next: Option<f64>,
    /// Errors of the app to show.
    pub errors: Vec<String>,
}

/// One running app: the engine, its input and what it committed.
pub struct Runtime<M: Measure + 'static> {
    engine: LocalEngine<MiniProgramTextEngine<M>>,
    control: EngineControl,
    outbox: Arc<Mutex<Outbox>>,
}

impl<M: Measure + 'static> Runtime<M> {
    /// Starts `root` with `ui_sets` (`nsh/base` and the app's) on `device`,
    /// measuring text with `measure`; `None` if the app could not start
    /// (the reason is in the log).
    pub fn start(
        root: Root,
        ui_sets: Vec<Registry>,
        device: Device,
        measure: M,
    ) -> Option<Self> {
        let outbox = Arc::new(Mutex::new(Outbox::default()));
        let config = EngineConfig {
            viewport: Size::new(Points(device.width), Points(device.height)),
            environment: device.environment(),
            // NSH draws no header: the Mini Program's navigation bar is the
            // system's, and a page's `header` slot sits at its top.
            header_heights: nsh_core::NO_NATIVE_HEADER,
            ui_sets,
            ..EngineConfig::new(App::Rust(root))
        };
        let engine = LocalEngine::start(
            config,
            MiniProgramTextEngine::new(measure),
            Sink(outbox.clone()),
        )?;
        let control = engine.control();
        Some(Runtime {
            engine,
            control,
            outbox,
        })
    }

    /// Runs what is due and returns what the commits made.
    pub fn pump(&mut self) -> Pumped {
        let deadline = self.engine.pump();
        let mut outbox = self.outbox.lock().unwrap_or_else(|e| e.into_inner());
        Pumped {
            batches: std::mem::take(&mut outbox.batches),
            frame: std::mem::take(&mut outbox.frame),
            next: deadline.map(|at| {
                at.saturating_duration_since(nsh_core::time::Instant::now())
                    .as_secs_f64()
                    * 1000.0
            }),
            errors: std::mem::take(&mut outbox.errors),
        }
    }

    /// A display frame (asked for with [`Pumped::frame`]).
    pub fn frame(&mut self) -> Pumped {
        self.control.frame();
        self.pump()
    }

    /// The user's finger on view `id`: `kind` is `onPress`, `onPressIn`,
    /// `onPressOut` or `onLongPress` ([`EventKind::from_prop`]).
    pub fn event(&mut self, id: u32, kind: &str) -> Pumped {
        if let Some(kind) = EventKind::from_prop(kind) {
            self.control
                .dispatch_event(id, kind, EventPayload::None);
        } else {
            log::warn!("unknown event {kind}");
        }
        self.pump()
    }

    /// Scroll view `id` is at `(x, y)`.
    pub fn scroll(&mut self, id: u32, x: f32, y: f32) -> Pumped {
        self.control
            .scroll(id, point(Points(x), Points(y)), 0);
        self.pump()
    }

    /// The page changed size or appearance.
    pub fn set_device(&mut self, device: Device) -> Pumped {
        self.control
            .resize(Size::new(Points(device.width), Points(device.height)));
        self.control.set_environment(device.environment());
        self.pump()
    }
}

/// Makes the runner a Mini Program's runtime of a Rust app: `$sets` are
/// its UI sets (`nsh/base` and the app's, a `Vec<Registry>`) and `$root`
/// its root (a [`Root`]), as NSH's generated crate `nsh_rust_app` gives
/// them:
///
/// ```ignore
/// nsh_platform_miniprogram::app!(nsh_rust_app::ui_sets(), nsh_rust_app::root());
/// ```
#[macro_export]
macro_rules! app {
    ($sets:expr, $root:expr) => {
        /// Registers the app (`nsh-mp.js` calls it before starting).
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn nsh_mp_init() {
            $crate::ffi::set_app(|| ($sets, $root));
        }
    };
}

#[cfg(test)]
mod tests {
    use nsh_core::prelude::*;

    use super::*;
    use crate::text::ApproxMeasure;

    #[component]
    fn Counter() -> impl IntoElement {
        let count = signal(0);
        view()
            .test_id("button")
            .bg(rgb(0x2563eb))
            .on_press(move || count.update(|n| *n += 1))
            .child(text(move || format!("Count {}", count.get())))
    }

    /// The text ops' lines, in order.
    fn texts(pumped: &Pumped) -> Vec<String> {
        pumped
            .batches
            .iter()
            .flat_map(|b| b.as_array().unwrap().clone())
            .filter(|op| op[0] == "t")
            .flat_map(|op| {
                op[2]["lines"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| l[0].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    #[test]
    fn an_app_renders_and_answers_presses() {
        let mut runtime = Runtime::start(
            Root::of::<Counter>(),
            Vec::new(),
            Device::default(),
            ApproxMeasure,
        )
        .unwrap();
        let first = runtime.pump();
        assert_eq!(texts(&first), ["Count 0"]);
        let ops: Vec<Value> = first
            .batches
            .iter()
            .flat_map(|b| b.as_array().unwrap().clone())
            .collect();
        // The button reports presses, and has its background.
        let button = ops
            .iter()
            .find(|op| op[0] == "e" && op[2].as_u64().unwrap() & 1 == 1)
            .map(|op| op[1].as_u64().unwrap() as u32)
            .expect("a view reporting presses");
        assert!(
            ops.iter()
                .any(|op| op[0] == "s" && op[1] == button && op[2] == "background-color:rgb(37,99,235);")
        );
        let pressed = runtime.event(button, "onPress");
        assert_eq!(texts(&pressed), ["Count 1"]);
    }

    #[test]
    fn the_platform_reports_its_own_name() {
        let environment = Device::default().environment();
        assert_eq!(environment.os.name(), "miniprogram");
        assert!(!environment.desktop());
    }
}
