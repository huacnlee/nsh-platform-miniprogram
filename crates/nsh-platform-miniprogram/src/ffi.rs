//! The WebAssembly module's interface with `runtime/nsh-mp.js`, the mini
//! program's side. Strings cross as a pointer and a length into this
//! module's memory (UTF-8); JS copies them before a call returns.
//!
//! Exports: `nsh_mp_alloc` / `nsh_mp_free` (memory for strings JS passes),
//! `nsh_mp_init` (the runner's [`crate::app!`]), `nsh_mp_start`,
//! `nsh_mp_pump`, `nsh_mp_frame`, `nsh_mp_event`, `nsh_mp_scroll`,
//! `nsh_mp_device`. After each, what the engine made arrives through the
//! imports: `submit` per commit, `request_frame`, `schedule` (the next
//! pump, in ms; -1 for none) and `report_error`.
//!
//! Imports, module `nsh_mp`: those, `measure_text` and `log`. NSH's core
//! asks for two more, module `nsh`: `now_ms` (a monotonic clock in ms) and
//! `date_now_ms` (`Date.now()`) (NSH's `docs/dev/platforms.md`).

use std::cell::RefCell;

use nsh_core::element::{Registry, Root};

use crate::{Device, Pumped, Runtime, text::Measure};

#[link(wasm_import_module = "nsh_mp")]
unsafe extern "C" {
    #[link_name = "submit"]
    fn js_submit(json: *const u8, len: usize);
    #[link_name = "request_frame"]
    fn js_request_frame();
    #[link_name = "schedule"]
    fn js_schedule(delay_ms: f64);
    #[link_name = "report_error"]
    fn js_report_error(message: *const u8, len: usize);
    #[link_name = "measure_text"]
    fn js_measure_text(font: *const u8, font_len: usize, text: *const u8, text_len: usize) -> f64;
    #[link_name = "log"]
    fn js_log(level: u32, message: *const u8, len: usize);
}

/// Measures with the logic layer's canvas (`measure_text`).
struct HostMeasure;

impl Measure for HostMeasure {
    fn width(&self, font: &str, text: &str) -> f32 {
        // SAFETY: both ranges are live strings JS only reads during the call.
        unsafe { js_measure_text(font.as_ptr(), font.len(), text.as_ptr(), text.len()) as f32 }
    }
}

type MakeApp = Box<dyn Fn() -> (Vec<Registry>, Root)>;

thread_local! {
    static APP: RefCell<Option<MakeApp>> = RefCell::new(None);
    static RUNTIME: RefCell<Option<Runtime<HostMeasure>>> = const { RefCell::new(None) };
}

/// The OS `nsh-mp.js` passes: 0 iOS, 1 Android, 2 macOS, 3 Windows,
/// 4 Linux (`osCode` there).
fn os_of(code: u32) -> nsh_core::platform::Os {
    use nsh_core::platform::Os;
    match code {
        1 => Os::Android,
        2 => Os::MacOs,
        3 => Os::Windows,
        4 => Os::Linux,
        _ => Os::Ios,
    }
}

/// Sets the app [`nsh_mp_start`] starts ([`crate::app!`]).
pub fn set_app(make: impl Fn() -> (Vec<Registry>, Root) + 'static) {
    APP.with(|a| *a.borrow_mut() = Some(Box::new(make)));
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = format!("[{}] {}", record.target(), record.args());
        // SAFETY: a live string JS only reads during the call.
        unsafe { js_log(record.level() as u32, message.as_ptr(), message.len()) };
    }

    fn flush(&self) {}
}

static LOGGER: Logger = Logger;

fn report_error(message: &str) {
    // SAFETY: a live string JS only reads during the call.
    unsafe { js_report_error(message.as_ptr(), message.len()) };
}

/// Hands what a pump made to JS.
fn deliver(pumped: Pumped) {
    for batch in pumped.batches {
        let json = batch.to_string();
        // SAFETY: a live string JS only reads during the call.
        unsafe { js_submit(json.as_ptr(), json.len()) };
    }
    for error in pumped.errors {
        report_error(&error);
    }
    if pumped.frame {
        // SAFETY: a host function without arguments.
        unsafe { js_request_frame() };
    }
    // SAFETY: a host function taking a number.
    unsafe { js_schedule(pumped.next.unwrap_or(-1.0)) };
}

fn with_runtime(f: impl FnOnce(&mut Runtime<HostMeasure>) -> Pumped) {
    let pumped = RUNTIME.with(|r| r.borrow_mut().as_mut().map(f));
    if let Some(pumped) = pumped {
        deliver(pumped);
    }
}

/// `len` bytes of memory for a string JS passes in.
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Frees memory from [`nsh_mp_alloc`].
///
/// # Safety
///
/// `ptr` came from `nsh_mp_alloc(len)` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nsh_mp_free(ptr: *mut u8, len: usize) {
    // SAFETY: guaranteed by the caller.
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

/// Starts the app on a page `width` × `height` px; 1 if it started.
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_start(
    width: f32,
    height: f32,
    pixel_ratio: f32,
    dark: u32,
    os: u32,
) -> u32 {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Info);
        std::panic::set_hook(Box::new(|info| report_error(&format!("nsh panicked: {info}"))));
    });
    let Some((sets, root)) = APP.with(|a| a.borrow().as_ref().map(|make| make())) else {
        report_error("this module has no app: its runner calls `nsh_platform_miniprogram::app!`");
        return 0;
    };
    let device = Device {
        width,
        height,
        pixel_ratio,
        dark: dark != 0,
        os: os_of(os),
    };
    let Some(runtime) = Runtime::start(root, sets, device, HostMeasure) else {
        return 0;
    };
    RUNTIME.with(|r| *r.borrow_mut() = Some(runtime));
    with_runtime(Runtime::pump);
    1
}

/// Runs what is due (at the time `schedule` asked for, or after input JS
/// handled itself).
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_pump() {
    with_runtime(Runtime::pump);
}

/// A display frame (after `request_frame`).
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_frame() {
    with_runtime(Runtime::frame);
}

/// An event on view `id`, named by its prop (`onPress`).
///
/// # Safety
///
/// `name..name + len` is memory from [`nsh_mp_alloc`] holding UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn nsh_mp_event(id: u32, name: *const u8, len: usize) {
    // SAFETY: guaranteed by the caller.
    let bytes = unsafe { std::slice::from_raw_parts(name, len) };
    let name = String::from_utf8_lossy(bytes).into_owned();
    with_runtime(|r| r.event(id, &name));
}

/// Scroll view `id` scrolled to `(x, y)`.
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_scroll(id: u32, x: f32, y: f32) {
    with_runtime(|r| r.scroll(id, x, y));
}

/// The page's size or appearance changed.
#[unsafe(no_mangle)]
pub extern "C" fn nsh_mp_device(width: f32, height: f32, pixel_ratio: f32, dark: u32, os: u32) {
    let device = Device {
        width,
        height,
        pixel_ratio,
        dark: dark != 0,
        os: os_of(os),
    };
    with_runtime(|r| r.set_device(device));
}
