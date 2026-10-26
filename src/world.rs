//! Мир без окна и платформы: паук, плеть, режим и счёт. Общий для приложения (`app.rs`)
//! и записи гифок (`record.rs`), поэтому поведение в них совпадает.

use glam::Vec2;

use crate::config::Preset;
use crate::input::WhipInput;
use crate::render::DrawList;
use crate::settings::{Leather, LightSide, Mode};
use crate::whip::skin::{leather_palette, LeatherPalette};
use crate::sim::Creature;
use crate::whip::{Whip, WhipEvent};

pub struct World {
    pub creature: Creature,
    pub whip: Whip,
    mode: Mode,
    /// Вид плети: палитра кожи и сторона света.
    pal: LeatherPalette,
    pub light: LightSide,
    events: Vec<WhipEvent>,
    /// Счёт текущей сессии режима: попадания и удары.
    pub hits: u32,
    pub strikes: u32,
    /// Последний щелчок (для отладки записи).
    pub last_crack: Option<Vec2>,
    /// Край экрана для паука: `None` — без ограничений.
    bounds: Option<(Vec2, Vec2)>,
}

impl World {
    pub fn new(preset: &Preset, mode: Mode, leather: Leather, light: LightSide, whip_size: f32, cursor: Vec2) -> Self {
        let mut creature = Creature::new(preset, cursor);
        creature.set_game(mode == Mode::Game);
        let mut whip = Whip::new(cursor);
        whip.set_scale(whip_size, cursor);
        Self { creature, whip, mode, pal: leather_palette(leather), light, events: Vec::new(), hits: 0, strikes: 0, last_crack: None, bounds: None }
    }

    #[allow(dead_code)]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_leather(&mut self, leather: Leather) {
        self.pal = leather_palette(leather);
    }

    /// Смена режима сбрасывает счёт и эффекты.
    pub fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.creature.set_game(mode == Mode::Game);
        self.whip.fx_mut().clear();
        self.hits = 0;
        self.strikes = 0;
    }

    /// Видимая область (pt). Паук держится внутри с отступом, чтобы его ноги не уходили за край.
    pub fn set_bounds(&mut self, size: Vec2) {
        const MARGIN: f32 = 60.0;
        let m = Vec2::splat(MARGIN).min(size * 0.5);
        self.bounds = Some((m, size - m));
    }

    /// Шаг симуляции. Возвращает `true`, если изменился счёт.
    pub fn step(&mut self, dt: f32, cursor: Vec2, input: WhipInput) -> bool {
        if self.mode != Mode::Whip {
            self.creature.update(dt, cursor);
            if let Some((min, max)) = self.bounds {
                self.creature.keep_inside(min, max);
            }
        }
        if self.mode == Mode::Spider {
            return false;
        }
        let target = (self.mode == Mode::Game).then(|| self.creature.position());
        self.whip.update(dt, cursor, input, target, &mut self.events);

        let mut score_changed = false;
        for ev in self.events.drain(..) {
            match ev {
                WhipEvent::Crack { pos, dir, big } => {
                    self.strikes += 1;
                    self.last_crack = Some(pos);
                    score_changed = true;
                    match self.mode {
                        Mode::Game => {
                            if self.creature.strike(pos, big) {
                                self.hits += 1;
                            }
                        }
                        Mode::Whip => self.whip.fx_mut().welt(pos, dir, big),
                        Mode::Spider => {}
                    }
                }
                WhipEvent::Caught => {
                    self.creature.grab();
                    self.hits += 1;
                    score_changed = true;
                }
                WhipEvent::Hold { pos } => self.creature.hold_at(pos),
                WhipEvent::Released => self.creature.release(cursor),
            }
        }
        score_changed
    }

    /// Идёт ли анимация плети (удар, захват, догорающие эффекты).
    pub fn whip_animating(&self) -> bool {
        self.mode != Mode::Spider && self.whip.is_animating()
    }

    /// `whip_visible` — плеть вместо курсора (хоткей зажат); эффекты рисуются всегда.
    /// `alpha` — доля шага физики, прошедшая к моменту кадра (интерполяция плети).
    pub fn draw(&self, list: &mut DrawList, preset: &Preset, cursor: Vec2, whip_visible: bool, debug: bool, alpha: f32) {
        // Кольцо остановки вокруг курсора — только в отладке, как цели ступней.
        if debug && self.mode != Mode::Whip {
            list.ring(cursor, preset.body.stop_radius, 1.0, preset.color_ring);
        }
        if self.mode != Mode::Whip {
            self.creature.draw(list, debug);
        }
        if self.mode != Mode::Spider {
            self.whip.draw(list, &self.pal, self.light.dir(), whip_visible, alpha);
        }
    }
}
