//! Параметры существа и пресеты.
//!
//! Контракт загрузки (реализует агент конфига; сейчас заглушки):
//! - `presets_dir()`: папка с `*.toml` (по умолчанию `./presets`, рядом с бинарником в `.app`);
//! - `list_presets()`: имена пресетов (имя файла без `.toml`), отсортированы;
//! - `load_preset(name)`: парсит файл; отсутствующие поля берутся из `Preset::default()`;
//! - `PresetWatcher::poll()`: `true`, если с прошлого вызова какой-то `*.toml` изменился.
//!
//! Папка ищется так: `$SPIDER_PRESETS` → `<exe>/../Resources/presets` (внутри `.app`) → `./presets`.
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

pub fn presets_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SPIDER_PRESETS") {
        return PathBuf::from(dir);
    }
    // Внутри .app: Contents/MacOS/<exe> → Contents/Resources/presets.
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        let bundled = exe_dir.join("../Resources/presets");
        if bundled.is_dir() {
            return bundled;
        }
    }
    PathBuf::from("presets")
}

pub fn list_presets() -> Vec<String> {
    list_presets_in(&presets_dir())
}

fn list_presets_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec!["spider".into()];
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_toml(p) && p.is_file())
        .filter_map(|p| p.file_stem()?.to_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

fn is_toml(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "toml")
}

pub fn load_preset(name: &str) -> Result<Preset, String> {
    load_preset_in(&presets_dir(), name)
}

fn load_preset_in(dir: &Path, name: &str) -> Result<Preset, String> {
    let path = dir.join(format!("{name}.toml"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("пресет {}: не удалось прочитать: {e}", path.display()))?;
    let mut preset = parse_preset(&text).map_err(|e| format!("пресет {}: {e}", path.display()))?;
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
    pub fn new() -> Self {
        Self::watch(&presets_dir())
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

    fn repo_presets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("presets")
    }

    #[test]
    fn all_repo_presets_load() {
        let dir = repo_presets();
        let names = list_presets_in(&dir);
        assert!(names.len() >= 5, "{names:?}");
        for n in &names {
            let p = load_preset_in(&dir, n).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(&p.name, n);
        }
    }

    #[test]
    fn spider_toml_equals_defaults() {
        let p = load_preset_in(&repo_presets(), "spider").unwrap();
        let d = Preset::default();
        // Сравниваем через TOML, чтобы не требовать PartialEq от структур.
        assert_eq!(toml::to_string(&p).unwrap(), toml::to_string(&d).unwrap());
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
        assert!(load_preset_in(&repo_presets(), "no_such_preset").unwrap_err().contains("no_such_preset.toml"));
    }

    #[test]
    fn missing_dir_lists_default() {
        assert_eq!(list_presets_in(Path::new("/nonexistent/spider/presets")), vec!["spider".to_string()]);
    }

    #[test]
    fn color_roundtrip() {
        for s in ["#4a4aff", "#8080a059", "#ffffff"] {
            assert_eq!(color::format(&color::parse(s).unwrap()), s);
        }
    }
}
