//! Платформенный слой. Контракт (одинаковый для macOS и Windows):
//! - `configure_event_loop(&mut EventLoopBuilder)`: платформенные настройки цикла
//!   (macOS: без иконки в Dock).
//! - `overlay_attributes() -> WindowAttributes`: атрибуты окна-оверлея (позиция и размер
//!   уже выставлены: macOS — главный экран, Windows — весь виртуальный рабочий стол).
//! - `configure_overlay(&Window)`: после создания окна: прозрачное, click-through,
//!   поверх всего, без фокуса и без панели задач / Dock.
//! - `cursor_position(&Window) -> Vec2`: курсор в логических точках относительно
//!   левого верхнего угла окна-оверлея, ось Y вниз. Это и есть координаты симуляции.
//! - `on_scale_factor_changed(&Window, &mut InnerSizeWriter)`: не дать системе
//!   ужать оверлей при смене DPI.
//! - Windows дополнительно: `Composition` — DirectComposition-визуал для прозрачной
//!   wgpu-поверхности (см. `render::Renderer`).

//! Плеть: `hotkey_held(&Hotkey)`, `set_whip_capture(&Window, on)` — см. комментарии в реализациях.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(any(target_os = "macos", windows)))]
compile_error!("поддерживаются только macOS и Windows");
