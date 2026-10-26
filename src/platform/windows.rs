//! Windows: одно окно на весь виртуальный рабочий стол (все мониторы),
//! поверх всего, клики насквозь, без фокуса, без панели задач и Alt-Tab.
//!
//! Координаты: winit по умолчанию включает per-monitor DPI awareness v2, поэтому
//! `GetSystemMetrics(SM_*VIRTUALSCREEN)` и `GetCursorPos` отдают физические пиксели.
//! Логические точки = физические пиксели / scale factor окна.
//! Оговорка для мониторов с разным DPI: у окна один scale factor (монитора, где
//! большая его часть), поэтому на остальных мониторах паук будет того же размера
//! в пикселях, т.е. визуально крупнее/мельче. Курсор и рендер при этом согласованы.

use std::ffi::c_void;

use glam::Vec2;
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, GetWindowLongPtrW, SetLayeredWindowAttributes,
    SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, HWND_TOPMOST, LWA_ALPHA, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::InnerSizeWriter;
use winit::event_loop::EventLoopBuilder;
use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes, WindowLevel};

/// Прямоугольник виртуального рабочего стола в физических пикселях: (x, y, w, h).
/// x/y могут быть отрицательными (монитор слева/сверху от главного).
fn virtual_screen() -> (i32, i32, i32, i32) {
    // SAFETY: GetSystemMetrics без побочных эффектов.
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    }
}

fn hwnd(window: &Window) -> HWND {
    let handle = window.window_handle().expect("нет window handle");
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        panic!("ожидался Win32 window handle");
    };
    h.hwnd.get() as HWND
}

/// DPI awareness (per-monitor v2) winit включает сам; больше ничего не нужно.
pub fn configure_event_loop<T: 'static>(_builder: &mut EventLoopBuilder<T>) {}

pub fn overlay_attributes() -> WindowAttributes {
    let (x, y, w, h) = virtual_screen();
    Window::default_attributes()
        .with_title("Desktop Spider")
        .with_transparent(true)
        .with_decorations(false)
        .with_resizable(false)
        .with_active(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_skip_taskbar(true)
        .with_undecorated_shadow(false)
        .with_corner_preference(CornerPreference::DoNotRound)
        // WS_EX_NOREDIRECTIONBITMAP: у окна нет GDI-поверхности, на экране только
        // DirectComposition-визуал. Иначе под визуалом видна непрозрачная GDI-поверхность
        // (на практике белый прямоугольник размером с исходное окно winit в левом верхнем углу).
        .with_no_redirection_bitmap(true)
        .with_position(PhysicalPosition::new(x, y))
        .with_inner_size(PhysicalSize::new(w as u32, h as u32))
}

pub fn configure_overlay(window: &Window) {
    // winit ставит WS_EX_TRANSPARENT | WS_EX_LAYERED.
    window
        .set_cursor_hittest(false)
        .expect("set_cursor_hittest не поддерживается");

    let hwnd = hwnd(window);
    let (x, y, w, h) = virtual_screen();
    // SAFETY: hwnd живое окно этого потока (мы на главном потоке, окно создано здесь).
    // Важно: winit пересчитывает GWL_EXSTYLE при смене своих флагов окна
    // (set_visible, set_window_level, ...), после таких вызовов стиль надо ставить заново.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let ex = ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        // Layered-окно без атрибутов не показывается; альфа 255 = попиксельная
        // прозрачность целиком на совести поверхности (DirectComposition).
        SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        SetWindowPos(hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }
}

pub fn cursor_position(window: &Window) -> Vec2 {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: p — валидный out-указатель.
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return Vec2::ZERO;
    }
    let origin = window.inner_position().unwrap_or_else(|_| {
        let (x, y, _, _) = virtual_screen();
        PhysicalPosition::new(x, y)
    });
    let s = window.scale_factor() as f32;
    Vec2::new((p.x - origin.x) as f32 / s, (p.y - origin.y) as f32 / s)
}

/// При WM_DPICHANGED winit предлагает пересчитанный размер — оставляем весь рабочий стол.
pub fn on_scale_factor_changed(_window: &Window, writer: &mut InnerSizeWriter) {
    let (_, _, w, h) = virtual_screen();
    let _ = writer.request_inner_size(PhysicalSize::new(w as u32, h as u32));
}

/// DirectComposition-дерево из одного визуала поверх HWND.
///
/// Swapchain для HWND (DXGI flip model) на DX12 умеет только `Opaque` — оверлей
/// был бы чёрным. Swapchain «для композиции» поддерживает PreMultiplied: wgpu
/// создаёт его сам (`SurfaceTargetUnsafe::CompositionVisual`) и делает
/// `visual.SetContent(swapchain)` в `surface.configure`; после этого нужен `commit()`.
pub struct Composition {
    device: ::windows::Win32::Graphics::DirectComposition::IDCompositionDevice,
    _target: ::windows::Win32::Graphics::DirectComposition::IDCompositionTarget,
    visual: ::windows::Win32::Graphics::DirectComposition::IDCompositionVisual,
}

impl Composition {
    pub fn new(window: &Window) -> Result<Self, String> {
        use ::windows::core::IUnknown;
        use ::windows::Win32::Foundation::{BOOL, HWND as WinHwnd};
        use ::windows::Win32::Graphics::DirectComposition::{
            DCompositionCreateDevice2, IDCompositionDevice,
        };

        let hwnd = WinHwnd(hwnd(window) as *mut c_void);
        // SAFETY: обычные COM-вызовы на главном потоке; hwnd принадлежит процессу.
        unsafe {
            let device: IDCompositionDevice =
                DCompositionCreateDevice2(None::<&IUnknown>).map_err(|e| e.to_string())?;
            // topmost = true: визуал поверх GDI-содержимого окна.
            let target = device
                .CreateTargetForHwnd(hwnd, BOOL::from(true))
                .map_err(|e| e.to_string())?;
            let visual = device.CreateVisual().map_err(|e| e.to_string())?;
            target.SetRoot(&visual).map_err(|e| e.to_string())?;
            Ok(Self { device, _target: target, visual })
        }
    }

    /// Сырой `IDCompositionVisual*` для `wgpu::SurfaceTargetUnsafe::CompositionVisual`
    /// (wgpu сам делает AddRef).
    pub fn visual_ptr(&self) -> *mut c_void {
        ::windows::core::Interface::as_raw(&self.visual)
    }

    /// Применить изменения дерева (после каждого `surface.configure`).
    pub fn commit(&self) {
        // SAFETY: device живой.
        if let Err(e) = unsafe { self.device.Commit() } {
            eprintln!("DirectComposition Commit: {e}");
        }
    }
}

// --- Плеть ---

/// Удерживается ли хоткей плети прямо сейчас. Опрос `GetAsyncKeyState`, разрешений не нужно.
/// Модификаторы должны совпасть точно (см. `keys::mods_match`); `cmd` = левая или правая Win.
pub fn hotkey_held(hotkey: &crate::settings::Hotkey) -> bool {
    use super::keys;
    // Старший бит = клавиша зажата сейчас.
    // SAFETY: GetAsyncKeyState без побочных эффектов.
    let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0;
    let mods = keys::Mods {
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        shift: down(VK_SHIFT),
        cmd: down(VK_LWIN) || down(VK_RWIN),
    };
    let mut held = keys::mods_match(hotkey, mods);
    if held {
        if let Some(name) = &hotkey.key {
            held = match keys::win_vk(name) {
                Some(vk) => down(vk),
                None => {
                    keys::warn_unknown_key(name);
                    false
                }
            };
        }
    }
    keys::report(hotkey, held)
}

thread_local! {
    static CAPTURE_ON: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Режим плети: `on` — оверлей принимает клики (ЛКМ/ПКМ не доходят до приложений под ним),
/// системный курсор над оверлеем скрыт; `off` — клики насквозь, курсор виден.
/// Фокус не уходит вовсе: `WS_EX_NOACTIVATE` — клик по окну его не активирует.
pub fn set_whip_capture(window: &Window, on: bool) {
    use super::input_debug;
    if CAPTURE_ON.replace(on) == on {
        return;
    }
    if !on {
        window.set_cursor_visible(true);
    }
    // winit пересобирает GWL_EXSTYLE целиком (теряются TOOLWINDOW/NOACTIVATE, а при
    // hittest=true и LAYERED) — сразу ставим наши стили обратно.
    let _ = window.set_cursor_hittest(on);
    let hwnd = hwnd(window);
    // SAFETY: hwnd — живое окно этого потока.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let mut ex = ex | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED;
        if on {
            ex &= !WS_EX_TRANSPARENT;
        } else {
            ex |= WS_EX_TRANSPARENT;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex as isize);
        SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    if on {
        // winit прячет курсор (ShowCursor), когда он внутри клиентской области — т.е. с
        // первого WM_MOUSEMOVE после снятия WS_EX_TRANSPARENT.
        window.set_cursor_visible(false);
    }
    input_debug!("захват {}", if on { "ВКЛ" } else { "ВЫКЛ" });
}
