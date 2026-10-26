//! Настройки приложения: `config.toml` в каталоге данных пользователя.
//! macOS: `~/Library/Application Support/DesktopSpider/config.toml`,
//! Windows: `%APPDATA%\DesktopSpider\config.toml`,
//! прочие: `$XDG_CONFIG_HOME` или `~/.config` + `desktop_spider/config.toml`.
//! `$SPIDER_CONFIG` — полный путь к файлу (для тестов и отладки).
//!
//! - `Settings::load()` — читает файл; нет файла или ошибка → значения по умолчанию (+ eprintln);
//!   файл правят руками, поэтому плохое поле → умолчание только для него, лишние ключи → предупреждение;
//! - `Settings::save(&self)` — пишет атомарно (tmp + rename), создавая каталог; ошибки только в лог;
//! - `Hotkey::parse("ctrl+alt")` / `Display` — формат `ctrl+alt+shift+cmd+<клавиша>`, регистр не важен;
//!   на Windows `cmd` = клавиша Win.

use std::fmt;
use std::path::{Path, PathBuf};

/// Режим приложения.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Только паук, плети нет (как в v0.1).
    #[default]
    Spider,
    /// Паук держит дистанцию и уворачивается, плеть по хоткею, счёт попаданий.
    Game,
    /// Паука нет, только плеть по хоткею; на месте удара — гаснущий след.
    Whip,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Spider => "spider",
            Mode::Game => "game",
            Mode::Whip => "whip",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "spider" => Some(Mode::Spider),
            "game" => Some(Mode::Game),
            "whip" => Some(Mode::Whip),
            _ => None,
        }
    }
}

/// Цвет плети.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WhipSkin {
    #[default]
    Light,
    Black,
}

impl WhipSkin {
    pub fn as_str(self) -> &'static str {
        match self {
            WhipSkin::Light => "light",
            WhipSkin::Black => "black",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(WhipSkin::Light),
            "black" => Some(WhipSkin::Black),
            _ => None,
        }
    }
}

/// Тон кожи плети. Палитра строится из базового цвета (`whip::skin::palette`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Leather {
    #[default]
    Natural,
    Cognac,
    Chocolate,
    Burgundy,
    Black,
    /// Свой цвет `#rrggbb`.
    Custom([u8; 3]),
}

impl Leather {
    /// Базовый цвет кожи, sRGB.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Leather::Natural => [0x77, 0x46, 0x1f],
            Leather::Cognac => [0x8a, 0x47, 0x19],
            Leather::Chocolate => [0x4a, 0x28, 0x16],
            Leather::Burgundy => [0x5e, 0x1c, 0x24],
            Leather::Black => [0x12, 0x12, 0x17],
            Leather::Custom(c) => c,
        }
    }
}

/// Сторона источника света: блик сильнее с этой стороны, тень — в противоположную.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LightSide {
    #[default]
    Left,
    Right,
}

impl LightSide {
    /// Единичное направление на свет в экранных координатах (Y вниз): сверху слева / справа.
    pub fn dir(self) -> glam::Vec2 {
        let x = if self == LightSide::Right { 0.45 } else { -0.45 };
        glam::Vec2::new(x, -0.89).normalize()
    }
}

/// Сочетание для «удерживать — плеть». Модификаторы + необязательная обычная клавиша.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Обычная клавиша (`"w"`, `"f13"`…), в нижнем регистре.
    /// `None` — только модификаторы (не требует разрешений на macOS).
    /// Допустимо: один печатный ASCII-символ, `f1`–`f24` или имя из `NAMED_KEYS`.
    pub key: Option<String>,
}

/// Именованные клавиши, которые принимает `Hotkey::parse` (кроме `f1`–`f24` и одиночных символов).
pub const NAMED_KEYS: &[&str] = &[
    "space", "tab", "enter", "esc", "backspace", "delete", "insert", "home", "end", "pageup",
    "pagedown", "up", "down", "left", "right", "capslock",
];

impl Default for Hotkey {
    /// `⌃⌥` / `Ctrl+Alt`.
    fn default() -> Self {
        Self { ctrl: true, alt: true, shift: false, cmd: false, key: None }
    }
}

impl Hotkey {
    /// Только модификаторы — для готовых пресетов меню.
    pub const fn mods(ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Self {
        Self { ctrl, alt, shift, cmd, key: None }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let mut h = Self::mods(false, false, false, false);
        if s.trim().is_empty() {
            return Err("пустой хоткей".into());
        }
        for raw in s.split('+') {
            let tok = raw.trim().to_lowercase();
            if tok.is_empty() {
                return Err(format!("пустая часть в хоткее {s:?}"));
            }
            let flag = match tok.as_str() {
                "ctrl" | "control" | "⌃" => Some(&mut h.ctrl),
                "alt" | "option" | "opt" | "⌥" => Some(&mut h.alt),
                "shift" | "⇧" => Some(&mut h.shift),
                "cmd" | "command" | "super" | "win" | "meta" | "⌘" => Some(&mut h.cmd),
                _ => None,
            };
            if let Some(flag) = flag {
                if *flag {
                    return Err(format!("модификатор {:?} повторяется", raw.trim()));
                }
                *flag = true;
                continue;
            }
            let key = normalize_key(&tok).ok_or_else(|| format!("неизвестная клавиша {:?}", raw.trim()))?;
            if h.key.is_some() {
                return Err(format!("больше одной обычной клавиши в {s:?}"));
            }
            h.key = Some(key);
        }
        Ok(h)
    }

    /// Подпись для меню: macOS `⌃⌥⇧⌘W`, Windows `Ctrl+Alt+Shift+Win+W`.
    pub fn label(&self) -> String {
        let key = self.key.as_deref().map(key_label);
        #[cfg(target_os = "macos")]
        {
            let mut s = String::new();
            for (on, sym) in [(self.ctrl, "⌃"), (self.alt, "⌥"), (self.shift, "⇧"), (self.cmd, "⌘")] {
                if on {
                    s.push_str(sym);
                }
            }
            if let Some(k) = key {
                s.push_str(&k);
            }
            s
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut parts: Vec<String> = Vec::new();
            for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.cmd, "Win")] {
                if on {
                    parts.push(name.into());
                }
            }
            parts.extend(key);
            parts.join("+")
        }
    }
}

/// Приводит имя клавиши к каноническому виду; `None` — неизвестная.
fn normalize_key(tok: &str) -> Option<String> {
    let alias = match tok {
        "return" => "enter",
        "escape" => "esc",
        "del" => "delete",
        "ins" => "insert",
        "pgup" => "pageup",
        "pgdn" | "pgdown" => "pagedown",
        t => t,
    };
    let mut chars = alias.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        // Один печатный символ (кроме '+', он разделитель).
        return (c.is_ascii_graphic() && c != '+').then(|| alias.to_string());
    }
    if let Some(n) = alias.strip_prefix('f') {
        if let Ok(n) = n.parse::<u8>() {
            return (1..=24).contains(&n).then(|| alias.to_string());
        }
    }
    NAMED_KEYS.contains(&alias).then(|| alias.to_string())
}

/// Клавиша для подписи: `w` → `W`, `f13` → `F13`, `space` → `Space`.
fn key_label(k: &str) -> String {
    if k.chars().count() == 1 || (k.starts_with('f') && k[1..].chars().all(|c| c.is_ascii_digit())) {
        return k.to_uppercase();
    }
    let mut c = k.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [(self.ctrl, "ctrl"), (self.alt, "alt"), (self.shift, "shift"), (self.cmd, "cmd")] {
            if on {
                parts.push(name);
            }
        }
        if let Some(k) = &self.key {
            parts.push(k);
        }
        write!(f, "{}", parts.join("+"))
    }
}

/// Размер плети: множитель к эталону из эскиза (рукоять ≈ 50 pt, хлыст ≈ 190 pt).
pub const WHIP_SIZE_MIN: f32 = 0.5;
pub const WHIP_SIZE_MAX: f32 = 3.0;
/// Готовые размеры для меню.
pub const WHIP_SIZES: [(f32, &str); 4] = [(0.7, "Маленькая"), (1.0, "Средняя"), (1.4, "Большая"), (2.0, "Огромная")];

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    pub whip_skin: WhipSkin,
    /// Множитель размера плети, `WHIP_SIZE_MIN..=WHIP_SIZE_MAX`.
    pub whip_size: f32,
    /// Тон кожи (заменит `whip_skin`; старый ключ читается как natural / black).
    pub whip_leather: Leather,
    pub whip_light: LightSide,
    /// Сила блика 0..1 (0,5 — средний).
    pub whip_gloss: f32,
    pub whip_hotkey: Hotkey,
    /// Имя пресета паука; пусто — `$SPIDER_PRESET` или "spider".
    pub preset: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            whip_skin: WhipSkin::default(),
            whip_size: 1.0,
            whip_leather: Leather::default(),
            whip_light: LightSide::default(),
            whip_gloss: 0.5,
            whip_hotkey: Hotkey::default(),
            preset: String::new(),
        }
    }
}

/// Формат файла при записи.
#[derive(serde::Serialize)]
struct RawOut<'a> {
    mode: &'a str,
    whip_skin: &'a str,
    whip_size: f32,
    whip_hotkey: String,
    preset: &'a str,
}

impl Settings {
    pub fn load() -> Self {
        match config_path() {
            Some(p) => Self::load_from(&p),
            None => {
                eprintln!("settings: не найден каталог настроек, беру умолчания");
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        match config_path() {
            Some(p) => {
                if let Err(e) = self.save_to(&p) {
                    eprintln!("settings: не удалось сохранить {}: {e}", p.display());
                }
            }
            None => eprintln!("settings: не найден каталог настроек, не сохраняю"),
        }
    }

    /// Нет файла → умолчания молча; битый файл → умолчания + лог.
    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text, &path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                eprintln!("settings: не прочитать {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Разбор по полям: каждое плохое поле → умолчание для него.
    pub fn from_toml(text: &str, origin: &str) -> Self {
        let mut s = Self::default();
        let table: toml::Table = match toml::from_str(text) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("settings: {origin}: ошибка TOML, беру умолчания: {e}");
                return s;
            }
        };
        for (k, v) in &table {
            // Единственное числовое поле; остальные — строки.
            if k == "whip_size" {
                match v.as_float().or_else(|| v.as_integer().map(|i| i as f64)) {
                    Some(x) => {
                        let x = x as f32;
                        s.whip_size = x.clamp(WHIP_SIZE_MIN, WHIP_SIZE_MAX);
                        if s.whip_size != x {
                            eprintln!("settings: {origin}: whip_size = {x} вне {WHIP_SIZE_MIN}..{WHIP_SIZE_MAX}, беру {}", s.whip_size);
                        }
                    }
                    None => eprintln!("settings: {origin}: whip_size должен быть числом, игнорирую"),
                }
                continue;
            }
            let Some(val) = v.as_str() else {
                eprintln!("settings: {origin}: {k} должен быть строкой, игнорирую");
                continue;
            };
            match k.as_str() {
                "mode" => match Mode::parse(val) {
                    Some(m) => s.mode = m,
                    None => eprintln!("settings: {origin}: mode = {val:?} — ожидаю spider|game|whip"),
                },
                "whip_skin" => match WhipSkin::parse(val) {
                    Some(m) => s.whip_skin = m,
                    None => eprintln!("settings: {origin}: whip_skin = {val:?} — ожидаю light|black"),
                },
                "whip_hotkey" => match Hotkey::parse(val) {
                    Ok(h) => s.whip_hotkey = h,
                    Err(e) => eprintln!("settings: {origin}: whip_hotkey = {val:?}: {e}"),
                },
                "preset" => s.preset = val.to_string(),
                _ => eprintln!("settings: {origin}: неизвестный ключ {k:?}, игнорирую"),
            }
        }
        s
    }

    pub fn to_toml(&self) -> String {
        let raw = RawOut {
            mode: self.mode.as_str(),
            whip_skin: self.whip_skin.as_str(),
            whip_size: self.whip_size,
            whip_hotkey: self.whip_hotkey.to_string(),
            preset: &self.preset,
        };
        toml::to_string(&raw).expect("settings serialize")
    }

    /// Атомарная запись: временный файл рядом + rename.
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, self.to_toml())?;
        // На Windows rename поверх существующего файла работает (MoveFileEx с REPLACE_EXISTING).
        std::fs::rename(&tmp, path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

/// Путь к `config.toml` (см. шапку модуля).
pub fn config_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(p) = env("SPIDER_CONFIG") {
        return Some(p);
    }
    #[cfg(target_os = "macos")]
    let dir = env("HOME").map(|h| h.join("Library/Application Support/DesktopSpider"));
    #[cfg(windows)]
    let dir = env("APPDATA").map(|a| a.join("DesktopSpider"));
    #[cfg(not(any(target_os = "macos", windows)))]
    let dir = env("XDG_CONFIG_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".config")))
        .map(|c| c.join("desktop_spider"));
    dir.map(|d| d.join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("desktop_spider_test_{}_{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn roundtrip() {
        let s = Settings {
            mode: Mode::Game,
            whip_skin: WhipSkin::Black,
            whip_size: 1.4,
            whip_leather: Leather::default(),
            whip_light: LightSide::default(),
            whip_gloss: 0.5,
            whip_hotkey: Hotkey::parse("shift+cmd+F13").unwrap(),
            preset: "tarantula".into(),
        };
        let dir = tmp_dir("roundtrip");
        let p = dir.join("sub/config.toml");
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        // Перезапись поверх существующего файла.
        let s2 = Settings { mode: Mode::Whip, ..s };
        s2.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s2);
        assert!(!dir.join("sub/config.toml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_default() {
        let p = tmp_dir("missing").join("config.toml");
        assert_eq!(Settings::load_from(&p), Settings::default());
    }

    #[test]
    fn partial_file() {
        let s = Settings::from_toml("whip_skin = \"black\"\n", "test");
        assert_eq!(s, Settings { whip_skin: WhipSkin::Black, ..Settings::default() });
    }

    #[test]
    fn garbage_values() {
        let text = r#"
            mode = "dance"
            whip_skin = 5
            whip_hotkey = "ctrl+ctrl"
            preset = "big"
            unknown = "x"
            [section]
            a = 1
        "#;
        let s = Settings::from_toml(text, "test");
        assert_eq!(s, Settings { preset: "big".into(), ..Settings::default() });
        // Совсем не TOML.
        assert_eq!(Settings::from_toml("mode = = ", "test"), Settings::default());
        // Регистр значений не важен.
        assert_eq!(Settings::from_toml("mode = \"WHIP\"", "test").mode, Mode::Whip);
    }

    #[test]
    fn hotkey_parse_display() {
        let h = Hotkey::parse("Control + Option").unwrap();
        assert_eq!(h, Hotkey::default());
        assert_eq!(h.to_string(), "ctrl+alt");

        let h = Hotkey::parse("W+⌘+shift").unwrap();
        assert_eq!(h, Hotkey { shift: true, cmd: true, key: Some("w".into()), ..Hotkey::mods(false, false, false, false) });
        assert_eq!(h.to_string(), "shift+cmd+w");
        assert_eq!(Hotkey::parse(&h.to_string()).unwrap(), h);

        assert_eq!(Hotkey::parse("win+alt+ctrl+meta").unwrap_err().contains("повторяется"), true);
        assert_eq!(Hotkey::parse("f13").unwrap().to_string(), "f13");
        assert_eq!(Hotkey::parse("ctrl+Escape").unwrap().key.as_deref(), Some("esc"));
        assert!(Hotkey::parse("").is_err());
        assert!(Hotkey::parse("ctrl+").is_err());
        assert!(Hotkey::parse("ctrl+a+b").is_err());
        assert!(Hotkey::parse("ctrl+banana").is_err());
        assert!(Hotkey::parse("f25").is_err());
        assert!(Hotkey::parse("alt+alt").is_err());
    }

    #[test]
    fn hotkey_label() {
        let all = Hotkey::parse("cmd+shift+alt+ctrl+w").unwrap();
        let mods = Hotkey::parse("ctrl+alt").unwrap();
        let f = Hotkey::parse("alt+f13").unwrap();
        let sp = Hotkey::parse("ctrl+space").unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(all.label(), "⌃⌥⇧⌘W");
            assert_eq!(mods.label(), "⌃⌥");
            assert_eq!(f.label(), "⌥F13");
            assert_eq!(sp.label(), "⌃Space");
        } else {
            assert_eq!(all.label(), "Ctrl+Alt+Shift+Win+W");
            assert_eq!(mods.label(), "Ctrl+Alt");
            assert_eq!(f.label(), "Alt+F13");
            assert_eq!(sp.label(), "Ctrl+Space");
        }
    }

    /// Единственный тест, трогающий `$SPIDER_CONFIG`.
    #[test]
    fn spider_config_path() {
        let dir = tmp_dir("envpath");
        let p = dir.join("nested/my.toml");
        std::env::set_var("SPIDER_CONFIG", &p);
        assert_eq!(config_path(), Some(p.clone()));
        let s = Settings { mode: Mode::Whip, preset: "x".into(), ..Settings::default() };
        s.save();
        assert!(p.exists());
        assert_eq!(Settings::load(), s);
        std::env::remove_var("SPIDER_CONFIG");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn whip_size_parse_and_clamp() {
        assert_eq!(Settings::from_toml("whip_size = 1.4\n", "t").whip_size, 1.4);
        assert_eq!(Settings::from_toml("whip_size = 2\n", "t").whip_size, 2.0);
        assert_eq!(Settings::from_toml("whip_size = 9.0\n", "t").whip_size, WHIP_SIZE_MAX);
        assert_eq!(Settings::from_toml("whip_size = 0.1\n", "t").whip_size, WHIP_SIZE_MIN);
        assert_eq!(Settings::from_toml("whip_size = \"big\"\n", "t").whip_size, 1.0);
    }
}
