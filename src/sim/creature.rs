use glam::Vec2;

use super::body::Body;
use super::draw::draw_leg;
use super::gait::{leg_index, plan_steps};
use super::leg::Leg;
use crate::config::Preset;
use crate::render::DrawList;

pub struct Creature {
    preset: Preset,
    body: Body,
    legs: Vec<Leg>,
    /// Буфер порядка шагов для `plan_steps` (без аллокаций в тике).
    order: Vec<usize>,
}

impl Creature {
    pub fn new(preset: &Preset, pos: Vec2) -> Self {
        Self::with_body(preset, Body::new(pos))
    }

    fn with_body(preset: &Preset, body: Body) -> Self {
        let pairs = preset.legs.spread_deg.len().min(preset.legs.hip_along.len());
        let mut legs = Vec::with_capacity(pairs * 2);
        for pair in 0..pairs {
            for side in [-1.0, 1.0] {
                legs.push(Leg::new(pair, side, &body, &preset.body, &preset.legs));
                debug_assert_eq!(legs.len() - 1, leg_index(pair, side));
            }
        }
        let order = Vec::with_capacity(legs.len());
        Self { preset: preset.clone(), body, legs, order }
    }

    /// Смена пресета на лету: тело (позиция, курс, скорость) сохраняется, ноги пересобираются.
    pub fn set_preset(&mut self, preset: &Preset) {
        let b = &self.body;
        let body = Body { pos: b.pos, vel: b.vel, heading: b.heading };
        *self = Self::with_body(preset, body);
    }

    // --- Игра с плетью (реализует агент поведения, W2e; сейчас заглушки) ---

    /// Центр тела.
    pub fn position(&self) -> Vec2 {
        self.body.pos
    }

    /// Режим «Игра»: держать 250–400 pt от курсора и уворачиваться вместо погони.
    pub fn set_game(&mut self, _on: bool) {}

    /// Щелчок плети в `pos`. Если близко (≈ 42 pt, `big` — 55) — оглушение 0,45 с, затем бегство. Возвращает попадание.
    pub fn strike(&mut self, _pos: Vec2, _big: bool) -> bool {
        false
    }

    /// Пойман захлёстом: ноги поджаты, тело ведёт `hold_at`.
    pub fn grab(&mut self) {}

    /// Пока пойман — тело ставится в `pos` каждый шаг.
    pub fn hold_at(&mut self, _pos: Vec2) {}

    /// Отпущен: удирает от `from` (курсор).
    pub fn release(&mut self, _from: Vec2) {}

    /// Фиксированный шаг симуляции. `target` — позиция курсора.
    pub fn update(&mut self, dt: f32, target: Vec2) {
        let lp = &self.preset.legs;
        self.body.update(dt, target, &self.preset.body);
        plan_steps(&mut self.legs, &self.body, lp, &mut self.order);
        for leg in &mut self.legs {
            leg.update(dt, &self.body, lp);
        }
    }

    /// Добавляет фигуры в DrawList. `debug` — рисовать цели ступней.
    pub fn draw(&self, list: &mut DrawList, debug: bool) {
        let p = &self.preset;
        let b = &self.body;

        if debug {
            for leg in &self.legs {
                list.ring(leg.ideal, 4.0, 1.0, p.color_debug);
            }
        }

        for leg in &self.legs {
            draw_leg(list, &leg.joints, p);
        }

        // Корпус: повёрнутый прямоугольник и «голова» спереди.
        let (hl, hw) = (p.body.half_len, p.body.half_width);
        let c = [
            Vec2::new(hl, -hw),
            Vec2::new(hl, hw),
            Vec2::new(-hl, hw),
            Vec2::new(-hl, -hw),
        ]
        .map(|v| b.to_world(v));
        for i in 0..4 {
            list.segment(c[i], c[(i + 1) % 4], p.line_width, p.color_body);
        }
        list.dot(b.to_world(Vec2::new(hl * 0.6, 0.0)), hw * 0.45, p.color_head);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LegStyle;

    fn run_chase(preset: &Preset) {
        let mut c = Creature::new(preset, Vec2::ZERO);
        let target = Vec2::new(600.0, 300.0);
        let n = preset.legs.segments.len();
        let total: f32 = preset.legs.segments.iter().sum();
        for _ in 0..(120 * 4) {
            c.update(1.0 / 120.0, target);
            for l in &c.legs {
                assert_eq!(l.joints.len(), n + 1);
                assert!(l.foot.is_finite() && l.joints.iter().all(|j| j.is_finite()));
                // Длины сегментов сохраняются (для 2 сегментов колено сдвигает подъём).
                if n >= 3 {
                    for (w, len) in l.joints.windows(2).zip(&preset.legs.segments) {
                        assert!((w[0].distance(w[1]) - len).abs() < 1e-2);
                    }
                }
                assert!(l.joints[n].distance(l.joints[0]) <= total + 1e-2);
                // Длинные цепочки — гладкие кривые без изломов.
                if n >= 8 {
                    for w in l.joints.windows(3) {
                        let turn = (w[1] - w[0]).angle_to(w[2] - w[1]).abs();
                        assert!(turn < 1.0, "{}: kink {turn}", preset.name);
                    }
                }
            }
        }
        let d = c.body.pos.distance(target);
        let bp = &preset.body;
        assert!(d > bp.stop_radius - 5.0 && d < bp.slow_radius, "{}: d = {d}", preset.name);
        assert!(c.body.vel.length() < 15.0);
        let mut list = DrawList::default();
        c.draw(&mut list, true);
        assert!(!list.shapes.is_empty());
    }

    #[test]
    fn chases_and_stops_at_ring() {
        run_chase(&Preset::default());
    }

    fn multi_segment(segments: Vec<f32>, style: LegStyle, pairs: usize) -> Preset {
        let mut p = Preset::default();
        let total: f32 = segments.iter().sum();
        p.name = format!("{}x{:?}", segments.len(), style);
        p.legs.spread_deg = (0..pairs).map(|i| 30.0 + 130.0 * i as f32 / (pairs - 1) as f32).collect();
        p.legs.hip_along = (0..pairs).map(|i| 0.6 - 1.2 * i as f32 / (pairs - 1) as f32).collect();
        p.legs.rest_dist = total * 0.7;
        p.legs.step_threshold = total * 0.3;
        p.legs.segments = segments;
        p.legs.style = style;
        p
    }

    #[test]
    fn all_styles_and_chain_lengths() {
        for style in [LegStyle::Straight, LegStyle::Curve, LegStyle::Dotted] {
            run_chase(&multi_segment(vec![30.0, 32.0, 28.0], style, 4));
            run_chase(&multi_segment(vec![7.0; 14], style, 3));
            run_chase(&multi_segment(vec![36.0, 38.0], style, 6));
        }
    }

    #[test]
    fn set_preset_keeps_body() {
        let mut c = Creature::new(&Preset::default(), Vec2::ZERO);
        for _ in 0..30 {
            c.update(1.0 / 120.0, Vec2::new(500.0, 0.0));
        }
        let (pos, vel, heading) = (c.body.pos, c.body.vel, c.body.heading);
        c.set_preset(&multi_segment(vec![7.0; 14], LegStyle::Curve, 3));
        assert_eq!((c.body.pos, c.body.vel, c.body.heading), (pos, vel, heading));
        assert_eq!(c.legs.len(), 6);
    }
}
