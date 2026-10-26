//! Нога: точка крепления, rest-направление, состояние шага, IK.
use std::f32::consts::PI;

use glam::Vec2;

use super::body::Body;
use super::ik::{fabrik, smooth, two_bone_ik};
use crate::config::{BodyParams, LegParams};

/// Итерации FABRIK за тик (стартуем с прошлого решения, этого хватает).
const FABRIK_ITERS: usize = 10;
/// Скорость подтягивания суставов к шаблону выгиба (дуге), 1/с.
/// Ниже ~60 на бегу FABRIK успевает сложить цепочку пополам.
const POLE_RATE: f32 = 80.0;
/// Проходы сглаживания цепочек 3+ и их сила.
const SMOOTH_PASSES: usize = 2;
const SMOOTH_K: f32 = 0.5;
/// Поджатая ступня: доля `rest_dist` от бедра и скорость подтягивания, 1/с.
const CURL_DIST: f32 = 0.4;
const CURL_RATE: f32 = 22.0;

#[derive(Clone, Copy)]
pub enum LegState {
    Planted { foot: Vec2 },
    /// Пока нога в воздухе, конечная точка следует за текущей идеальной.
    Stepping { from: Vec2, t: f32 },
}

pub struct Leg {
    hip_local: Vec2,
    /// Направление rest-позиции в системе тела.
    rest_dir: Vec2,
    /// Знак выгиба колена, чтобы колено смотрело наружу от тела.
    bend: f32,
    pub state: LegState,
    pub foot: Vec2,
    pub ideal: Vec2,
    /// Суставы от бедра до ступни (N+1 точек); последний — достижимая ступня.
    pub joints: Vec<Vec2>,
    /// Шаблон выгиба (дуга) этого кадра, тот же размер, что `joints`.
    templ: Vec<Vec2>,
}

impl Leg {
    /// `side`: -1 — левая, +1 — правая (относительно «вперёд», Y вниз).
    pub fn new(pair: usize, side: f32, body: &Body, bp: &BodyParams, lp: &LegParams) -> Self {
        let hip_local = Vec2::new(lp.hip_along[pair] * bp.half_len, side * bp.half_width * 0.6);
        let spread = lp.spread_deg[pair];
        let rest_dir = Vec2::from_angle(side * spread.to_radians());
        // perp(dir) = (-dir.y, dir.x): у ног, смотрящих вперёд, он направлен наружу при
        // bend = side, у смотрящих назад — внутрь, поэтому знак меняется.
        let bend = side * if spread < 90.0 { 1.0 } else { -1.0 };
        let mut leg = Self {
            hip_local,
            rest_dir,
            bend,
            state: LegState::Planted { foot: Vec2::ZERO },
            foot: Vec2::ZERO,
            ideal: Vec2::ZERO,
            joints: vec![Vec2::ZERO; lp.segments.len() + 1],
            templ: vec![Vec2::ZERO; lp.segments.len() + 1],
        };
        leg.ideal = leg.compute_ideal(body, lp);
        leg.foot = leg.ideal;
        leg.state = LegState::Planted { foot: leg.ideal };
        // Первый кадр: суставы сразу ставим в шаблон выгиба.
        leg.solve(body, lp, 1.0);
        leg
    }

    pub fn hip(&self, body: &Body) -> Vec2 {
        body.to_world(self.hip_local)
    }

    fn compute_ideal(&self, body: &Body, lp: &LegParams) -> Vec2 {
        let dir = body.forward().rotate(self.rest_dir);
        self.hip(body) + dir * lp.rest_dist + body.vel * lp.step_lead
    }

    pub fn is_stepping(&self) -> bool {
        matches!(self.state, LegState::Stepping { .. })
    }

    /// Насколько стоящая ступня отстала от идеала.
    pub fn error(&self) -> f32 {
        self.foot.distance(self.ideal)
    }

    pub fn start_step(&mut self) {
        if let LegState::Planted { foot } = self.state {
            self.state = LegState::Stepping { from: foot, t: 0.0 };
        }
    }

    /// Обновляет идеал и анимацию шага, затем решает IK.
    #[cfg(test)]
    pub fn update(&mut self, dt: f32, body: &Body, lp: &LegParams) {
        self.update_scaled(dt, body, lp, 1.0);
    }

    /// То же, но длительность шага умножена на `step_scale` (на быстром бегстве < 1).
    pub fn update_scaled(&mut self, dt: f32, body: &Body, lp: &LegParams, step_scale: f32) {
        self.ideal = self.compute_ideal(body, lp);
        if step_scale < 1.0 {
            // Быстрее обычного (бегство, уворот): упреждение не должно уводить идеал за досягаемость.
            let hip = self.hip(body);
            let reach: f32 = lp.segments.iter().sum();
            self.ideal = hip + (self.ideal - hip).clamp_length_max(reach * 0.95);
        }
        match &mut self.state {
            LegState::Planted { foot } => self.foot = *foot,
            LegState::Stepping { from, t } => {
                *t += dt / (lp.step_duration * step_scale);
                if *t >= 1.0 {
                    self.foot = self.ideal;
                    self.state = LegState::Planted { foot: self.ideal };
                } else {
                    let foot = from.lerp(self.ideal, ease_in_out(*t));
                    // На быстром беге/после рывка начало шага бывает дальше досягаемости:
                    // ведём ступню по краю досягаемости (FABRIK и так вытянул бы прямо).
                    let hip = self.hip(body);
                    let reach: f32 = lp.segments.iter().sum();
                    self.foot = hip + (foot - hip).clamp_length_max(reach);
                }
            }
        }
        self.solve(body, lp, 1.0 - (-POLE_RATE * dt).exp());
    }

    /// Поджатая нога (оглушение, захлёст): ступня тянется к точке у тела, `wiggle` —
    /// поворот этой точки вокруг бедра (дрыгание), рад. Ступня всегда в досягаемости.
    pub fn update_curled(&mut self, dt: f32, body: &Body, lp: &LegParams, wiggle: f32) {
        self.ideal = self.compute_ideal(body, lp);
        let hip = self.hip(body);
        let dir = body.forward().rotate(self.rest_dir).rotate(Vec2::from_angle(wiggle * self.bend));
        let reach: f32 = lp.segments.iter().sum();
        let curled = hip + dir * (lp.rest_dist * CURL_DIST).min(reach * 0.6);
        let k = 1.0 - (-CURL_RATE * dt).exp();
        let mut foot = self.foot.lerp(curled, k);
        // Тело могли телепортировать — не даём ноге вытянуться за предел.
        foot = hip + (foot - hip).clamp_length_max(reach * 0.95);
        if !foot.is_finite() {
            foot = curled;
        }
        self.foot = foot;
        self.state = LegState::Planted { foot };
        self.solve(body, lp, 1.0 - (-POLE_RATE * dt).exp());
    }

    /// Текущий подъём ноги (0 у стоящей).
    fn lift(&self, lp: &LegParams) -> f32 {
        match self.state {
            LegState::Stepping { t, .. } => (PI * t).sin() * lp.lift,
            LegState::Planted { .. } => 0.0,
        }
    }

    /// `pole_blend` — доля подтяжки суставов к шаблону выгиба (только для 3+ сегментов).
    fn solve(&mut self, body: &Body, lp: &LegParams, pole_blend: f32) {
        let hip = self.hip(body);
        let segs = &lp.segments;
        let lift = self.lift(lp);
        if segs.len() >= 3 {
            self.drag_with_ends(hip, segs);
        }
        self.joints[0] = hip;
        match segs.len() {
            0 => {}
            1 => {
                let dir = (self.foot - hip).try_normalize().unwrap_or(Vec2::X);
                self.joints[1] = hip + dir * segs[0];
            }
            2 => {
                let (knee, foot) = two_bone_ik(hip, self.foot, segs[0], segs[1], self.bend);
                // Подъём ноги: в 2D просто выгибаем колено сильнее наружу.
                let dir = (self.foot - hip).normalize_or_zero();
                let perp = Vec2::new(-dir.y, dir.x) * self.bend;
                self.joints[1] = knee + perp * lift;
                self.joints[2] = foot;
            }
            _ => {
                self.pull_to_pole(hip, segs, lift, pole_blend);
                fabrik(&mut self.joints, segs, hip, self.foot, FABRIK_ITERS);
                // Гасим зигзаги (важно для щупалец) и восстанавливаем длины.
                for _ in 0..SMOOTH_PASSES {
                    smooth(&mut self.joints, &self.templ, SMOOTH_K);
                }
                fabrik(&mut self.joints, segs, hip, self.foot, FABRIK_ITERS / 2);
            }
        }
    }

    /// Сдвигает прошлое решение вслед за концами: бедро тянет начало цепочки,
    /// ступня — конец, с линейным переходом по длине. Иначе при быстром беге
    /// или шаге концы убегают от цепочки и у них появляются изломы.
    fn drag_with_ends(&mut self, hip: Vec2, segs: &[f32]) {
        let last = self.joints.len() - 1;
        let dh = hip - self.joints[0];
        let df = self.foot - self.joints[last];
        let total: f32 = segs.iter().sum();
        if total <= 0.0 || !(dh.is_finite() && df.is_finite()) {
            return;
        }
        let mut acc = 0.0;
        for i in 0..=last {
            let s = acc / total;
            self.joints[i] += dh * (1.0 - s) + df * s;
            acc += segs.get(i).copied().unwrap_or(0.0);
        }
    }

    /// Подтягивает внутренние суставы к шаблону: дуга окружности длиной в цепочку
    /// от бедра к цели, выгнутая наружу. Не даёт цепочке перевернуться и сложиться,
    /// подъём — дополнительный выгиб `sin(πs)` поверх дуги.
    fn pull_to_pole(&mut self, hip: Vec2, segs: &[f32], lift: f32, blend: f32) {
        let total: f32 = segs.iter().sum();
        if total <= 0.0 {
            return;
        }
        let to = self.foot - hip;
        let u = to.try_normalize().unwrap_or(Vec2::X);
        let n = Vec2::new(-u.y, u.x) * self.bend;
        let d = to.length().min(total);
        // Дуга: полуугол α из sin(α)/α = d/L, радиус R = L/(2α).
        let alpha = arc_half_angle(d / total);
        let last = self.joints.len() - 1;
        self.templ[0] = hip;
        self.templ[last] = hip + u * d;
        let mut acc = 0.0;
        for i in 1..last {
            acc += segs[i - 1];
            let s = acc / total;
            // Локальные координаты: x — вдоль хорды, y — наружу.
            let local = if alpha < 1e-4 {
                Vec2::new(acc, 0.0)
            } else {
                let r = total / (2.0 * alpha);
                let a = std::f32::consts::FRAC_PI_2 + alpha - 2.0 * alpha * s;
                Vec2::new(r * (alpha.sin() + a.cos()), r * (a.sin() - alpha.cos()))
            };
            self.templ[i] = hip + u * local.x + n * (local.y + lift * (PI * s).sin());
            self.joints[i] = self.joints[i].lerp(self.templ[i], blend);
        }
    }
}

/// Решает sin(α)/α = ratio для α ∈ [0, 0.95π] бисекцией (функция убывает).
fn arc_half_angle(ratio: f32) -> f32 {
    let ratio = ratio.clamp(0.0, 1.0);
    let (mut lo, mut hi) = (0.0_f32, 0.95 * PI);
    for _ in 0..20 {
        let mid = 0.5 * (lo + hi);
        if mid.sin() / mid.max(1e-6) > ratio { lo = mid } else { hi = mid }
    }
    0.5 * (lo + hi)
}

fn ease_in_out(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LegStyle, Preset};

    /// Знак отклонения среднего сустава от хорды бедро→ступня.
    fn bulge_side(j: &[Vec2]) -> f32 {
        let (a, b, m) = (j[0], *j.last().unwrap(), j[j.len() / 2]);
        (b - a).perp_dot(m - a)
    }

    #[test]
    fn chains_bulge_outward_and_stay_smooth() {
        for (segs, style) in [
            (vec![30.0, 32.0, 28.0], LegStyle::Straight),
            (vec![7.0; 14], LegStyle::Curve),
        ] {
            let mut p = Preset::default();
            let total: f32 = segs.iter().sum();
            p.legs.rest_dist = total * 0.7;
            p.legs.segments = segs;
            p.legs.style = style;
            let mut body = Body::new(Vec2::ZERO);
            let mut legs: Vec<Leg> = (0..4)
                .flat_map(|pair| [-1.0, 1.0].map(|s| (pair, s)))
                .map(|(pair, s)| Leg::new(pair, s, &body, &p.body, &p.legs))
                .collect();
            // Тело медленно крутится на месте — ступни стоят, цепочки тянутся.
            for _ in 0..240 {
                body.heading += 0.004;
                for l in &mut legs {
                    l.update(1.0 / 120.0, &body, &p.legs);
                    assert!(bulge_side(&l.joints) * l.bend > 0.0, "flipped");
                    if style == LegStyle::Curve {
                        for w in l.joints.windows(3) {
                            let turn = (w[1] - w[0]).angle_to(w[2] - w[1]).abs();
                            assert!(turn < 0.6, "zigzag: {turn}");
                        }
                    }
                }
            }
        }
    }
}
