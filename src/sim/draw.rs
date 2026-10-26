//! Отрисовка ног по стилю: прямые сегменты, кривая Катмулла–Рома, пунктир.
use glam::Vec2;

use crate::config::{LegStyle, Preset};
use crate::render::DrawList;

/// Сэмплов кривой на один сегмент цепочки.
const CURVE_SAMPLES: usize = 4;
/// Ширина линии у кончика относительно бедра.
const TIP_WIDTH: f32 = 0.5;
/// Шаг точек пунктира, pt.
const DOT_SPACING: f32 = 4.5;

/// Цвет вспышки попадания (#ff6b6b).
const FLASH_RGB: [f32; 3] = [1.0, 107.0 / 255.0, 107.0 / 255.0];

/// Смешивает цвет пресета с красным вспышки; `k` 0..1, альфа сохраняется.
pub fn flash_color(c: [f32; 4], k: f32) -> [f32; 4] {
    let k = k.clamp(0.0, 1.0);
    let mix = |a: f32, b: f32| a + (b - a) * k;
    [mix(c[0], FLASH_RGB[0]), mix(c[1], FLASH_RGB[1]), mix(c[2], FLASH_RGB[2]), c[3]]
}

/// `flash` — сила красной вспышки (0 — цвета пресета).
pub fn draw_leg(list: &mut DrawList, joints: &[Vec2], p: &Preset, flash: f32) {
    let Some(&foot) = joints.last() else { return };
    let lw = p.line_width;
    let color_leg = flash_color(p.color_leg, flash);
    let color_joint = flash_color(p.color_joint, flash * 0.6);
    match p.legs.style {
        LegStyle::Straight => {
            for w in joints.windows(2) {
                list.segment(w[0], w[1], lw, color_leg);
            }
            for &j in &joints[1..] {
                list.dot(j, p.joint_dot, color_joint);
            }
        }
        LegStyle::Curve => {
            let mut prev = joints[0];
            for_each_curve_point(joints, |pt, frac| {
                list.segment(prev, pt, lw * taper(frac), color_leg);
                prev = pt;
            });
        }
        LegStyle::Dotted => {
            // Идём по ломаной из сэмплов и ставим точки через равные промежутки.
            let mut prev = joints[0];
            let mut next_at = DOT_SPACING * 0.5;
            let mut walked = 0.0;
            for_each_curve_point(joints, |pt, frac| {
                let len = prev.distance(pt);
                while walked + len >= next_at && len > 0.0 {
                    let c = prev.lerp(pt, (next_at - walked) / len);
                    list.dot(c, lw * 0.8 * taper(frac), color_leg);
                    next_at += DOT_SPACING;
                }
                walked += len;
                prev = pt;
            });
        }
    }
    if p.legs.style != LegStyle::Straight {
        list.dot(foot, p.joint_dot, color_joint);
    }
}

fn taper(frac: f32) -> f32 {
    1.0 + (TIP_WIDTH - 1.0) * frac
}

/// Сэмплы кривой Катмулла–Рома через все суставы (без первой точки);
/// `frac` — доля пути от бедра (0) до ступни (1). Без аллокаций.
fn for_each_curve_point(j: &[Vec2], mut f: impl FnMut(Vec2, f32)) {
    let n = j.len();
    if n < 2 {
        return;
    }
    // Фантомные концы: отражение соседнего сустава.
    let at = |i: isize| -> Vec2 {
        if i < 0 {
            j[0] * 2.0 - j[1]
        } else if i as usize >= n {
            j[n - 1] * 2.0 - j[n - 2]
        } else {
            j[i as usize]
        }
    };
    let total = ((n - 1) * CURVE_SAMPLES) as f32;
    for i in 0..n - 1 {
        let k = i as isize;
        let (p0, p1, p2, p3) = (at(k - 1), at(k), at(k + 1), at(k + 2));
        for s in 1..=CURVE_SAMPLES {
            let t = s as f32 / CURVE_SAMPLES as f32;
            let frac = (i * CURVE_SAMPLES + s) as f32 / total;
            f(catmull_rom(p0, p1, p2, p3, t), frac);
        }
    }
}

fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_passes_through_joints() {
        let j = [Vec2::ZERO, Vec2::new(10.0, 5.0), Vec2::new(20.0, 0.0)];
        let mut pts = Vec::new();
        for_each_curve_point(&j, |p, frac| pts.push((p, frac)));
        assert_eq!(pts.len(), 2 * CURVE_SAMPLES);
        assert!(pts[CURVE_SAMPLES - 1].0.distance(j[1]) < 1e-4);
        assert!(pts.last().unwrap().0.distance(j[2]) < 1e-4);
        assert!((pts.last().unwrap().1 - 1.0).abs() < 1e-6);
    }
}
