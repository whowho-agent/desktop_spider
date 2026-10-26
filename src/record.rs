//! Запись демо-роликов без окна: `desktop_spider --record <spider|game|whip> <out.rgba>`.
//! Сценарий задаёт курсор и кнопки по времени (и может смотреть, где паук), мир — тот же `World`,
//! что в приложении. Кадры пишутся сырыми RGBA (straight alpha) подряд; ffmpeg кладёт их на фон
//! и собирает GIF (`scripts/gifs.sh`).

use std::io::Write;

use glam::Vec2;

use crate::config::{self, Preset};
use crate::input::WhipInput;
use crate::render::{DrawList, Renderer};
use crate::settings::{Mode, WhipSkin};
use crate::world::World;

/// Кадр в логических точках; рендер ×SCALE пикселей.
pub const W: f32 = 1000.0;
pub const H: f32 = 625.0;
pub const SCALE: f64 = 1.6;
pub const FPS: u32 = 20;
const STEP: f32 = 1.0 / 120.0;

/// Прямоугольник условного окна на фоне режима «Просто плеть» (x, y, w, h в pt).
/// Запись печатает его в пикселях — scripts/gifs.sh рисует окно ровно там.
pub const APP_RECT: (f32, f32, f32, f32) = (537.5, 112.5, 375.0, 350.0);

pub fn run(args: &[String]) -> Result<(), String> {
    let [mode, out] = args else {
        return Err("использование: desktop_spider --record <spider|game|whip> <out.rgba>".into());
    };
    let preset = config::load_preset("spider").unwrap_or_else(|_| Preset::default());
    let mut sc = Scenario::new(mode)?;
    let start = sc.cursor(0.0, None);
    let mut world = World::new(&preset, sc.mode, sc.skin, 1.0, start);
    world.set_bounds(Vec2::new(W, H));
    if sc.mode != Mode::Whip {
        // Паук стартует в стороне от курсора.
        world.creature = crate::sim::Creature::new(&preset, sc.spider_start);
        world.creature.set_game(sc.mode == Mode::Game);
    }

    let (pw, ph) = ((W as f64 * SCALE) as u32, (H as f64 * SCALE) as u32);
    let mut renderer = Renderer::headless(pw, ph, SCALE);
    let mut file = std::fs::File::create(out).map_err(|e| format!("{out}: {e}"))?;
    let mut list = DrawList::default();

    let frames = (sc.secs * FPS as f32) as u32;
    let steps_per_frame = (1.0 / FPS as f32 / STEP).round() as u32;
    let mut t = 0.0f32;
    let mut cursor = start;
    let mut input = WhipInput::default();
    let mut prev_spider = world.creature.position();
    for _ in 0..frames {
        for _ in 0..steps_per_frame {
            let spider = (sc.mode == Mode::Game).then(|| world.creature.position());
            // Упреждение: где паук будет к щелчку (≈ 0,28 с).
            let lead = spider.map(|p| p + (p - prev_spider) / STEP * 0.4);
            prev_spider = world.creature.position();
            cursor = sc.cursor(t, spider).clamp(Vec2::splat(20.0), Vec2::new(W - 20.0, H - 20.0));
            let next = sc.input(t);
            // Прицел на цель в момент клика ЛКМ / начала раскрутки (см. Whip::aim_at).
            if (next.lmb && !input.lmb) || (next.rmb && !input.rmb) {
                if let Some(goal) = lead.or_else(|| sc.aim_for(t)) {
                    world.whip.aim_at(cursor, goal);
                }
            }
            if next.held && !input.held {
                world.whip.reset(cursor);
            }
            input = next;
            let before = (world.hits, world.strikes);
            world.step(STEP, cursor, input);
            if std::env::var_os("RECORD_DEBUG").is_some() && (world.hits, world.strikes) != before {
                let d = world.creature.position().distance(cursor);
                eprintln!(
                    "t={t:.2} счёт {}/{} курсор {cursor:.0} паук {:.0} ({d:.0} pt), щелчок {:?}",
                    world.hits, world.strikes, world.creature.position(), world.last_crack
                );
            }
            t += STEP;
        }
        list.clear();
        world.draw(&mut list, &preset, cursor, input.held, false);
        if !input.held {
            draw_arrow(&mut list, cursor);
        }
        let rgba = renderer.render_to_rgba(&list);
        file.write_all(&rgba).map_err(|e| e.to_string())?;
    }
    let s = SCALE as f32;
    let (ax, ay, aw, ah) = APP_RECT;
    println!(
        "{pw} {ph} {FPS} {frames} {} {} {} {} hits={} strikes={}",
        (ax * s) as u32, (ay * s) as u32, (aw * s) as u32, (ah * s) as u32, world.hits, world.strikes
    );
    Ok(())
}

/// Системный курсор-стрелка (его нет в кадрах без окна): белая с тёмной обводкой.
fn draw_arrow(list: &mut DrawList, tip: Vec2) {
    let pts = [(0.0, 0.0), (0.0, 16.0), (4.0, 12.5), (7.0, 19.0), (9.5, 18.0), (6.5, 11.5), (11.5, 11.5)];
    let p: Vec<Vec2> = pts.iter().map(|&(x, y)| tip + Vec2::new(x, y)).collect();
    for (w, c) in [(3.2, [0.0, 0.0, 0.0, 0.9]), (1.4, [1.0, 1.0, 1.0, 1.0])] {
        for i in 0..p.len() {
            list.segment(p[i], p[(i + 1) % p.len()], w, c);
        }
    }
    // Заливка внутренности тонкими линиями от острия.
    for i in 1..p.len() - 1 {
        list.segment(tip + Vec2::new(1.2, 2.5), p[i].lerp(tip, 0.25), 2.2, [1.0, 1.0, 1.0, 1.0]);
    }
}

/// Плавный переход между точками маршрута с паузами.
struct Path {
    /// (момент начала движения, длительность, из, в)
    legs: Vec<(f32, f32, Vec2, Vec2)>,
}

impl Path {
    fn new(start: Vec2, moves: &[(f32, f32, (f32, f32))]) -> Self {
        let mut legs = Vec::new();
        let mut from = start;
        for &(t0, dur, (x, y)) in moves {
            let to = Vec2::new(x, y);
            legs.push((t0, dur, from, to));
            from = to;
        }
        Self { legs }
    }

    fn at(&self, t: f32) -> Vec2 {
        let mut p = self.legs.first().map(|l| l.2).unwrap_or(Vec2::ZERO);
        for &(t0, dur, from, to) in &self.legs {
            if t < t0 {
                break;
            }
            let u = ((t - t0) / dur).clamp(0.0, 1.0);
            p = from.lerp(to, u * u * (3.0 - 2.0 * u));
        }
        p
    }
}

/// Удар по сценарию: подкрасться к паузе, коротким рывком задать направление, нажать.
#[derive(Clone, Copy)]
enum Act {
    /// ЛКМ в момент t; рывок 0,12 с перед ним в сторону цели.
    Strike { t: f32 },
    /// ПКМ держать с t0 до t1, рывок перед отпусканием.
    Lasso { t0: f32, t1: f32 },
}

struct Scenario {
    mode: Mode,
    skin: WhipSkin,
    secs: f32,
    spider_start: Vec2,
    path: Path,
    /// Плеть зажата на этом интервале.
    held: (f32, f32),
    acts: Vec<Act>,
    /// Куда целиться, если паука нет (режим «Просто плеть»): по очереди для каждого действия.
    aims: Vec<Vec2>,
    /// Смещение курсора от маршрута, накопленное подходами и рывками.
    flick: Vec2,
}

const FLICK: f32 = 0.12;
const FLICK_LEN: f32 = 55.0;
/// Подход к пауку перед ударом: окно и целевая дистанция.
const APPROACH: f32 = 0.7;
const STRIKE_DIST: f32 = 170.0;
/// Сколько рука ещё идёт за пауком после клика.
const FOLLOW: f32 = 0.35;
const CRACK_DIST: f32 = 205.0;

impl Scenario {
    fn new(mode: &str) -> Result<Self, String> {
        Ok(match mode {
            "spider" => Self {
                mode: Mode::Spider,
                skin: WhipSkin::Light,
                secs: 9.0,
                spider_start: Vec2::new(750.0, 475.0),
                path: Path::new(
                    Vec2::new(250.0, 275.0),
                    &[(0.8, 1.3, (750.0, 162.5)), (3.0, 1.1, (775.0, 475.0)), (5.0, 1.6, (237.5, 462.5)), (7.4, 0.9, (450.0, 275.0))],
                ),
                held: (f32::MAX, f32::MAX),
                acts: vec![],
                aims: vec![],
                flick: Vec2::ZERO,
            },
            "game" => Self {
                mode: Mode::Game,
                skin: WhipSkin::Light,
                secs: 11.0,
                spider_start: Vec2::new(700.0, 325.0),
                path: Path::new(
                    Vec2::new(200.0, 325.0),
                    &[(0.5, 1.4, (325.0, 312.5)), (3.4, 1.2, (375.0, 212.5)), (6.2, 1.0, (350.0, 400.0))],
                ),
                held: (0.4, 10.5),
                acts: vec![Act::Strike { t: 2.2 }, Act::Strike { t: 5.0 }, Act::Lasso { t0: 7.4, t1: 8.7 }],
                aims: vec![],
                flick: Vec2::ZERO,
            },
            "whip" => Self {
                mode: Mode::Whip,
                skin: WhipSkin::Black,
                secs: 9.0,
                spider_start: Vec2::ZERO,
                path: Path::new(Vec2::new(325.0, 412.5), &[(3.6, 0.9, (337.5, 350.0))]),
                held: (0.4, 8.6),
                acts: vec![Act::Strike { t: 1.4 }, Act::Strike { t: 2.8 }, Act::Lasso { t0: 4.8, t1: 6.2 }, Act::Strike { t: 7.4 }],
                aims: vec![Vec2::new(675.0, 375.0), Vec2::new(750.0, 412.5), Vec2::new(700.0, 375.0), Vec2::new(650.0, 325.0)],
                flick: Vec2::ZERO,
            },
            other => return Err(format!("неизвестный режим {other:?}: spider | game | whip")),
        })
    }

    fn act_time(a: &Act) -> f32 {
        match *a {
            Act::Strike { t } => t,
            Act::Lasso { t1, .. } => t1,
        }
    }

    /// Курсор = маршрут + подход перед ударами. С пауком: подкрасться до ≈ 215 pt (700 pt/с — медленнее
    /// порога уворота 900) и в последние 0,12 с вести курсор прямо на него, чтобы прицел смотрел туда.
    /// Без паука: короткий рывок к точке на окне.
    fn cursor(&mut self, t: f32, spider: Option<Vec2>) -> Vec2 {
        let base = self.path.at(t);
        for (i, a) in self.acts.iter().enumerate() {
            let at = Self::act_time(a);
            match spider {
                // После клика рука продолжает идти на паука, пока плеть разворачивается.
                Some(sp) if t >= at - APPROACH && t < at + if matches!(a, Act::Strike { .. }) { FOLLOW } else { 0.0 } => {
                    let d = sp - (base + self.flick);
                    let speed = if t >= at {
                        // Щелчок ляжет ≈ 200 pt впереди руки — держим паука на этой дистанции.
                        if d.length() > CRACK_DIST { 700.0 } else { 0.0 }
                    } else if d.length() > STRIKE_DIST {
                        700.0
                    } else if t >= at - FLICK {
                        500.0
                    } else {
                        0.0
                    };
                    self.flick += d.normalize_or(Vec2::X) * speed * STEP;
                }
                None if t >= at - FLICK && t < at => {
                    let goal = self.aims.get(i).copied().unwrap_or(base + Vec2::X);
                    let d = goal - (base + self.flick);
                    self.flick += d.normalize_or(Vec2::X) * (FLICK_LEN / FLICK) * STEP;
                }
                _ => {}
            }
        }
        base + self.flick
    }

    /// Точка прицела без паука: для ближайшего действия.
    fn aim_for(&self, t: f32) -> Option<Vec2> {
        self.acts
            .iter()
            .zip(&self.aims)
            .find(|(a, _)| match **a {
                Act::Strike { t: at } => (t - at).abs() < 0.2,
                Act::Lasso { t0, .. } => (t - t0).abs() < 0.2,
            })
            .map(|(_, p)| *p)
    }

    fn input(&self, t: f32) -> WhipInput {
        let held = t >= self.held.0 && t < self.held.1;
        let mut lmb = false;
        let mut rmb = false;
        for a in &self.acts {
            match *a {
                Act::Strike { t: at } => lmb |= t >= at && t < at + 0.08,
                Act::Lasso { t0, t1 } => rmb |= t >= t0 && t < t1,
            }
        }
        WhipInput { held, lmb: held && lmb, rmb: held && rmb }
    }
}
