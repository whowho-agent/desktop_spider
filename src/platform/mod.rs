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

//! Плеть: `hotkey_held(&Hotkey)`, `set_whip_capture(&Window, on)` — см. комментарии в реализациях;
//! `input_debug_enabled()` — `SPIDER_INPUT_DEBUG=1`, лог переходов в stderr.

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

/// Отладка ввода плети: `SPIDER_INPUT_DEBUG=1` → переходы хоткея/захвата в stderr.
pub fn input_debug_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("SPIDER_INPUT_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

macro_rules! input_debug {
    ($($arg:tt)*) => {
        if $crate::platform::input_debug_enabled() {
            eprintln!("[input] {}", format!($($arg)*));
        }
    };
}
#[allow(unused_imports)]
pub(crate) use input_debug;

/// Общая (чистая) логика хоткея; платформенные файлы только опрашивают ОС.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod keys {
    use std::cell::{Cell, RefCell};

    use crate::settings::Hotkey;

    /// Зажатые модификаторы. `cmd` = ⌘ на macOS, Win на Windows.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Mods {
        pub ctrl: bool,
        pub alt: bool,
        pub shift: bool,
        pub cmd: bool,
    }

    /// Модификаторы должны совпасть ТОЧНО: лишний модификатор → не держится.
    /// Так `ctrl+alt` не срабатывает на чужих сочетаниях вроде ⌃⌥⌘X / Ctrl+Alt+Del-подобных.
    /// Хоткей без модификаторов и без клавиши не срабатывает никогда.
    pub fn mods_match(hk: &Hotkey, m: Mods) -> bool {
        let none = !(hk.ctrl || hk.alt || hk.shift || hk.cmd);
        if none && hk.key.is_none() {
            return false;
        }
        hk.ctrl == m.ctrl && hk.alt == m.alt && hk.shift == m.shift && hk.cmd == m.cmd
    }

    /// Символ → «базовый» символ клавиши в раскладке US (`!` → `1`, `?` → `/`).
    /// Т.е. `ctrl+!` ловит физическую клавишу `1`; Shift при этом всё равно нужно указать явно.
    fn base_char(c: char) -> char {
        match c {
            '!' => '1', '@' => '2', '#' => '3', '$' => '4', '%' => '5',
            '^' => '6', '&' => '7', '*' => '8', '(' => '9', ')' => '0',
            '_' => '-', '+' => '=', '{' => '[', '}' => ']', '|' => '\\',
            ':' => ';', '"' => '\'', '<' => ',', '>' => '.', '?' => '/', '~' => '`',
            c => c.to_ascii_lowercase(),
        }
    }

    /// Номер F-клавиши из `f1`…`f24`.
    fn f_number(name: &str) -> Option<u16> {
        let n: u16 = name.strip_prefix('f')?.parse().ok()?;
        (1..=24).contains(&n).then_some(n)
    }

    fn single_char(name: &str) -> Option<char> {
        let mut it = name.chars();
        let c = it.next()?;
        (it.next().is_none() && (c == ' ' || c.is_ascii_graphic())).then(|| base_char(c))
    }

    /// Имя клавиши (формат `settings::Hotkey::key`) → виртуальный код macOS (`kVK_*`, US ANSI).
    pub fn mac_keycode(name: &str) -> Option<u16> {
        let name = name.to_ascii_lowercase();
        if let Some(n) = f_number(&name) {
            // F21–F24 на Mac нет.
            const F: [u16; 20] = [
                122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64,
                79, 80, 90,
            ];
            return F.get(n as usize - 1).copied();
        }
        if let Some(c) = single_char(&name) {
            const LETTERS: [u16; 26] = [
                0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9,
                13, 7, 16, 6,
            ];
            const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
            return Some(match c {
                'a'..='z' => LETTERS[c as usize - 'a' as usize],
                '0'..='9' => DIGITS[c as usize - '0' as usize],
                ' ' => 49,
                '-' => 27, '=' => 24, '[' => 33, ']' => 30, '\\' => 42, ';' => 41,
                '\'' => 39, ',' => 43, '.' => 47, '/' => 44, '`' => 50,
                _ => return None,
            });
        }
        Some(match name.as_str() {
            "space" => 49,
            "tab" => 48,
            "enter" => 36,
            "esc" => 53,
            "backspace" => 51,
            "delete" => 117, // forward delete
            "insert" => 114, // Help — на месте Insert у полноразмерных клавиатур
            "home" => 115,
            "end" => 119,
            "pageup" => 116,
            "pagedown" => 121,
            "left" => 123,
            "right" => 124,
            "down" => 125,
            "up" => 126,
            "capslock" => 57,
            _ => return None,
        })
    }

    /// Имя клавиши → Windows virtual-key (`VK_*`, раскладка US для OEM-клавиш).
    pub fn win_vk(name: &str) -> Option<u16> {
        let name = name.to_ascii_lowercase();
        if let Some(n) = f_number(&name) {
            return Some(0x70 + n - 1); // VK_F1..VK_F24
        }
        if let Some(c) = single_char(&name) {
            return Some(match c {
                'a'..='z' => c.to_ascii_uppercase() as u16,
                '0'..='9' => c as u16,
                ' ' => 0x20,
                ';' => 0xBA, '=' => 0xBB, ',' => 0xBC, '-' => 0xBD, '.' => 0xBE,
                '/' => 0xBF, '`' => 0xC0, '[' => 0xDB, '\\' => 0xDC, ']' => 0xDD, '\'' => 0xDE,
                _ => return None,
            });
        }
        Some(match name.as_str() {
            "space" => 0x20,
            "tab" => 0x09,
            "enter" => 0x0D,
            "esc" => 0x1B,
            "backspace" => 0x08,
            "delete" => 0x2E,
            "insert" => 0x2D,
            "home" => 0x24,
            "end" => 0x23,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "left" => 0x25,
            "up" => 0x26,
            "right" => 0x27,
            "down" => 0x28,
            "capslock" => 0x14,
            _ => return None,
        })
    }

    thread_local! {
        static LAST_HELD: Cell<bool> = const { Cell::new(false) };
        static WARNED_KEY: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// Неизвестная клавиша: одно сообщение на имя (не каждый кадр).
    pub fn warn_unknown_key(name: &str) {
        WARNED_KEY.with_borrow_mut(|w| {
            if w.as_deref() != Some(name) {
                eprintln!("хоткей плети: клавиша «{name}» не поддерживается на этой платформе");
                *w = Some(name.to_string());
            }
        });
    }

    /// Итог опроса + лог перехода (при `SPIDER_INPUT_DEBUG`).
    pub fn report(hk: &Hotkey, held: bool) -> bool {
        if LAST_HELD.replace(held) != held {
            super::input_debug!("хоткей {hk} {}", if held { "зажат" } else { "отпущен" });
        }
        held
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn hk(ctrl: bool, alt: bool, shift: bool, cmd: bool, key: Option<&str>) -> Hotkey {
            Hotkey { ctrl, alt, shift, cmd, key: key.map(str::to_string) }
        }
        fn m(ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Mods {
            Mods { ctrl, alt, shift, cmd }
        }

        #[test]
        fn exact_modifier_match() {
            let h = hk(true, true, false, false, None);
            assert!(mods_match(&h, m(true, true, false, false)));
            assert!(!mods_match(&h, m(true, false, false, false)));
            assert!(!mods_match(&h, m(true, true, true, false)), "лишний Shift");
            assert!(!mods_match(&h, m(true, true, false, true)), "лишний Cmd");
            assert!(!mods_match(&h, Mods::default()));
        }

        #[test]
        fn empty_hotkey_never_matches() {
            assert!(!mods_match(&hk(false, false, false, false, None), Mods::default()));
            // Только клавиша, без модификаторов: модификаторы должны быть отпущены.
            let h = hk(false, false, false, false, Some("f13"));
            assert!(mods_match(&h, Mods::default()));
            assert!(!mods_match(&h, m(false, false, true, false)));
        }

        #[test]
        fn mac_codes() {
            assert_eq!(mac_keycode("a"), Some(0));
            assert_eq!(mac_keycode("w"), Some(13));
            assert_eq!(mac_keycode("z"), Some(6));
            assert_eq!(mac_keycode("0"), Some(29));
            assert_eq!(mac_keycode("5"), Some(23));
            assert_eq!(mac_keycode("%"), Some(23));
            assert_eq!(mac_keycode("f1"), Some(122));
            assert_eq!(mac_keycode("f13"), Some(105));
            assert_eq!(mac_keycode("f20"), Some(90));
            assert_eq!(mac_keycode("f21"), None);
            assert_eq!(mac_keycode("space"), Some(49));
            assert_eq!(mac_keycode(" "), Some(49));
            assert_eq!(mac_keycode("esc"), Some(53));
            assert_eq!(mac_keycode("/"), Some(44));
            assert_eq!(mac_keycode("bogus"), None);
        }

        #[test]
        fn win_codes() {
            assert_eq!(win_vk("a"), Some(0x41));
            assert_eq!(win_vk("w"), Some(0x57));
            assert_eq!(win_vk("7"), Some(0x37));
            assert_eq!(win_vk("f1"), Some(0x70));
            assert_eq!(win_vk("f24"), Some(0x87));
            assert_eq!(win_vk("f25"), None);
            assert_eq!(win_vk("esc"), Some(0x1B));
            assert_eq!(win_vk("?"), Some(0xBF));
            assert_eq!(win_vk("bogus"), None);
        }

        /// Всё, что принимает `Hotkey::parse`, покрыто обеими картами (кроме F21–F24 на Mac).
        #[test]
        fn covers_settings_key_set() {
            let mut names: Vec<String> =
                crate::settings::NAMED_KEYS.iter().map(|s| s.to_string()).collect();
            names.extend((1..=24).map(|n| format!("f{n}")));
            names.extend((0x20u8..0x7F).map(|b| (b as char).to_string()));
            for n in &names {
                assert!(win_vk(n).is_some(), "win: {n:?}");
                let mac_ok = mac_keycode(n).is_some();
                let f_hi = f_number(n).is_some_and(|k| k > 20);
                assert_eq!(mac_ok, !f_hi, "mac: {n:?}");
            }
        }

        #[test]
        fn mac_codes_unique_for_named() {
            let mut seen = std::collections::HashMap::new();
            for n in crate::settings::NAMED_KEYS.iter().filter(|n| **n != "space") {
                let c = mac_keycode(n).unwrap();
                assert!(seen.insert(c, *n).is_none(), "{n} дублирует {:?}", seen.get(&c));
            }
        }
    }
}
