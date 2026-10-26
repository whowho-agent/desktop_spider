use glam::Vec2;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSColor, NSEvent, NSScreen, NSScreenSaverWindowLevel, NSView, NSWindowCollectionBehavior,
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

// --- Плеть (реализует агент платформы, W0 + W2c; сейчас заглушки) ---

/// Удерживается ли хоткей плети прямо сейчас. Опрос, без событий; вызывается каждый кадр.
pub fn hotkey_held(_hotkey: &crate::settings::Hotkey) -> bool {
    false
}

/// Режим плети: `on` — оверлей принимает клики (ЛКМ/ПКМ не доходят до приложений под ним),
/// системный курсор скрыт, фокус активного приложения не отнимается (или возвращается при `false`).
/// `off` — всё как было: клики насквозь, курсор виден.
pub fn set_whip_capture(_window: &Window, _on: bool) {}
