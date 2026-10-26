//! Параметры существа и пресеты.
//!
//! Встроенные пресеты вшиты в бинарник (`presets/*.toml` на этапе сборки) — для работы по
//! умолчанию файлы не нужны. Пользовательские лежат в `presets_dir()`: новые имена добавляются
//! в меню, файл с именем встроенного пресета заменяет его.
//! - `list_presets()`: встроенные ∪ пользовательские, отсортированы;
//! - `load_preset(name)`: файл пользователя, иначе встроенный; отсутствующие поля — из `Preset::default()`;
//! - `PresetWatcher::poll()`: `true`, если с прошлого вызова какой-то `*.toml` в папке изменился.
//!
//! Папка: `$SPIDER_PRESETS`, иначе `presets/` рядом с `config.toml`
//! (macOS `~/Library/Application Support/DesktopSpider/presets`, Windows `%APPDATA%\DesktopSpider\presets`).
//! Цвета в TOML — строки `"#rrggbb"` или `"#rrggbbaa"`. Неизвестные ключи — ошибка (ловим опечатки).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    #[serde(with = "color")]
    pub color_leg: [f32; 4],
    #[serde(with = "color")]
    pub color_joint: [f32; 4],
    #[serde(with = "color")]
    pub color_body: [f32; 4],
    #[serde(with = "color")]
    pub color_head: [f32; 4],
    #[serde(with = "color")]
    pub color_ring: [f32; 4],
    #[serde(with = "color")]
    pub color_debug: [f32; 4],
    pub line_width: f32,
    pub joint_dot: f32,

    pub body: BodyParams,
    pub legs: LegParams,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BodyParams {
    pub max_speed: f32,
    pub max_accel: f32,
    /// Скорость поворота корпуса к направлению движения, 1/с.
    pub turn_rate: f32,
    /// Радиус кольца вокруг курсора: здесь паук останавливается.
    pub stop_radius: f32,
    /// С этого расстояния паук начинает тормозить.
    pub slow_radius: f32,
    pub half_len: f32,
    pub half_width: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LegParams {
    /// Углы rest-позиций пар ног от направления «вперёд», в градусах (пары спереди назад).
    /// Длина вектора = число пар ног.
    pub spread_deg: Vec<f32>,
    /// Точки крепления пар вдоль тела, доля от half_len (+ = вперёд). Та же длина, что spread_deg.
    pub hip_along: Vec<f32>,
    /// Расстояние от бедра до rest-позиции ступни.
    pub rest_dist: f32,
    /// Длины сегментов ноги от бедра к ступне: 2 → аналитический IK, 3+ → FABRIK.
    pub segments: Vec<f32>,
    pub style: LegStyle,
    /// Ступня шагает, когда отстала от цели дальше этого.
    pub step_threshold: f32,
    /// Порог «доводочного» шага, когда паук стоит.
    pub settle_threshold: f32,
    pub step_duration: f32,
    /// Упреждение цели по скорости тела, секунд.
    pub step_lead: f32,
    /// Насколько колено выгибается наружу в середине шага (имитация подъёма).
    pub lift: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LegStyle {
    /// Прямые отрезки + точки на суставах.
    #[default]
    Straight,
    /// Сглаженная кривая через суставы (щупальца).
    Curve,
    /// Та же кривая, но пунктиром из точек.
    Dotted,
}

fn hex(rgb: u32, a: f32) -> [f32; 4] {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        a,
    ]
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            name: "spider".into(),
            color_leg: hex(0x3a3aff, 1.0),
            color_joint: hex(0xffffff, 1.0),
            color_body: hex(0x7fe0b0, 1.0),
            color_head: hex(0xffffff, 1.0),
            color_ring: hex(0x8080a0, 0.35),
            color_debug: hex(0x3030a0, 0.6),
            line_width: 1.2,
            joint_dot: 1.5,
            body: BodyParams {
                max_speed: 520.0,
                max_accel: 2600.0,
                turn_rate: 9.0,
                stop_radius: 45.0,
                slow_radius: 160.0,
                half_len: 12.5,
                half_width: 7.5,
            },
            legs: LegParams {
                spread_deg: vec![40.0, 80.0, 115.0, 150.0],
                hip_along: vec![0.3, 0.1, -0.1, -0.3],
                rest_dist: 95.0,
                segments: vec![46.0, 46.0, 46.0],
                style: LegStyle::Straight,
                step_threshold: 34.0,
                settle_threshold: 5.0,
                step_duration: 0.1,
                step_lead: 0.05,
                lift: 9.0,
            },
        }
    }
}

// Частичные таблицы `[body]` / `[legs]` добирают недостающее из дефолтного пресета.
impl Default for BodyParams {
    fn default() -> Self {
        Preset::default().body
    }
}

impl Default for LegParams {
    fn default() -> Self {
        Preset::default().legs
    }
}

/// (Де)сериализация цвета `[f32; 4]` ↔ `"#rrggbb"` / `"#rrggbbaa"`.
mod color {
    use super::*;

    pub fn parse(s: &str) -> Result<[f32; 4], String> {
        let bad = || format!("неверный цвет {s:?}: ожидается \"#rrggbb\" или \"#rrggbbaa\"");
        let h = s.strip_prefix('#').ok_or_else(bad)?;
        if !(h.len() == 6 || h.len() == 8) || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap() as f32 / 255.0;
        let a = if h.len() == 8 { byte(6) } else { 1.0 };
        Ok([byte(0), byte(2), byte(4), a])
    }

    pub fn format(c: &[f32; 4]) -> String {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let s = format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]));
        if b(c[3]) == 255 {
            s
        } else {
            format!("{s}{:02x}", b(c[3]))
        }
    }

    pub fn serialize<S: Serializer>(c: &[f32; 4], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(c))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[f32; 4], D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Встроенные пресеты: имя → TOML, вшиты при сборке.
const BUILTIN: &[(&str, &str)] = &[
    ("blue3", include_str!("../presets/blue3.toml")),
    ("harvestman", include_str!("../presets/harvestman.toml")),
    ("octopus", include_str!("../presets/octopus.toml")),
    ("spider", include_str!("../presets/spider.toml")),
    ("tentacle", include_str!("../presets/tentacle.toml")),
];

/// Папка пользовательских пресетов (может не существовать).
pub fn presets_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SPIDER_PRESETS") {
        return PathBuf::from(dir);
    }
    crate::settings::config_path()
        .and_then(|p| p.parent().map(|d| d.join("presets")))
        .unwrap_or_else(|| PathBuf::from("presets"))
}

pub fn list_presets() -> Vec<String> {
    list_presets_in(&presets_dir())
}

/// Встроенные плюс файлы из папки (без повторов), по алфавиту.
fn list_presets_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = BUILTIN.iter().map(|(n, _)| n.to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(dir) {
        names.extend(
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| is_toml(p) && p.is_file())
                .filter_map(|p| p.file_stem()?.to_str().map(str::to_owned)),
        );
    }
    names.sort();
    names.dedup();
    names
}

fn is_toml(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "toml")
}

pub fn load_preset(name: &str) -> Result<Preset, String> {
    load_preset_in(&presets_dir(), name)
}

/// Файл пользователя важнее встроенного пресета с тем же именем.
fn load_preset_in(dir: &Path, name: &str) -> Result<Preset, String> {
    let path = dir.join(format!("{name}.toml"));
    let mut preset = if path.is_file() {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("пресет {}: не удалось прочитать: {e}", path.display()))?;
        parse_preset(&text).map_err(|e| format!("пресет {}: {e}", path.display()))?
    } else if let Some((_, text)) = BUILTIN.iter().find(|(n, _)| *n == name) {
        parse_preset(text).map_err(|e| format!("встроенный пресет {name}: {e}"))?
    } else {
        return Err(format!("пресет «{name}» не найден: нет встроенного и нет файла {}", path.display()));
    };
    preset.name = name.to_owned();
    Ok(preset)
}

/// Разбор + проверка, без привязки к файлу.
fn parse_preset(text: &str) -> Result<Preset, String> {
    let p: Preset = toml::from_str(text).map_err(|e| e.to_string())?;
    validate(&p)?;
    Ok(p)
}

fn validate(p: &Preset) -> Result<(), String> {
    let l = &p.legs;
    if l.spread_deg.is_empty() || l.spread_deg.len() != l.hip_along.len() {
        return Err(format!(
            "legs.spread_deg ({}) и legs.hip_along ({}) должны быть одной длины ≥ 1",
            l.spread_deg.len(),
            l.hip_along.len()
        ));
    }
    if l.segments.len() < 2 {
        return Err(format!("legs.segments: нужно ≥ 2 сегментов, задано {}", l.segments.len()));
    }
    let lengths = [
        ("legs.rest_dist", l.rest_dist),
        ("legs.step_duration", l.step_duration),
        ("body.half_len", p.body.half_len),
        ("body.half_width", p.body.half_width),
    ];
    for (key, v) in lengths {
        if !(v > 0.0) {
            return Err(format!("{key} должно быть > 0, задано {v}"));
        }
    }
    if let Some((i, v)) = l.segments.iter().enumerate().find(|(_, v)| !(**v > 0.0)) {
        return Err(format!("legs.segments[{i}] должно быть > 0, задано {v}"));
    }
    Ok(())
}

/// Следит за папкой пресетов. Создаётся один раз в главном потоке, `poll()` дёшев — можно каждый кадр.
pub struct PresetWatcher {
    // Держим watcher живым: при drop слежение прекращается.
    _watcher: Option<notify::RecommendedWatcher>,
    rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
}

impl PresetWatcher {
    /// Папку создаём заранее: так за ней можно следить, и пользователю видно, куда класть свои пресеты.
    pub fn new() -> Self {
        let dir = presets_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("пресеты: не удалось создать {}: {e}", dir.display());
        }
        Self::watch(&dir)
    }

    fn watch(dir: &Path) -> Self {
        use notify::Watcher;
        let (tx, rx) = mpsc::channel();
        let res = notify::recommended_watcher(tx).and_then(|mut w| {
            w.watch(dir, notify::RecursiveMode::NonRecursive)?;
            Ok(w)
        });
        match res {
            Ok(w) => Self { _watcher: Some(w), rx: Some(rx) },
            Err(e) => {
                eprintln!("пресеты: hot-reload выключен, не удалось следить за {}: {e}", dir.display());
                Self { _watcher: None, rx: None }
            }
        }
    }

    /// `true`, если с прошлого вызова какой-то `*.toml` создан или изменён. Не блокирует.
    pub fn poll(&self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let mut changed = false;
        while let Ok(ev) = rx.try_recv() {
            let Ok(ev) = ev else { continue };
            if matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_))
                && ev.paths.iter().any(|p| is_toml(p))
            {
                changed = true;
            }
        }
        changed
    }
}

impl Default for PresetWatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Пустая временная папка для пользовательских пресетов.
    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("spider_presets_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn builtins_load_without_files() {
        let dir = Path::new("/nonexistent/spider/presets");
        let names = list_presets_in(dir);
        assert_eq!(names, ["blue3", "harvestman", "octopus", "spider", "tentacle"]);
        for n in &names {
            let p = load_preset_in(dir, n).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(&p.name, n);
        }
    }

    #[test]
    fn spider_builtin_equals_defaults() {
        let p = load_preset_in(Path::new("/nonexistent"), "spider").unwrap();
        let d = Preset::default();
        // Сравниваем через TOML, чтобы не требовать PartialEq от структур.
        assert_eq!(toml::to_string(&p).unwrap(), toml::to_string(&d).unwrap());
    }

    #[test]
    fn user_files_add_and_override() {
        let dir = temp_dir("override");
        std::fs::write(dir.join("spider.toml"), "color_leg = \"#ff0000\"\n").unwrap();
        std::fs::write(dir.join("mine.toml"), "[legs]\nstyle = \"dotted\"\n").unwrap();
        let names = list_presets_in(&dir);
        assert!(names.contains(&"mine".to_string()) && names.iter().filter(|n| *n == "spider").count() == 1, "{names:?}");
        assert_eq!(load_preset_in(&dir, "spider").unwrap().color_leg, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(load_preset_in(&dir, "mine").unwrap().legs.style, LegStyle::Dotted);
        // Встроенный без файла — как был.
        assert_eq!(load_preset_in(&dir, "octopus").unwrap().legs.style, LegStyle::Dotted);
        // Битый пользовательский файл — ошибка с путём, встроенный не подменяет молча.
        std::fs::write(dir.join("tentacle.toml"), "[legs]\nsegments = [1.0]\n").unwrap();
        assert!(load_preset_in(&dir, "tentacle").unwrap_err().contains("tentacle.toml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn partial_keeps_defaults() {
        let p = parse_preset("color_leg = \"#ff000080\"\n[legs]\nsegments = [10.0, 20.0, 30.0]\nstyle = \"dotted\"\n").unwrap();
        let d = Preset::default();
        assert_eq!(p.color_leg, [1.0, 0.0, 0.0, 128.0 / 255.0]);
        assert_eq!(p.color_body, d.color_body);
        assert_eq!(p.legs.segments, vec![10.0, 20.0, 30.0]);
        assert_eq!(p.legs.style, LegStyle::Dotted);
        assert_eq!(p.legs.rest_dist, d.legs.rest_dist);
        assert_eq!(p.legs.spread_deg, d.legs.spread_deg);
        assert_eq!(p.body.max_speed, d.body.max_speed);
    }

    #[test]
    fn errors() {
        let e = parse_preset("color_leg = \"#12345\"").unwrap_err();
        assert!(e.contains("#12345"), "{e}");
        let e = parse_preset("[legs]\nstep_treshold = 3.0").unwrap_err();
        assert!(e.contains("step_treshold"), "{e}");
        let e = parse_preset("[legs]\nstyle = \"wavy\"").unwrap_err();
        assert!(e.contains("wavy"), "{e}");
        assert!(parse_preset("[legs]\nsegments = [10.0]").is_err());
        assert!(parse_preset("[legs]\nspread_deg = [10.0]").is_err());
        assert!(parse_preset("[legs]\nsegments = [10.0, 0.0]").is_err());
        assert!(load_preset_in(Path::new("/nonexistent"), "no_such_preset").unwrap_err().contains("no_such_preset"));
    }

    #[test]
    fn color_roundtrip() {
        for s in ["#4a4aff", "#8080a059", "#ffffff"] {
            assert_eq!(color::format(&color::parse(s).unwrap()), s);
        }
    }
}
