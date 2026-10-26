//! Эффекты: вспышка щелчка и следы ударов. Реализует агент отрисовки (W2b).

use glam::Vec2;

use crate::render::DrawList;

/// Длительность вспышки щелчка, с.
pub const CRACK_LIFE: f32 = 0.4;
/// Жизнь следа, с; последнюю секунду он гаснет.
pub const WELT_LIFE: f32 = 2.5;
const WELT_FADE: f32 = 1.0;
/// Больше следов не держим — старые уходят.
const MAX_WELTS: usize = 48;

const FLASH: u32 = 0xfff4c2;
const RING: u32 = 0xffe9a8;
const RAYS: u32 = 0xffd36b;
const WELT: u32 = 0xff4d6d;
const WELT_HI: u32 = 0xffd0d8;

struct Crack {
    pos: Vec2,
    big: bool,
    /// Масштаб плети на момент щелчка.
    s: f32,
    t: f32,
    rot: f32,
}

struct Welt {
    pos: Vec2,
    dir: Vec2,
    big: bool,
    s: f32,
    t: f32,
}

/// Живые эффекты на оверлее. Время жизни: вспышка ≈ 0,4 с, след 2,5 с (последняя секунда — затухание).
pub struct Fx {
    cracks: Vec<Crack>,
    welts: Vec<Welt>,
    rng: u32,
    /// Масштаб новых эффектов (= размер плети).
    scale: f32,
}

impl Default for Fx {
    fn default() -> Self {
        Self { cracks: Vec::new(), welts: Vec::new(), rng: 0x9e37_79b9, scale: 1.0 }
    }
}

impl Fx {
    /// Вспышка, 8 лучей, расходящееся кольцо. `big` — усиленный (захлёст): крупнее в 1,6 раза. Без текста.
    pub fn crack(&mut self, pos: Vec2, big: bool) {
        let rot = self.rand() * std::f32::consts::PI;
        self.cracks.push(Crack { pos, big, s: self.scale, t: 0.0, rot });
    }

    /// Масштаб для новых вспышек и следов; уже живые эффекты не меняются.
    pub fn set_scale(&mut self, s: f32) {
        self.scale = s;
    }

    /// Красный след вдоль `dir` (направление кончика хлыста) — только в режиме «Просто плеть».
    pub fn welt(&mut self, pos: Vec2, dir: Vec2, big: bool) {
        if self.welts.len() >= MAX_WELTS {
            self.welts.remove(0);
        }
        self.welts.push(Welt { pos, dir: dir.normalize_or(Vec2::X), big, s: self.scale, t: 0.0 });
    }

    pub fn update(&mut self, dt: f32) {
        for c in &mut self.cracks {
            c.t += dt;
        }
        for w in &mut self.welts {
            w.t += dt;
        }
        self.cracks.retain(|c| c.t < CRACK_LIFE);
        self.welts.retain(|w| w.t < WELT_LIFE);
    }

    /// Есть ли живые эффекты (нужна ли перерисовка).
    pub fn is_active(&self) -> bool {
        !self.cracks.is_empty() || !self.welts.is_empty()
    }

    pub fn clear(&mut self) {
        self.cracks.clear();
        self.welts.clear();
    }

    /// Рисует следы, затем вспышки (вспышки поверх). В приложении слои кладутся раздельно, см. `Whip::draw`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn draw(&self, list: &mut DrawList) {
        self.draw_welts(list);
        self.draw_cracks(list);
    }

    /// Только следы — если их нужно положить под плеть отдельным слоем.
    pub fn draw_welts(&self, list: &mut DrawList) {
        for w in &self.welts {
            let op = if w.t < WELT_LIFE - WELT_FADE { 1.0 } else { (WELT_LIFE - w.t) / WELT_FADE };
            if op <= 0.0 {
                continue;
            }
            let l = if w.big { 26.0 } else { 18.0 } * w.s;
            let d = w.dir * l;
            // Контрольная точка квадратичной кривой — сбоку от центра (лёгкий изгиб).
            let ctrl = w.pos + Vec2::new(-d.y, d.x) * 0.25;
            quad(list, w.pos - d, ctrl, w.pos + d, if w.big { 5.0 } else { 3.5 } * w.s, rgb(WELT, 0.85 * op));
            quad(list, w.pos - d * 0.8, ctrl, w.pos + d * 0.8, w.s, rgb(WELT_HI, op));
        }
    }

    /// Только вспышки щелчков.
    pub fn draw_cracks(&self, list: &mut DrawList) {
        for c in &self.cracks {
            let k = if c.big { 1.6 } else { 1.0 } * c.s;
            let u = (c.t / CRACK_LIFE).clamp(0.0, 1.0);
            // Вспышка: растёт и гаснет за первую треть.
            let fo = (1.0 - u * 3.0).max(0.0);
            if fo > 0.0 {
                let r = 3.0 * c.s + 12.0 * k * ease_out((u * 3.0).min(1.0));
                list.dot(c.pos, r, rgb(FLASH, fo));
            }
            let op = 1.0 - u;
            // Расходящееся кольцо.
            list.ring(c.pos, 4.0 * c.s + 34.0 * k * ease_out(u), 1.5 * c.s, rgb(RING, op));
            // 8 лучей: отходят от центра и укорачиваются.
            let r0 = 6.0 * c.s + 10.0 * k * u;
            let r1 = r0 + 10.0 * k * (1.0 - u);
            let w = 1.6 * c.s;
            if r1 - r0 > 0.05 {
                for j in 0..8 {
                    let a = Vec2::from_angle(c.rot + j as f32 * std::f32::consts::FRAC_PI_4);
                    // Капсула выходит за концы на W/2 — сдвигаем внутрь, если есть куда.
                    let pad = ((r1 - r0) * 0.5).min(w * 0.5);
                    list.segment(c.pos + a * (r0 + pad), c.pos + a * (r1 - pad), w, rgb(RAYS, op));
                }
            }
        }
    }

    /// Детерминированный xorshift32 → [0, 1).
    fn rand(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Квадратичная кривая ломаной из капсул. Мало звеньев: на стыках полупрозрачные капсулы
/// перекрываются и темнеют, а кривизна тут слабая.
fn quad(list: &mut DrawList, p0: Vec2, c: Vec2, p2: Vec2, width: f32, color: [f32; 4]) {
    const STEPS: usize = 5;
    let at = |t: f32| p0 * (1.0 - t) * (1.0 - t) + c * 2.0 * (1.0 - t) * t + p2 * t * t;
    let mut prev = p0;
    for i in 1..=STEPS {
        let p = at(i as f32 / STEPS as f32);
        list.segment(prev, p, width, color);
        prev = p;
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn rgb(hex: u32, a: f32) -> [f32; 4] {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        a.clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crack_lives_04s() {
        let mut fx = Fx::default();
        assert!(!fx.is_active());
        fx.crack(Vec2::new(10.0, 10.0), false);
        assert!(fx.is_active());
        fx.update(0.39);
        assert!(fx.is_active());
        let mut l = DrawList::default();
        fx.draw(&mut l);
        assert!(!l.shapes.is_empty());
        fx.update(0.02);
        assert!(!fx.is_active());
        let mut l = DrawList::default();
        fx.draw(&mut l);
        assert!(l.shapes.is_empty());
    }

    #[test]
    fn welt_lives_25s_and_fades() {
        let mut fx = Fx::default();
        fx.welt(Vec2::ZERO, Vec2::new(0.0, 0.0), true);
        fx.update(1.0);
        let mut l = DrawList::default();
        fx.draw(&mut l);
        let a_early = l.shapes[0].color[3];
        fx.update(1.0);
        let mut l = DrawList::default();
        fx.draw(&mut l);
        assert!(l.shapes[0].color[3] < a_early);
        assert!(l.shapes.iter().all(|s| s.a.iter().chain(&s.b).all(|v| v.is_finite())));
        fx.update(0.51);
        assert!(!fx.is_active());
    }

    #[test]
    fn clear_and_rng() {
        let mut fx = Fx::default();
        for _ in 0..100 {
            let r = fx.rand();
            assert!((0.0..1.0).contains(&r));
        }
        fx.crack(Vec2::ZERO, true);
        fx.welt(Vec2::ZERO, Vec2::X, false);
        fx.clear();
        assert!(!fx.is_active());
    }
}
