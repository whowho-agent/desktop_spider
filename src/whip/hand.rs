//! Рукоять: ключи удара, раскрутка, захлёст. Реализует агент физики (W2a).
//! Тайминги и углы — из `design/whip_demo.html`.

use std::f32::consts::{PI, TAU};

use glam::Vec2;

use super::HANDLE_REACH;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mode {
    Ready,
    Strike,
    Spin,
    Wrap,
}

#[derive(Clone, Copy)]
enum Ease {
    Linear,
    In,
    Out,
}

impl Ease {
    fn at(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t,
            Ease::In => t * t * t,
            Ease::Out => ease_out(t),
        }
    }
}

/// Ключ удара: время, угол рукояти, сглаживание участка до него, вынос кисти вдоль прицела.
#[derive(Clone, Copy)]
struct Key {
    t: f32,
    a: f32,
    ease: Ease,
    along: f32,
}

/// Захват цели захлёстом.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wrap {
    pub t: f32,
    /// Фаза витков вокруг цели.
    pub phase: f32,
    /// Где цель была в момент поимки.
    pub from: Vec2,
}

pub(crate) struct Hand {
    pub mode: Mode,
    /// Направление удара, рад.
    pub aim: f32,
    /// Угол рукояти, рад.
    pub angle: f32,
    /// Вынос кисти от курсора.
    pub off: Vec2,
    /// Остаток выноса поперёк прицела в начале удара (гаснет за 0,1 с).
    resid: Vec2,
    /// Время удара.
    pub t: f32,
    keys: [Key; 4],
    nkeys: usize,
    /// Удар — захлёст после раскрутки.
    pub power: bool,
    spin_phase: f32,
    pub spin_time: f32,
    pub wrap: Option<Wrap>,
}

impl Hand {
    pub fn new(aim: f32) -> Self {
        let k = Key { t: 0.0, a: 0.0, ease: Ease::Linear, along: 0.0 };
        Self {
            mode: Mode::Ready,
            aim,
            angle: aim,
            off: Vec2::ZERO,
            resid: Vec2::ZERO,
            t: 0.0,
            keys: [k; 4],
            nkeys: 0,
            power: false,
            spin_phase: 0.0,
            spin_time: 0.0,
            wrap: None,
        }
    }

    pub fn dir(&self) -> Vec2 {
        Vec2::from_angle(self.angle)
    }

    pub fn aim_dir(&self) -> Vec2 {
        Vec2::from_angle(self.aim)
    }

    pub fn pos(&self, cursor: Vec2) -> Vec2 {
        cursor + self.off
    }

    /// Кончик рукояти — корень хлыста.
    pub fn tip(&self, cursor: Vec2) -> Vec2 {
        self.pos(cursor) + self.dir() * HANDLE_REACH
    }

    /// Заряд раскрутки 0..1 (вне раскрутки 0).
    pub fn charge(&self) -> f32 {
        if self.mode == Mode::Spin {
            (self.spin_time / 1.2).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn start_strike(&mut self, power: bool) {
        self.mode = Mode::Strike;
        self.t = 0.0;
        self.power = power;
        let (a0, aim) = (self.angle, self.aim);
        let d = self.aim_dir();
        // Вынос кисти раскладываем на «вдоль прицела» (анимируется) и остаток (быстро гаснет).
        let along0 = self.off.dot(d);
        self.resid = self.off - d * along0;
        let key = |t, a, ease, along| Key { t, a, ease, along };
        if power {
            // Бросок продолжает вращение раскрутки (по часовой), без разворота назад.
            let mut delta = (aim - 0.3 - a0).rem_euclid(TAU);
            if delta < 0.8 {
                delta += TAU;
            }
            self.keys[..3].copy_from_slice(&[
                key(0.0, a0, Ease::Linear, along0),
                key(0.09, a0 + delta, Ease::In, 34.0),
                key(0.42, a0 + delta + 0.3, Ease::Out, 0.0),
            ]);
            self.nkeys = 3;
        } else {
            // Замах назад «через верх» с отводом кисти, бросок с выносом кисти вперёд, возврат.
            let c = aim.cos();
            let s = if c == 0.0 { 1.0 } else { c.signum() };
            let back = aim - s * PI * 0.85;
            self.keys = [
                key(0.0, a0, Ease::Linear, along0),
                key(0.13, a0 + ang_diff(a0, back), Ease::Out, -22.0),
                key(0.21, back + ang_diff(back, aim - 0.3), Ease::In, 30.0),
                key(0.5, aim, Ease::Out, 0.0),
            ];
            self.nkeys = 4;
        }
    }

    pub fn start_spin(&mut self) {
        self.mode = Mode::Spin;
        self.spin_time = 0.0;
        self.spin_phase = self.angle - PI / 2.0;
    }

    pub fn start_wrap(&mut self, phase: f32, from: Vec2) {
        self.mode = Mode::Wrap;
        self.wrap = Some(Wrap { t: 0.0, phase, from });
    }

    /// Вернуться в покой (отмена раскрутки, конец захвата).
    pub fn set_ready(&mut self) {
        self.mode = Mode::Ready;
        self.wrap = None;
    }

    /// Угол и вынос кисти по ключам на время `t`; `None` — удар закончен.
    fn key_at(&self, t: f32) -> Option<(f32, f32)> {
        let k = &self.keys[..self.nkeys];
        for i in 1..k.len() {
            if t <= k[i].t {
                let u = k[i].ease.at(((t - k[i - 1].t) / (k[i].t - k[i - 1].t)).clamp(0.0, 1.0));
                return Some((lerp(k[i - 1].a, k[i].a, u), lerp(k[i - 1].along, k[i].along, u)));
            }
        }
        None
    }

    /// Подшаг рукояти. `vel` — сглаженная скорость курсора.
    pub fn step(&mut self, dt: f32, vel: Vec2) {
        // Направление удара — куда двигалась мышь.
        if self.mode == Mode::Ready && vel.length() > 350.0 {
            self.aim += ang_diff(self.aim, vel.y.atan2(vel.x)) * (dt * 6.0).min(1.0);
        }
        match self.mode {
            Mode::Ready => {
                self.angle += ang_diff(self.angle, self.aim) * (dt * 8.0).min(1.0);
                self.off -= self.off * (dt * 10.0).min(1.0);
                // Держим углы в пределах ±π, чтобы не копилась погрешность.
                self.aim = (self.aim + PI).rem_euclid(TAU) - PI;
                self.angle = self.aim - ang_diff(self.angle, self.aim);
            }
            Mode::Strike => {
                self.t += dt;
                match self.key_at(self.t) {
                    None => {
                        self.mode = Mode::Ready;
                        self.angle = self.aim;
                    }
                    Some((a, along)) => {
                        self.angle = a;
                        let fade = (1.0 - self.t / 0.1).max(0.0);
                        self.off = self.aim_dir() * along + self.resid * fade;
                    }
                }
            }
            Mode::Spin => {
                // Кисть ходит по кругу, рукоять смотрит по касательной, хлыст тянется следом.
                self.spin_time += dt;
                let charge = (self.spin_time / 1.2).clamp(0.0, 1.0);
                self.spin_phase += lerp(3.0, 10.0, charge) * dt;
                self.off = Vec2::from_angle(self.spin_phase) * lerp(6.0, 28.0, charge);
                self.angle = self.spin_phase + PI / 2.0;
            }
            Mode::Wrap => {
                self.angle += ang_diff(self.angle, self.aim) * (dt * 6.0).min(1.0);
                if let Some(w) = &mut self.wrap {
                    w.t += dt;
                }
            }
        }
    }

    /// Рукоять успокоилась (угол дошёл до прицела, кисть вернулась).
    pub fn settled(&self) -> bool {
        self.mode == Mode::Ready && self.off.length() < 0.3 && ang_diff(self.angle, self.aim).abs() < 0.01
    }
}

pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(crate) fn ang_diff(a: f32, b: f32) -> f32 {
    (b - a).sin().atan2((b - a).cos())
}

pub(crate) fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}
