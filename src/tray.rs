//! Иконка в меню-баре (macOS) / трее (Windows): режим, плеть (цвет, хоткей), пресет, пауза,
//! отладка, выход; счёт игры в заголовке (macOS) / подсказке (Windows).
//!
//! События меню приходят из muda через `MenuEvent::set_event_handler`
//! (ставится в `main.rs`) и пересылаются в winit как `UserEvent::Menu`.
//! `Tray::action` превращает такое событие в `TrayAction`.

use tray_icon::menu::accelerator::Accelerator;
#[cfg(target_os = "macos")]
use tray_icon::menu::accelerator::{Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::settings::{Hotkey, Mode, Settings, WhipSkin};
use std::cell::Cell;

/// Готовые сочетания хоткея в меню: ⌃⌥, ⌃⇧, ⌥⇧, ⌃⌥⇧.
const HOTKEY_PRESETS: [Hotkey; 4] = [
    Hotkey::mods(true, true, false, false),
    Hotkey::mods(true, false, true, false),
    Hotkey::mods(false, true, true, false),
    Hotkey::mods(true, true, true, false),
];

/// Что пользователь выбрал в меню.
#[derive(Debug)]
pub enum TrayAction {
    Preset(String),
    Pause(bool),
    Debug(bool),
    Reload,
    Quit,
    /// Режим: паук / игра / просто плеть.
    Mode(crate::settings::Mode),
    /// Цвет плети.
    Skin(crate::settings::WhipSkin),
    /// Хоткей плети (из списка готовых сочетаний).
    Hotkey(crate::settings::Hotkey),
}

pub struct Tray {
    _icon: TrayIcon,
    presets_menu: Submenu,
    presets: Vec<(String, CheckMenuItem)>,
    pause: CheckMenuItem,
    debug: CheckMenuItem,
    reload: MenuItem,
    quit: MenuItem,
    /// Радиогруппа режимов.
    modes: Vec<(Mode, CheckMenuItem)>,
    /// Радиогруппа цветов плети.
    skins: Vec<(WhipSkin, CheckMenuItem)>,
    hotkey_menu: Submenu,
    hotkeys: Vec<(Hotkey, CheckMenuItem)>,
    /// «Своё: …» — неактивный пункт с галочкой, если хоткей из config.toml не из списка.
    custom_hotkey: CheckMenuItem,
    custom_shown: Cell<bool>,
}

impl Tray {
    /// Создавать только на главном потоке после старта event loop (в `resumed`).
    pub fn new(presets: &[String], current: &str, debug: bool) -> Result<Self, String> {
        let presets_menu = Submenu::new("Пресет", true);
        let pause = CheckMenuItem::new("Пауза", true, false, None);
        let debug = CheckMenuItem::new("Отладка", true, debug, None);
        let reload = MenuItem::new("Перезагрузить пресеты", true, None);
        let quit = MenuItem::new("Выход", true, quit_accelerator());

        // Режим: три check-пункта, ведут себя как радиокнопки.
        let mode_menu = Submenu::new("Режим", true);
        let modes: Vec<(Mode, CheckMenuItem)> = [
            (Mode::Spider, "Паук"),
            (Mode::Game, "Игра: попади по пауку"),
            (Mode::Whip, "Просто плеть"),
        ]
        .into_iter()
        .map(|(m, t)| (m, CheckMenuItem::new(t, true, m == Mode::default(), None)))
        .collect();
        for (_, item) in &modes {
            mode_menu.append(item).map_err(|e| e.to_string())?;
        }

        // Плеть: цвет + подменю хоткея.
        let whip_menu = Submenu::new("Плеть", true);
        let skins: Vec<(WhipSkin, CheckMenuItem)> = [(WhipSkin::Light, "Светлая"), (WhipSkin::Black, "Чёрная")]
            .into_iter()
            .map(|(k, t)| (k, CheckMenuItem::new(t, true, k == WhipSkin::default(), None)))
            .collect();
        let hotkey_menu = Submenu::new("Хоткей (удерживать)", true);
        let hotkeys: Vec<(Hotkey, CheckMenuItem)> = HOTKEY_PRESETS
            .iter()
            .map(|h| (h.clone(), CheckMenuItem::new(h.label(), true, *h == Hotkey::default(), None)))
            .collect();
        for (_, item) in &hotkeys {
            hotkey_menu.append(item).map_err(|e| e.to_string())?;
        }
        // Пункт «Своё» вставляется после пресетов только когда нужен (см. set_settings).
        let custom_hotkey = CheckMenuItem::new("Своё", false, true, None);
        hotkey_menu
            .append_items(&[
                &PredefinedMenuItem::separator(),
                &MenuItem::new("Изменить: config.toml", false, None),
            ])
            .map_err(|e| e.to_string())?;
        for (_, item) in &skins {
            whip_menu.append(item).map_err(|e| e.to_string())?;
        }
        whip_menu
            .append_items(&[&PredefinedMenuItem::separator(), &hotkey_menu])
            .map_err(|e| e.to_string())?;

        let menu = Menu::new();
        menu.append_items(&[
            &mode_menu,
            &whip_menu,
            &PredefinedMenuItem::separator(),
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
            modes,
            skins,
            hotkey_menu,
            hotkeys,
            custom_hotkey,
            custom_shown: Cell::new(false),
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
        // Радиогруппы: muda сама переключила галочку (и могла снять её с уже выбранного) —
        // сразу выставляем одну галочку у нажатого.
        if let Some(m) = pick(&self.modes, id) {
            return Some(TrayAction::Mode(m));
        }
        if let Some(k) = pick(&self.skins, id) {
            return Some(TrayAction::Skin(k));
        }
        if let Some(h) = pick(&self.hotkeys, id) {
            self.show_custom_hotkey(None);
            return Some(TrayAction::Hotkey(h));
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

/// Если `id` из группы — отмечает только его и возвращает значение.
fn pick<T: Clone>(group: &[(T, CheckMenuItem)], id: &tray_icon::menu::MenuId) -> Option<T> {
    let value = group.iter().find(|(_, item)| id == item.id())?.0.clone();
    for (_, item) in group {
        item.set_checked(id == item.id());
    }
    Some(value)
}

impl Tray {
    /// Отметить текущие режим, цвет и хоткей в меню. Вызывать сразу после `new`
    /// и после каждого изменения настроек.
    pub fn set_settings(&self, s: &Settings) {
        for (m, item) in &self.modes {
            item.set_checked(*m == s.mode);
        }
        for (k, item) in &self.skins {
            item.set_checked(*k == s.whip_skin);
        }
        let mut known = false;
        for (h, item) in &self.hotkeys {
            item.set_checked(*h == s.whip_hotkey);
            known |= *h == s.whip_hotkey;
        }
        self.show_custom_hotkey((!known).then_some(&s.whip_hotkey));
    }

    /// Показать «Своё: <хоткей> (config.toml)» после пресетов или убрать (`None`).
    fn show_custom_hotkey(&self, h: Option<&Hotkey>) {
        match h {
            Some(h) => {
                self.custom_hotkey.set_text(format!("Своё: {} (config.toml)", h.label()));
                self.custom_hotkey.set_checked(true);
                if !self.custom_shown.get() {
                    let _ = self.hotkey_menu.insert(&self.custom_hotkey, self.hotkeys.len());
                    self.custom_shown.set(true);
                }
            }
            None => {
                if self.custom_shown.get() {
                    let _ = self.hotkey_menu.remove(&self.custom_hotkey);
                    self.custom_shown.set(false);
                }
            }
        }
    }

    /// Счёт игры: заголовок у иконки (macOS) / подсказка (Windows). `None` — убрать.
    pub fn set_score(&self, text: Option<&str>) {
        #[cfg(target_os = "macos")]
        self._icon.set_title(text);
        #[cfg(not(target_os = "macos"))]
        {
            let tip = match text {
                Some(t) => format!("Desktop Spider — {t}"),
                None => "Desktop Spider".to_string(),
            };
            if let Err(e) = self._icon.set_tooltip(Some(tip)) {
                eprintln!("tray: set_tooltip: {e}");
            }
        }
    }
}
