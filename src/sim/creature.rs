use glam::Vec2;

use super::body::{smoothstep, Body};
use super::draw::{draw_leg, flash_color};
use super::gait::{leg_index, plan_steps};
use super::leg::Leg;
use crate::config::Preset;
use crate::render::DrawList;

// --- Константы игры с плетью ---
/// Радиус попадания по телу: обычный щелчок / усиленный.
const HIT_RADIUS: f32 = 42.0;
const HIT_RADIUS_BIG: f32 = 55.0;
/// Оглушение после удара и после захлёста, с.
const STUN_TIME: f32 = 0.45;
const RELEASE_STUN_TIME: f32 = 0.2;
/// Толчок тела от удара, pt/с, и его затухание, 1/с.
const KICK_SPEED: f32 = 300.0;
const KICK_DECAY: f32 = 7.0;
/// Бегство: длительность, с; скорость в долях `max_speed` (в начале → в конце).
const FLEE_TIME: f32 = 1.4;
const FLEE_SPEED: f32 = 2.2;
const FLEE_SPEED_END: f32 = 1.0;
/// Множитель ускорения на бегстве и увороте.
const BURST_ACCEL: f32 = 2.5;
/// Вспышка красным, с.
const FLASH_TIME: f32 = 0.4;
/// Дрыгание поджатых ног: частота, рад/с, и амплитуда, рад.
const WIGGLE_HELD: (f32, f32) = (30.0, 0.35);
const WIGGLE_STUN: (f32, f32) = (45.0, 0.12);
/// «Игра»: кольцо вокруг курсора, pt.
const RING_MIN: f32 = 250.0;
const RING_MAX: f32 = 400.0;
const GOAL_R: (f32, f32) = (275.0, 375.0);
/// Разброс угла новой цели от текущего пеленга, рад.
const GOAL_SPREAD: f32 = 0.9;
/// Интервал смены цели, с.
const GOAL_INTERVAL: (f32, f32) = (1.5, 3.5);
/// Крейсерская скорость в «Игре» (доля `max_speed`), когда кольцо соблюдено.
const GAME_CRUISE: f32 = 0.45;
/// Уворот: порог скорости курсора, pt/с; дальность; cos угла «летит на паука».
const DODGE_CURSOR_SPEED: f32 = 900.0;
const DODGE_RANGE: f32 = 260.0;
const DODGE_COS: f32 = 0.6;
const DODGE_TIME: f32 = 0.3;
const DODGE_SPEED: f32 = 2.0;
const DODGE_COOLDOWN: f32 = 0.45;
/// Сглаживание оценки скорости курсора, 1/с.
const CURSOR_VEL_RATE: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Погоня (Паук) или держим дистанцию (Игра).
    Normal,
    /// Оглушён: ноги поджаты, тело дёрнуто толчком; потом бегство от `from`.
    Stunned { t: f32, dur: f32, from: Vec2 },
    Flee { t: f32, from: Vec2 },
    /// Пойман захлёстом: позицию задаёт `hold_at`.
    Held,
}

pub struct Creature {
    preset: Preset,
    body: Body,
    legs: Vec<Leg>,
    /// Буфер порядка шагов для `plan_steps` (без аллокаций в тике).
    order: Vec<usize>,
    mode: Mode,
    game: bool,
    /// Цель на кольце: угол вокруг курсора, радиус, время до смены.
    goal_angle: f32,
    goal_r: f32,
    goal_timer: f32,
    /// Оценка скорости курсора по последовательным `target`.
    prev_target: Option<Vec2>,
    cursor_vel: Vec2,
    /// Уворот: оставшееся время, направление, перезарядка.
    dodge_t: f32,
    dodge_dir: Vec2,
    dodge_cd: f32,
    /// Вспышка 1 → 0.
    flash: f32,
    wiggle: f32,
    hold: Option<Vec2>,
    rng: u32,
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
        let rng = (body.pos.x.to_bits() ^ body.pos.y.to_bits().rotate_left(16)) | 1;
        Self {
            preset: preset.clone(),
            body,
            legs,
            order,
            mode: Mode::Normal,
            game: false,
            goal_angle: 0.0,
            goal_r: 0.5 * (GOAL_R.0 + GOAL_R.1),
            goal_timer: 0.0,
            prev_target: None,
            cursor_vel: Vec2::ZERO,
            dodge_t: 0.0,
            dodge_dir: Vec2::ZERO,
            dodge_cd: 0.0,
            flash: 0.0,
            wiggle: 0.0,
            hold: None,
            rng: rng ^ 0x9e37_79b9,
        }
    }

    /// Смена пресета на лету: тело (позиция, курс, скорость) сохраняется, ноги пересобираются.
    pub fn set_preset(&mut self, preset: &Preset) {
        let b = &self.body;
        let body = Body { pos: b.pos, vel: b.vel, heading: b.heading };
        let (mode, game, flash) = (self.mode, self.game, self.flash);
        *self = Self::with_body(preset, body);
        (self.mode, self.game, self.flash) = (mode, game, flash);
    }

    // --- Игра с плетью ---

    /// Центр тела.
    pub fn position(&self) -> Vec2 {
        self.body.pos
    }

    /// Режим «Игра»: держать 250–400 pt от курсора и уворачиваться вместо погони.
    pub fn set_game(&mut self, on: bool) {
        if on && !self.game {
            self.goal_timer = 0.0;
        }
        self.game = on;
        self.dodge_t = 0.0;
    }

    /// Щелчок плети в `pos`. Если близко (≈ 42 pt, `big` — 55) — оглушение 0,45 с, затем бегство. Возвращает попадание.
    pub fn strike(&mut self, pos: Vec2, big: bool) -> bool {
        let r = if big { HIT_RADIUS_BIG } else { HIT_RADIUS };
        if self.mode == Mode::Held || !pos.is_finite() || self.body.pos.distance(pos) > r {
            return false;
        }
        let away = (self.body.pos - pos).try_normalize().unwrap_or(-self.body.forward());
        self.body.vel = away * KICK_SPEED;
        self.mode = Mode::Stunned { t: 0.0, dur: STUN_TIME, from: pos };
        self.flash = 1.0;
        self.dodge_t = 0.0;
        true
    }

    /// Пойман захлёстом: ноги поджаты, тело ведёт `hold_at`.
    pub fn grab(&mut self) {
        self.mode = Mode::Held;
        self.hold = None;
        self.flash = 1.0;
        self.dodge_t = 0.0;
    }

    /// Пока пойман — тело ставится в `pos` каждый шаг.
    pub fn hold_at(&mut self, pos: Vec2) {
        if pos.is_finite() {
            self.hold = Some(pos);
        }
    }

    /// Отпущен: удирает от `from` (курсор).
    pub fn release(&mut self, from: Vec2) {
        if self.mode != Mode::Held {
            return;
        }
        let from = if from.is_finite() { from } else { self.body.pos - self.body.forward() };
        self.hold = None;
        self.mode = Mode::Stunned { t: 0.0, dur: RELEASE_STUN_TIME, from };
    }

    fn rand(&mut self) -> f32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn rand_range(&mut self, (a, b): (f32, f32)) -> f32 {
        a + (b - a) * self.rand()
    }

    /// Оценка скорости курсора (сглаженная: курсор может обновляться реже шага).
    fn track_cursor(&mut self, dt: f32, target: Vec2) {
        if let Some(prev) = self.prev_target {
            let raw = (target - prev) / dt;
            if raw.is_finite() {
                let k = 1.0 - (-CURSOR_VEL_RATE * dt).exp();
                self.cursor_vel = self.cursor_vel.lerp(raw, k);
            }
        }
        self.prev_target = Some(target);
    }

    /// Желаемая скорость в «Игре»: цель на кольце вокруг курсора, плюс уворот.
    /// Возвращает (скорость, ускорение).
    fn game_desired(&mut self, dt: f32, target: Vec2) -> (Vec2, f32) {
        let bp = &self.preset.body;
        let (max_speed, max_accel) = (bp.max_speed, bp.max_accel);
        let rel = self.body.pos - target;
        let d = rel.length();

        // Уворот: курсор быстро летит на паука — отскок вбок.
        self.dodge_cd = (self.dodge_cd - dt).max(0.0);
        let cv = self.cursor_vel;
        let cs = cv.length();
        if self.dodge_t <= 0.0 && self.dodge_cd <= 0.0 && cs > DODGE_CURSOR_SPEED && d < DODGE_RANGE {
            let toward = cv.dot(rel) / (cs * d.max(1e-3));
            if toward > DODGE_COS {
                // Вбок от линии полёта, в ту сторону, где уже стоим.
                let perp = Vec2::new(-cv.y, cv.x) / cs;
                let side = if perp.dot(rel) >= 0.0 { 1.0 } else { -1.0 };
                self.dodge_dir = perp * side;
                self.dodge_t = DODGE_TIME;
                self.dodge_cd = DODGE_TIME + DODGE_COOLDOWN;
                // После уворота — новая цель на кольце с этой стороны.
                self.goal_timer = 0.0;
            }
        }
        if self.dodge_t > 0.0 {
            self.dodge_t -= dt;
            return (self.dodge_dir * max_speed * DODGE_SPEED, max_accel * BURST_ACCEL * 1.5);
        }

        // Цель на кольце: держится рядом с текущим пеленгом, иногда меняется.
        self.goal_timer -= dt;
        if self.goal_timer <= 0.0 {
            let bearing = if d > 1.0 { rel.to_angle() } else { self.rand() * std::f32::consts::TAU };
            self.goal_angle = bearing + (self.rand() * 2.0 - 1.0) * GOAL_SPREAD;
            self.goal_r = self.rand_range(GOAL_R);
            self.goal_timer = self.rand_range(GOAL_INTERVAL);
        }
        let goal = target + Vec2::from_angle(self.goal_angle) * self.goal_r;
        let to = goal - self.body.pos;
        // Вне кольца — бегом, в кольце — прогулочным шагом.
        let urgency = 1.0 - smoothstep(RING_MIN - 60.0, RING_MIN, d) + smoothstep(RING_MAX, RING_MAX + 150.0, d);
        let speed = max_speed * (GAME_CRUISE + (1.0 - GAME_CRUISE) * urgency.min(1.0));
        let desired = to.normalize_or_zero() * speed * smoothstep(4.0, 90.0, to.length());
        (desired, max_accel)
    }

    /// Фиксированный шаг симуляции. `target` — позиция курсора.
    pub fn update(&mut self, dt: f32, target: Vec2) {
        if dt.is_nan() || dt <= 0.0 || !target.is_finite() {
            return;
        }
        self.track_cursor(dt, target);
        self.flash = (self.flash - dt / FLASH_TIME).max(0.0);
        let bp = self.preset.body.clone();

        let mut curl: Option<(f32, f32)> = None;
        match self.mode {
            Mode::Held => {
                if let Some(pos) = self.hold.take() {
                    // Скорость — из перемещения (ограничена), чтобы после отпускания не улетел.
                    let v = ((pos - self.body.pos) / dt).clamp_length_max(bp.max_speed * FLEE_SPEED);
                    self.body.vel = if v.is_finite() { v } else { Vec2::ZERO };
                    self.body.pos = pos;
                } else {
                    self.body.vel *= (-KICK_DECAY * dt).exp();
                }
                curl = Some(WIGGLE_HELD);
            }
            Mode::Stunned { t, dur, from } => {
                // Толчок затухает; тело не поворачивается.
                self.body.vel *= (-KICK_DECAY * dt).exp();
                self.body.pos += self.body.vel * dt;
                let t = t + dt;
                self.mode = if t >= dur { Mode::Flee { t: 0.0, from } } else { Mode::Stunned { t, dur, from } };
                curl = Some(WIGGLE_STUN);
            }
            Mode::Flee { t, from } => {
                let t = t + dt;
                let s = (t / FLEE_TIME).min(1.0);
                let away = (self.body.pos - from).try_normalize().unwrap_or(self.body.forward());
                let flee = away * bp.max_speed * (FLEE_SPEED + (FLEE_SPEED_END - FLEE_SPEED) * s);
                // К концу плавно возвращаемся к обычному поведению.
                let w = smoothstep(0.6, 1.0, s);
                let (normal, accel) = self.normal_desired(dt, target);
                let desired = flee.lerp(normal, w);
                let accel = bp.max_accel * BURST_ACCEL + (accel - bp.max_accel * BURST_ACCEL) * w;
                self.body.steer(dt, desired, accel, bp.turn_rate, target);
                self.mode = if t >= FLEE_TIME { Mode::Normal } else { Mode::Flee { t, from } };
            }
            Mode::Normal => {
                if self.game {
                    let (desired, accel) = self.game_desired(dt, target);
                    self.body.steer(dt, desired, accel, bp.turn_rate, target);
                } else {
                    self.body.update(dt, target, &bp);
                }
            }
        }

        let lp = &self.preset.legs;
        if let Some((rate, amp)) = curl {
            self.wiggle += rate * dt;
            let phase = self.wiggle;
            for (i, leg) in self.legs.iter_mut().enumerate() {
                let w = (phase + i as f32 * 1.7).sin() * amp;
                leg.update_curled(dt, &self.body, lp, w);
            }
        } else {
            // На бегстве шаги короче по времени, чтобы ноги успевали за телом.
            let speed = self.body.vel.length();
            let scale = if speed > bp.max_speed * 1.05 { (bp.max_speed / speed).max(0.5) } else { 1.0 };
            plan_steps(&mut self.legs, &self.body, lp, &mut self.order);
            for leg in &mut self.legs {
                leg.update_scaled(dt, &self.body, lp, scale);
            }
        }
    }

    /// Обычное поведение (погоня или кольцо в «Игре»): (желаемая скорость, ускорение).
    fn normal_desired(&mut self, dt: f32, target: Vec2) -> (Vec2, f32) {
        if self.game {
            self.game_desired(dt, target)
        } else {
            (self.body.chase_desired(target, &self.preset.body), self.preset.body.max_accel)
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

        // Вспышка попадания: резко красное, к концу — обратно в цвета пресета.
        let flash = self.flash.sqrt();
        for leg in &self.legs {
            draw_leg(list, &leg.joints, p, flash);
        }
        let color_body = flash_color(p.color_body, flash);

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
            list.segment(c[i], c[(i + 1) % 4], p.line_width, color_body);
        }
        list.dot(b.to_world(Vec2::new(hl * 0.6, 0.0)), hw * 0.45, flash_color(p.color_head, flash * 0.5));
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

    const DT: f32 = 1.0 / 120.0;

    /// Все суставы конечны, ступни в досягаемости.
    fn assert_legs_ok(c: &Creature) {
        let total: f32 = c.preset.legs.segments.iter().sum();
        assert!(c.body.pos.is_finite() && c.body.vel.is_finite());
        for l in &c.legs {
            assert!(l.foot.is_finite() && l.joints.iter().all(|j| j.is_finite()));
            assert!(l.joints.last().unwrap().distance(l.joints[0]) <= total + 1e-2);
            assert!(l.foot.distance(l.hip(&c.body)) <= total * 1.15, "stretched leg");
        }
    }

    fn run(c: &mut Creature, secs: f32, target: Vec2) {
        for _ in 0..(secs / DT) as usize {
            c.update(DT, target);
            assert_legs_ok(c);
        }
    }

    #[test]
    fn game_keeps_distance() {
        for start in [Vec2::ZERO, Vec2::new(590.0, 310.0), Vec2::new(1400.0, 900.0)] {
            let mut c = Creature::new(&Preset::default(), start);
            c.set_game(true);
            let cursor = Vec2::new(600.0, 300.0);
            run(&mut c, 3.0, cursor);
            let d = c.position().distance(cursor);
            assert!((200.0..=450.0).contains(&d), "start {start}: d = {d}");
            // Дальше тоже держит кольцо, гуляя.
            for _ in 0..4 {
                run(&mut c, 1.0, cursor);
                let d = c.position().distance(cursor);
                assert!((200.0..=450.0).contains(&d), "d = {d}");
            }
            // Выключили — снова погоня до кольца остановки.
            c.set_game(false);
            run(&mut c, 3.0, cursor);
            assert!(c.position().distance(cursor) < c.preset.body.slow_radius);
        }
    }

    #[test]
    fn strike_hits_stuns_and_flees() {
        let mut c = Creature::new(&Preset::default(), Vec2::ZERO);
        let cursor = Vec2::new(600.0, 300.0);
        run(&mut c, 2.0, cursor);
        let hit = c.position() + Vec2::new(30.0, 0.0);
        assert!(c.strike(hit, false));
        assert!(c.flash > 0.9);
        let d0 = c.position().distance(hit);
        run(&mut c, 0.3, cursor);
        assert!(matches!(c.mode, Mode::Stunned { .. }));
        // Ноги поджаты к телу.
        let rest = c.preset.legs.rest_dist;
        for l in &c.legs {
            assert!(l.foot.distance(l.hip(&c.body)) < rest * 0.7);
        }
        // Толчок от точки удара.
        assert!(c.position().distance(hit) > d0 + 10.0);
        run(&mut c, 1.2, cursor);
        assert!(matches!(c.mode, Mode::Flee { .. }));
        let d = c.position().distance(hit);
        assert!(d > d0 + 150.0, "d = {d}");
        // Потом возвращается к погоне.
        run(&mut c, 4.0, cursor);
        assert_eq!(c.mode, Mode::Normal);
        assert!(c.position().distance(cursor) < c.preset.body.slow_radius);
        assert_eq!(c.flash, 0.0);
    }

    #[test]
    fn strike_misses_outside_radius() {
        let mut c = Creature::new(&Preset::default(), Vec2::new(100.0, 100.0));
        assert!(!c.strike(Vec2::new(100.0, 145.0), false));
        assert!(!c.strike(Vec2::new(100.0, 160.0), true));
        assert_eq!(c.mode, Mode::Normal);
        assert_eq!(c.flash, 0.0);
        assert!(c.strike(Vec2::new(100.0, 150.0), true));
    }

    #[test]
    fn grab_hold_release() {
        let mut c = Creature::new(&Preset::default(), Vec2::ZERO);
        c.set_game(true);
        let cursor = Vec2::new(300.0, 0.0);
        run(&mut c, 1.0, cursor);
        c.grab();
        let p0 = c.position();
        let n = (1.0 / DT) as usize;
        for i in 0..n {
            let hold = p0 + Vec2::new(300.0 * i as f32 / n as f32, 0.0);
            c.hold_at(hold);
            c.update(DT, cursor);
            assert_legs_ok(&c);
            assert!(c.position().distance(hold) < 1e-3);
            // Удар по пойманному не считается.
            assert!(!c.strike(c.position(), true));
        }
        // Рывок-телепорт.
        c.hold_at(c.position() + Vec2::new(-250.0, 180.0));
        c.update(DT, cursor);
        assert_legs_ok(&c);
        c.release(cursor);
        assert!(matches!(c.mode, Mode::Stunned { .. }));
        let before = c.position().distance(cursor);
        run(&mut c, 1.0, cursor);
        assert!(c.position().distance(cursor) > before);
        run(&mut c, 3.0, cursor);
        assert_eq!(c.mode, Mode::Normal);
    }

    #[test]
    fn dodges_sideways_from_rushing_cursor() {
        let mut c = Creature::new(&Preset::default(), Vec2::new(300.0, 0.0));
        c.set_game(true);
        let mut cursor = Vec2::ZERO;
        run(&mut c, 3.0, cursor);
        let start = c.position();
        let dir = (start - cursor).normalize();
        let perp = Vec2::new(-dir.y, dir.x);
        // Курсор летит на паука 1500 pt/с, пока не подлетит на 40 pt к исходной точке.
        let mut lateral: f32 = 0.0;
        while cursor.distance(start) > 40.0 {
            cursor += dir * 1500.0 * DT;
            c.update(DT, cursor);
            assert_legs_ok(&c);
            lateral = lateral.max((c.position() - start).dot(perp).abs());
        }
        assert!(c.dodge_t > 0.0 || c.dodge_cd > 0.0, "no dodge");
        assert!(lateral > 50.0, "lateral = {lateral}");
        // И паук не там, куда прилетел курсор.
        assert!(c.position().distance(cursor) > 60.0);
    }

    #[test]
    fn spider_mode_ignores_fast_cursor() {
        let mut c = Creature::new(&Preset::default(), Vec2::ZERO);
        let mut cursor = Vec2::new(-400.0, 0.0);
        for _ in 0..60 {
            cursor.x += 1500.0 * DT;
            c.update(DT, cursor);
        }
        assert!(c.dodge_t <= 0.0 && c.dodge_cd <= 0.0);
    }

    #[test]
    fn flash_tints_colours() {
        let mut c = Creature::new(&Preset::default(), Vec2::ZERO);
        let mut plain = DrawList::default();
        c.draw(&mut plain, false);
        c.strike(c.position(), false);
        let mut red = DrawList::default();
        c.draw(&mut red, false);
        assert_eq!(plain.shapes.len(), red.shapes.len());
        assert!(red.shapes.iter().zip(&plain.shapes).any(|(a, b)| a.color != b.color));
    }

    #[test]
    fn reactions_stable_for_all_leg_kinds() {
        let presets = [
            Preset::default(),
            multi_segment(vec![36.0, 38.0], LegStyle::Straight, 6),
            multi_segment(vec![7.0; 14], LegStyle::Curve, 3),
            multi_segment(vec![30.0, 32.0, 28.0], LegStyle::Dotted, 4),
        ];
        for p in &presets {
            let mut c = Creature::new(p, Vec2::ZERO);
            c.set_game(true);
            let cursor = Vec2::new(500.0, 200.0);
            run(&mut c, 2.0, cursor);
            assert!(c.strike(c.position(), false));
            run(&mut c, 2.5, cursor);
            c.grab();
            let p0 = c.position();
            for i in 0..120 {
                c.hold_at(p0 + Vec2::new(i as f32 * 4.0, (i as f32 * 0.3).sin() * 80.0));
                c.update(DT, cursor);
                assert_legs_ok(&c);
            }
            c.release(cursor);
            run(&mut c, 3.0, cursor);
            assert_eq!(c.mode, Mode::Normal, "{}", p.name);
        }
    }
}
