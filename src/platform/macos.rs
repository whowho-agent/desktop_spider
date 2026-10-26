use std::cell::RefCell;

use glam::Vec2;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSColor, NSCursor, NSEvent,
    NSRunningApplication, NSScreen, NSScreenSaverWindowLevel, NSView, NSWindowCollectionBehavior,
    NSWorkspace,
};
use objc2_foundation::NSRect;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::InnerSizeWriter;
use winit::event_loop::EventLoopBuilder;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes};

/// Frame главного экрана (screens[0]) — тот, где меню-бар; начало координат Cocoa.
fn primary_frame(mtm: MainThreadMarker) -> NSRect {
    NSScreen::screens(mtm)
        .firstObject()
        .map(|s| s.frame())
        .unwrap_or_default()
}

pub fn configure_event_loop<T: 'static>(builder: &mut EventLoopBuilder<T>) {
    builder.with_activation_policy(ActivationPolicy::Accessory); // без иконки в Dock
}

pub fn overlay_attributes() -> WindowAttributes {
    let mtm = MainThreadMarker::new().expect("overlay_attributes: только с главного потока");
    let s = primary_frame(mtm).size;
    Window::default_attributes()
        .with_title("Desktop Spider")
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_has_shadow(false)
        .with_position(LogicalPosition::new(0.0, 0.0))
        .with_inner_size(LogicalSize::new(s.width, s.height))
}

pub fn configure_overlay(window: &Window) {
    let mtm = MainThreadMarker::new().expect("configure_overlay: только с главного потока");
    window
        .set_cursor_hittest(false)
        .expect("set_cursor_hittest не поддерживается");

    let handle = window.window_handle().expect("нет window handle");
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        panic!("ожидался AppKit window handle");
    };
    // SAFETY: winit гарантирует, что ns_view — живой NSView, пока жив `window`;
    // мы на главном потоке и не храним ссылку дольше вызова.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let ns_window = view.window().expect("NSView не прикреплён к окну");

    // Поверх меню-бара, Dock и fullscreen-приложений, на всех Spaces.
    ns_window.setLevel(NSScreenSaverWindowLevel);
    ns_window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    ns_window.setOpaque(false);
    ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
    ns_window.setHasShadow(false);
    ns_window.setIgnoresMouseEvents(true);
    // Полный frame главного экрана, включая меню-бар (не visibleFrame).
    ns_window.setFrame_display(primary_frame(mtm), true);
}

/// Окно совпадает с главным экраном, так что координаты экрана = координаты окна.
pub fn cursor_position(_window: &Window) -> Vec2 {
    let mtm = MainThreadMarker::new().expect("cursor_position: только с главного потока");
    // Cocoa: точки, начало — левый нижний угол главного экрана, Y вверх.
    let p = NSEvent::mouseLocation();
    let h = primary_frame(mtm).size.height;
    Vec2::new(p.x as f32, (h - p.y) as f32)
}

/// На macOS размер окна в точках не меняется со сменой scale factor — winit всё делает сам.
pub fn on_scale_factor_changed(_window: &Window, _writer: &mut InnerSizeWriter) {}

// --- Плеть ---
//
// W0 (Darwin 25): `CGEventSourceFlagsState` и `CGEventSourceKeyState` работают без
// «Мониторинга ввода» и без Accessibility (проверено отдельным .app без разрешений TCC).

/// FFI к CoreGraphics: опрос состояния клавиатуры (публичный API, без разрешений).
mod cg {
    /// `kCGEventSourceStateCombinedSessionState`.
    pub const COMBINED_SESSION: i32 = 0;
    pub const MASK_SHIFT: u64 = 0x0002_0000;
    pub const MASK_CONTROL: u64 = 0x0004_0000;
    pub const MASK_ALTERNATE: u64 = 0x0008_0000;
    pub const MASK_COMMAND: u64 = 0x0010_0000;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventSourceFlagsState(state: i32) -> u64;
        pub fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }
}

/// Удерживается ли хоткей плети прямо сейчас. Опрос, без событий; вызывается каждый кадр.
/// Модификаторы должны совпасть точно (см. `keys::mods_match`); клавиша — `kVK_*` по имени.
pub fn hotkey_held(hotkey: &crate::settings::Hotkey) -> bool {
    use super::keys;
    // SAFETY: чистые функции опроса без указателей.
    let flags = unsafe { cg::CGEventSourceFlagsState(cg::COMBINED_SESSION) };
    let mods = keys::Mods {
        ctrl: flags & cg::MASK_CONTROL != 0,
        alt: flags & cg::MASK_ALTERNATE != 0,
        shift: flags & cg::MASK_SHIFT != 0,
        cmd: flags & cg::MASK_COMMAND != 0,
    };
    let mut held = keys::mods_match(hotkey, mods);
    if held {
        if let Some(name) = &hotkey.key {
            held = match keys::mac_keycode(name) {
                // SAFETY: см. выше.
                Some(code) => unsafe { cg::CGEventSourceKeyState(cg::COMBINED_SESSION, code) },
                None => {
                    keys::warn_unknown_key(name);
                    false
                }
            };
        }
    }
    keys::report(hotkey, held)
}

#[derive(Default)]
struct Capture {
    on: bool,
    /// Активное приложение на момент включения захвата (не мы).
    saved_front: Option<Retained<NSRunningApplication>>,
}

thread_local! {
    static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default());
}

fn app_label(app: &NSRunningApplication) -> String {
    let name = app.localizedName().map(|n| n.to_string()).unwrap_or_default();
    format!("{name} (pid {})", app.processIdentifier())
}

/// Режим плети: `on` — оверлей принимает клики (ЛКМ/ПКМ не доходят до приложений под ним),
/// системный курсор скрыт; `off` — клики насквозь, курсор виден. Вызывать на переходах.
///
/// Фокус: публичного способа принять клик окном NSWindow без активации приложения нет
/// (non-activating — только у NSPanel; `_setPreventsActivation:` — приватный API).
/// Поэтому первый клик активирует наше Accessory-приложение (winit отвечает
/// `acceptsFirstMouse = YES`, так что этот же клик доходит до нас), а при выключении
/// мы возвращаем активность приложению, запомненному при включении, — только если
/// сейчас активны именно мы (пользователь мог сам переключиться через ⌘Tab).
///
/// Курсор: `NSCursor hide` и невидимый курсор winit (cursor rects) действуют только пока
/// активно наше приложение. До первого клика системный курсор виден — плеть рисуется поверх.
pub fn set_whip_capture(window: &Window, on: bool) {
    use super::input_debug;
    let mtm = MainThreadMarker::new().expect("set_whip_capture: только с главного потока");
    CAPTURE.with_borrow_mut(|c| {
        if c.on == on {
            return;
        }
        c.on = on;
        let me = NSRunningApplication::currentApplication();
        if on {
            let front = NSWorkspace::sharedWorkspace().frontmostApplication();
            c.saved_front = front.filter(|a| a.processIdentifier() != me.processIdentifier());
            input_debug!(
                "захват ВКЛ; активное приложение: {}",
                c.saved_front.as_deref().map(app_label).unwrap_or_else(|| "мы/нет".into())
            );
            let _ = window.set_cursor_hittest(true);
            window.set_cursor_visible(false);
            NSCursor::hide();
        } else {
            // Сначала курсор (пока мы, возможно, ещё активны), потом клики, потом фокус.
            NSCursor::unhide();
            window.set_cursor_visible(true);
            let _ = window.set_cursor_hittest(false);
            let saved = c.saved_front.take();
            let we_are_front = NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .is_some_and(|a| a.processIdentifier() == me.processIdentifier());
            match saved {
                Some(app) if we_are_front && !app.isTerminated() => {
                    let ns_app = NSApplication::sharedApplication(mtm);
                    // macOS 14+: кооперативная активация — сначала уступаем, потом просим.
                    if ns_app.respondsToSelector(objc2::sel!(yieldActivationToApplication:)) {
                        ns_app.yieldActivationToApplication(&app);
                    }
                    let ok = app.activateWithOptions(NSApplicationActivationOptions::empty());
                    input_debug!("захват ВЫКЛ; возвращаю фокус {} → {ok}", app_label(&app));
                }
                Some(app) => input_debug!(
                    "захват ВЫКЛ; фокус не трогаю (активны мы: {we_are_front}, сохранено {})",
                    app_label(&app)
                ),
                None => input_debug!("захват ВЫКЛ; сохранённого приложения нет"),
            }
        }
    });
}
