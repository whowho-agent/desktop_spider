//! Иконка в меню-баре (macOS) / трее (Windows): выбор пресета, пауза, отладка, выход.
//!
//! События меню приходят из muda через `MenuEvent::set_event_handler`
//! (ставится в `main.rs`) и пересылаются в winit как `UserEvent::Menu`.
//! `Tray::action` превращает такое событие в `TrayAction`.

use tray_icon::menu::accelerator::Accelerator;
#[cfg(target_os = "macos")]
use tray_icon::menu::accelerator::{Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Что пользователь выбрал в меню.
#[derive(Debug)]
pub enum TrayAction {
    Preset(String),
    Pause(bool),
    Debug(bool),
    Reload,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    presets_menu: Submenu,
    presets: Vec<(String, CheckMenuItem)>,
    pause: CheckMenuItem,
    debug: CheckMenuItem,
    reload: MenuItem,
    quit: MenuItem,
}

impl Tray {
    /// Создавать только на главном потоке после старта event loop (в `resumed`).
    pub fn new(presets: &[String], current: &str, debug: bool) -> Result<Self, String> {
        let presets_menu = Submenu::new("Пресет", true);
        let pause = CheckMenuItem::new("Пауза", true, false, None);
        let debug = CheckMenuItem::new("Отладка", true, debug, None);
        let reload = MenuItem::new("Перезагрузить пресеты", true, None);
        let quit = MenuItem::new("Выход", true, quit_accelerator());

        let menu = Menu::new();
        menu.append_items(&[
            &presets_menu,
            &pause,
            &debug,
            &reload,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;

        let builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Desktop Spider");
        // macOS: шаблонная иконка, цвет подставит система под тему меню-бара.
        #[cfg(target_os = "macos")]
        let builder = builder.with_icon_templated(glyph_icon(false));
        // Windows: цветная иконка, тёмный паук с белой обводкой — виден на любой панели задач.
        #[cfg(not(target_os = "macos"))]
        let builder = builder.with_icon(glyph_icon(true));
        let icon = builder.build().map_err(|e| e.to_string())?;

        let mut tray = Self {
            _icon: icon,
            presets_menu,
            presets: Vec::new(),
            pause,
            debug,
            reload,
            quit,
        };
        tray.set_presets(presets, current);
        Ok(tray)
    }

    /// Пересобирает подменю пресетов и отмечает текущий.
    pub fn set_presets(&mut self, names: &[String], current: &str) {
        for (_, item) in self.presets.drain(..) {
            let _ = self.presets_menu.remove(&item);
        }
        for name in names {
            let item = CheckMenuItem::new(name, true, name == current, None);
            let _ = self.presets_menu.append(&item);
            self.presets.push((name.clone(), item));
        }
    }

    /// Ставит галочку только у текущего пресета.
    pub fn set_current(&self, current: &str) {
        for (name, item) in &self.presets {
            item.set_checked(name == current);
        }
    }

    pub fn set_paused(&self, on: bool) {
        self.pause.set_checked(on);
    }

    pub fn set_debug(&self, on: bool) {
        self.debug.set_checked(on);
    }

    /// Разбирает событие меню. Check-пункты переключает сама muda (macOS и Windows),
    /// поэтому новое состояние читаем из `is_checked`.
    pub fn action(&self, ev: &MenuEvent) -> Option<TrayAction> {
        let id = &ev.id;
        if id == self.pause.id() {
            return Some(TrayAction::Pause(self.pause.is_checked()));
        }
        if id == self.debug.id() {
            return Some(TrayAction::Debug(self.debug.is_checked()));
        }
        if id == self.reload.id() {
            return Some(TrayAction::Reload);
        }
        if id == self.quit.id() {
            return Some(TrayAction::Quit);
        }
        self.presets
            .iter()
            .find(|(_, item)| id == item.id())
            .map(|(name, _)| TrayAction::Preset(name.clone()))
    }
}

/// Cmd+Q виден в меню и работает, пока меню открыто (Accessory-приложение
/// не бывает активным, глобально хоткей не перехватывается).
#[cfg(target_os = "macos")]
fn quit_accelerator() -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, Code::KeyQ))
}

/// На Windows у контекстного меню трея горячих клавиш нет — не обещаем лишнего.
#[cfg(not(target_os = "macos"))]
fn quit_accelerator() -> Option<Accelerator> {
    None
}

/// Процедурный глиф паука 36×36 (в меню-баре — 18 pt @2x): тело + 8 ломаных ног.
/// `outlined = false`: шаблон, важна только альфа (macOS). `true`: тёмный глиф
/// с белой обводкой ~1 px (Windows, где иконка показывается как есть).
fn glyph_icon(outlined: bool) -> Icon {
    const N: usize = 36;
    let mut a = vec![0f32; N * N]; // расстояние до глифа, px
    let c = N as f32 / 2.0;

    // Расстояние от точки до отрезка.
    fn seg(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (qx, qy) = (a.0 + dx * t - p.0, a.1 + dy * t - p.1);
        (qx * qx + qy * qy).sqrt()
    }

    // Ноги: колено вверх-наружу, ступня ниже.
    let mut legs = Vec::new();
    for side in [-1.0f32, 1.0] {
        for (hy, ky, fy) in [(-3.0, -11.0, -16.0), (-1.0, -6.0, -3.0), (1.0, 2.0, 8.0), (3.0, 8.0, 16.0)] {
            let hip = (c + side * 3.0, c + hy);
            let knee = (c + side * 11.0, c + ky);
            let foot = (c + side * 16.0, c + fy);
            legs.push((hip, knee));
            legs.push((knee, foot));
        }
    }

    for y in 0..N {
        for x in 0..N {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            // Тело (эллипс) и голова.
            let body = (((p.0 - c) / 6.0).powi(2) + ((p.1 - c - 2.0) / 8.0).powi(2)).sqrt() - 1.0;
            let head = ((p.0 - c).powi(2) + (p.1 - c + 8.0).powi(2)).sqrt() - 4.0;
            let mut d = (body * 6.0).min(head);
            for &(s, e) in &legs {
                d = d.min(seg(p, s, e) - 1.3);
            }
            a[y * N + x] = d;
        }
    }

    let rgba = a
        .iter()
        .flat_map(|&d| {
            let core = (0.5 - d).clamp(0.0, 1.0);
            if !outlined {
                return [0, 0, 0, (core * 255.0) as u8];
            }
            let halo = (1.7 - d).clamp(0.0, 1.0);
            // Внутри — почти чёрный, на кромке — белый; альфа по обводке.
            let k = if halo > 0.0 { core / halo } else { 0.0 };
            let c = (255.0 * (1.0 - k) + 30.0 * k) as u8;
            [c, c, c, (halo * 255.0) as u8]
        })
        .collect();
    Icon::from_rgba(rgba, N as u32, N as u32).expect("icon rgba")
}
