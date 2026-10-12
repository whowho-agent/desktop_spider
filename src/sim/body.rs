//! Корпус: arrive-стиринг и поворот по направлению движения.
use glam::Vec2;

use crate::config::BodyParams;

pub struct Body {
    pub pos: Vec2,
    pub vel: Vec2,
    /// Угол направления «вперёд», радианы (Y вниз).
    pub heading: f32,
}

impl Body {
    pub fn new(pos: Vec2) -> Self {
        Self { pos, vel: Vec2::ZERO, heading: -std::f32::consts::FRAC_PI_2 }
    }

    pub fn forward(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }

    /// Точка из системы тела (x — вперёд, y — вправо) в мир.
    pub fn to_world(&self, local: Vec2) -> Vec2 {
        self.pos + self.forward().rotate(local)
    }

    pub fn update(&mut self, dt: f32, target: Vec2, p: &BodyParams) {
        let to = target - self.pos;
        let dist = to.length();
        let k = smoothstep(p.stop_radius, p.slow_radius, dist);
        let desired = to.normalize_or_zero() * p.max_speed * k;
        self.vel += (desired - self.vel).clamp_length_max(p.max_accel * dt);
        self.pos += self.vel * dt;

        let speed = self.vel.length();
        if speed > 5.0 {
            self.turn_toward(self.vel.to_angle(), p.turn_rate * dt);
        } else if dist > 1.0 {
            // Стоим: неспешно разворачиваемся мордой к курсору.
            self.turn_toward(to.to_angle(), p.turn_rate * 0.25 * dt);
        }
    }

    fn turn_toward(&mut self, angle: f32, rate: f32) {
        let diff = wrap_angle(angle - self.heading);
        self.heading = wrap_angle(self.heading + diff * rate.min(1.0));
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Угол в (-π, π].
fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let a = a.rem_euclid(TAU);
    if a > PI { a - TAU } else { a }
}
