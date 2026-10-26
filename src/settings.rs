//! Настройки приложения: `config.toml` в каталоге данных пользователя.
//! macOS: `~/Library/Application Support/DesktopSpider/config.toml`,
//! Windows: `%APPDATA%\DesktopSpider\config.toml`.
//!
//! Контракт (реализует агент настроек; сейчас заглушки):
//! - `Settings::load()` — читает файл; нет файла или ошибка → `Settings::default()` (+ eprintln при ошибке);
//! - `Settings::save(&self)` — пишет файл, создавая каталог; ошибки только в лог;
//! - `Hotkey::parse("ctrl+alt")` / `Display` — формат `ctrl+alt+shift+cmd+<клавиша>`, регистр не важен;
//!   на Windows `cmd` = клавиша Win.

use std::fmt;

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

/// Цвет плети.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WhipSkin {
    #[default]
    Light,
    Black,
}

/// Сочетание для «удерживать — плеть». Модификаторы + необязательная обычная клавиша.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
    /// Обычная клавиша (`"w"`, `"f13"`…). `None` — только модификаторы (не требует разрешений на macOS).
    pub key: Option<String>,
}

impl Default for Hotkey {
    /// `⌃⌥` / `Ctrl+Alt`.
    fn default() -> Self {
        Self { ctrl: true, alt: true, shift: false, cmd: false, key: None }
    }
}

impl Hotkey {
    pub fn parse(_s: &str) -> Result<Self, String> {
        Ok(Self::default())
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ctrl+alt")
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Settings {
    pub mode: Mode,
    pub whip_skin: WhipSkin,
    pub whip_hotkey: Hotkey,
    /// Имя пресета паука; пусто — `$SPIDER_PRESET` или "spider".
    pub preset: String,
}

impl Settings {
    pub fn load() -> Self {
        Self::default()
    }

    pub fn save(&self) {}
}
