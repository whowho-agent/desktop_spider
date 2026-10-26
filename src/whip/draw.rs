//! Отрисовка плети v2 (`design/whip_v2.html`) в DrawList.
//!
//! Объём (затенение, блики, пряди) строит шейдер по виду фигуры: здесь только геометрия
//! и базовые цвета. Свет нужен лишь для смещения тени.

use glam::Vec2;

use super::skin::LeatherPalette;
use super::WhipPose;
use crate::render::DrawList;

/// Точка курсора на рукояти (локальная x).
pub const GRIP_X: f32 = 8.0;
/// Кончик рукояти (локальная x) = начало хлыста.
pub const TIP_X: f32 = 46.0;

/// Цвет кольца заряда (#ffd36b).
const ACCENT: [f32; 3] = [1.0, 0.827, 0.42];
/// Начало фола (доля длины хлыста).
const FALL_START: f32 = 0.82;
/// Темляк: `M-9 0 C-23 -1 -31 14 -21 22 C-13 28 -6 15 -8 4`.
const LOOP: [[(f32, f32); 4]; 2] = [
    [(-9.0, 0.0), (-23.0, -1.0), (-31.0, 14.0), (-21.0, 22.0)],
    [(-21.0, 22.0), (-13.0, 28.0), (-6.0, 15.0), (-8.0, 4.0)],
];
/// Точек на каждую кривую темляка (итого 2·LOOP_STEPS отрезков).
const LOOP_STEPS: usize = 9;

/// Рисует плеть в позе `pose` палитрой `pal`; `light` — единичный вектор на свет (экран, Y вниз),
/// от него зависит только сторона тени.
/// Порядок: тень, кромка/хлыст, узел, хлопушка, темляк, рукоять, кольцо заряда.
pub fn draw_whip(list: &mut DrawList, pose: &WhipPose, pal: &LeatherPalette, light: Vec2) {
    let s = pose.scale;
    let frame = Frame { hand: pose.hand, rot: Vec2::from_angle(pose.angle), s };
    let off = shadow_offset(light, s);
    draw_shadow(list, pose.rope, &frame, off);
    draw_lash(list, pose.rope, pal, s);
    draw_handle(list, &frame, pal);
    if pose.charge > 0.0 {
        draw_charge(list, pose.hand, pose.charge.min(1.0), s);
    }
}

/// Смещение тени: от света, (∓5, 8)·scale.
pub fn shadow_offset(light: Vec2, scale: f32) -> Vec2 {
    let sx = if light.x < 0.0 { -1.0 } else { 1.0 };
    Vec2::new(-5.0 * sx, 8.0) * scale
}

/// Локальные координаты рукояти → мир.
struct Frame {
    hand: Vec2,
    rot: Vec2,
    s: f32,
}

impl Frame {
    fn at(&self, x: f32, y: f32) -> Vec2 {
        self.hand + self.rot.rotate(Vec2::new((x - GRIP_X) * self.s, y * self.s))
    }
}

/// Точки темляка в локальных координатах.
fn loop_points() -> Vec<(f32, f32)> {
    let mut pts = vec![LOOP[0][0]];
    for c in &LOOP {
        for k in 1..=LOOP_STEPS {
            let t = k as f32 / LOOP_STEPS as f32;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            let x = (0..4).map(|j| c[j].0 * w[j]).sum();
            let y = (0..4).map(|j| c[j].1 * w[j]).sum();
            pts.push((x, y));
        }
    }
    pts
}

/// Полуширина хлыста (без масштаба) в доле длины `s`.
fn half_w(s: f32) -> f32 {
    if s < FALL_START {
        lerp(3.4, 1.1, s / FALL_START)
    } else {
        0.7
    }
}

/// Мягкая тень: хлыст, рукоять, темляк — рисуется первой.
fn draw_shadow(list: &mut DrawList, pts: &[Vec2], f: &Frame, off: Vec2) {
    let s = f.s;
    let col = [0.0, 0.0, 0.0, 0.3];
    let soft = 2.6 * s;
    let n = pts.len();
    // Хлыст — через звено: меньше стыков, где полупрозрачные капсулы складываются.
    if n >= 2 {
        let mut i = 0;
        while i < n - 1 {
            let j = (i + 2).min(n - 1);
            let w = (2.0 * half_w((i as f32 + 1.0) / (n - 1) as f32) + 1.5) * s;
            list.soft(pts[i] + off, pts[j] + off, w, col, soft);
            i = j;
        }
    }
    list.soft(f.at(-9.0, 0.0) + off, f.at(TIP_X, 0.0) + off, 8.5 * s, [0.0, 0.0, 0.0, 0.32], soft);
    let lp = loop_points();
    for k in (0..lp.len() - 1).step_by(2) {
        let (a, b) = (lp[k], lp[(k + 2).min(lp.len() - 1)]);
        list.soft(f.at(a.0, a.1) + off, f.at(b.0, b.1) + off, 3.0 * s, col, soft);
    }
}

/// Хлыст по точкам `pts` (pts[0] — кончик рукояти): кромка (тёмная кожа), плетёные звенья,
/// гладкий фол, узел фола, хлопушка. `k` — масштаб толщины.
pub fn draw_lash(list: &mut DrawList, pts: &[Vec2], pal: &LeatherPalette, k: f32) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    let fi = (FALL_START * (n - 1) as f32).round() as usize;
    let link_w = |i: usize| 2.0 * half_w((i as f32 + 0.5) / (n - 1) as f32) * k;

    if let Some(rim) = pal.rim {
        for i in 0..n - 1 {
            list.segment(pts[i], pts[i + 1], link_w(i) + 0.9 * k, rim);
        }
    }
    for i in 0..n - 1 {
        let w = link_w(i);
        if i < fi {
            // Шаг узора = две пряди по 0,6 полуширины, как в эскизе.
            list.braid(pts[i], pts[i + 1], w, pal.leather, 0.6 * w);
        } else {
            list.tube(pts[i], pts[i + 1], w, pal.fall);
        }
    }
    // Узел в начале фола.
    let kp = pts[fi.min(n - 1)];
    list.leather_sphere(kp, half_w(FALL_START - 0.01) * 1.7 * k, pal.leather);
    // Хлопушка: 7 нитей веером.
    let tip = pts[n - 1];
    let dir = (tip - pts[n.saturating_sub(3)]).normalize_or(Vec2::X);
    let col = [pal.cracker[0], pal.cracker[1], pal.cracker[2], pal.cracker[3] * 0.92];
    for j in 0..7 {
        let a = -0.5 + j as f32 / 6.0;
        let l = (8.0 + 3.0 * (j as f32 * 1.7 + 0.5).sin()) * k;
        list.segment(tip, tip + Vec2::from_angle(a).rotate(dir) * l, 0.6 * k, col);
    }
}

/// Темляк и рукоять: навершие, кольцо, оплётка, переход, турецкий узел, кольцо перед хлыстом.
fn draw_handle(list: &mut DrawList, f: &Frame, pal: &LeatherPalette) {
    let s = f.s;
    let lp = loop_points();
    // Части рукояти: (a, b, полуширина) в локальных координатах.
    const GRIP: ((f32, f32), (f32, f32), f32) = ((1.0 + 3.9, 0.0), (32.5 - 3.9, 0.0), 3.9);
    const BOLSTER: [(f32, f32, f32); 3] = [(39.0, 39.8, 3.4), (40.2, 41.8, 2.6), (41.4, 42.6, 1.9)];
    const KNOT_X: [f32; 2] = [33.6, 35.6];
    const KNOT_Y: [f32; 3] = [-2.7, 0.0, 2.7];
    const KNOT_R: f32 = 1.65;

    // Кромка для тёмной кожи — подо всеми кожаными частями сразу.
    if let Some(rim) = pal.rim {
        let pad = 0.9 * s;
        for w in lp.windows(2) {
            list.segment(f.at(w[0].0, w[0].1), f.at(w[1].0, w[1].1), 2.8 * s + pad, rim);
        }
        let (a, b, h) = GRIP;
        list.segment(f.at(a.0, a.1), f.at(b.0, b.1), 2.0 * h * s + pad, rim);
        for &x in &KNOT_X {
            for &y in &KNOT_Y {
                list.dot(f.at(x, y), KNOT_R * s + pad * 0.5, rim);
            }
        }
        for &(x0, x1, h) in &BOLSTER {
            list.segment(f.at(x0, 0.0), f.at(x1, 0.0), 2.0 * h * s + pad, rim);
        }
    }

    // Темляк.
    for w in lp.windows(2) {
        list.tube(f.at(w[0].0, w[0].1), f.at(w[1].0, w[1].1), 2.8 * s, pal.leather);
    }
    // Навершие.
    list.sphere(f.at(-6.0, 0.0), 5.2 * s, pal.metal);
    // Кольцо у навершия: 3 × 9, поперёк оси.
    list.metal(f.at(-0.3, -3.0), f.at(-0.3, 3.0), 3.0 * s, pal.metal);
    // Оплётка ромбом.
    let (a, b, h) = GRIP;
    list.braid(f.at(a.0, a.1), f.at(b.0, b.1), 2.0 * h * s, pal.leather, 3.2 * s);
    // Кожаный переход: сужается 3,5 → 1,9; левый край прячется под турецкий узел.
    for &(x0, x1, h) in &BOLSTER {
        list.tube(f.at(x0, 0.0), f.at(x1, 0.0), 2.0 * h * s, pal.leather);
    }
    // Турецкий узел.
    for &x in &KNOT_X {
        for &y in &KNOT_Y {
            list.leather_sphere(f.at(x, y), KNOT_R * s, pal.leather);
        }
    }
    // Кольцо перед хлыстом: 2,8 × 4,6.
    list.metal(f.at(44.8, -0.9), f.at(44.8, 0.9), 2.8 * s, pal.metal);
}

/// Пунктирное кольцо заряда (в эскизе dasharray «3 5», толщина 1,2).
pub fn draw_charge(list: &mut DrawList, center: Vec2, charge: f32, scale: f32) {
    let r = (38.0 + 6.0 * charge) * scale;
    let col = [ACCENT[0], ACCENT[1], ACCENT[2], 0.9 * charge];
    let w: f32 = 1.2 * scale;
    let n = ((std::f32::consts::TAU * r) / 8.0).round().max(8.0) as usize;
    // Штрих 3 pt по дуге; капсула добавляет w с концов, поэтому ось короче.
    let half = ((3.0 * scale - w) * 0.5).max(0.3 * scale) / r;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p0 = center + Vec2::from_angle(a - half) * r;
        let p1 = center + Vec2::from_angle(a + half) * r;
        list.segment(p0, p1, w, col);
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Выгрузка DrawList в SVG — проверка картинки без окна. Объёмные виды — приближённо:
/// трубки градиентом поперёк оси, пряди — штрихами, сферы — радиальным градиентом, тень — размытием.
#[cfg(test)]
pub(crate) mod svg {
    use crate::render::kind;
    use crate::render::DrawList;
    use glam::Vec2;
    use std::fmt::Write;

    /// Свет для схемы — сверху слева.
    const LIGHT: Vec2 = Vec2::new(-0.451, -0.892);

    fn hexc(c: [f32; 4], k: f32) -> String {
        let f = |v: f32| ((v * k).clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", f(c[0]), f(c[1]), f(c[2]))
    }

    /// Чуть подсветлить к белому (для бликов).
    fn tint(c: [f32; 4], t: f32) -> [f32; 4] {
        [c[0] + (1.0 - c[0]) * t, c[1] + (1.0 - c[1]) * t, c[2] + (1.0 - c[2]) * t, c[3]]
    }

    /// Фигуры списка как SVG-элементы (в порядке вставки).
    pub fn shapes(list: &DrawList) -> String {
        let mut s = String::new();
        for (id, sh) in list.shapes.iter().enumerate() {
            let (a, b) = (Vec2::from(sh.a), Vec2::from(sh.b));
            let c = sh.color;
            let op = c[3];
            let k = sh.style[0];
            let is_point = a == b;
            if is_point && sh.ring_width > 0.0 {
                let _ = writeln!(
                    s,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke-width=\"{}\" stroke=\"{}\" stroke-opacity=\"{op:.3}\"/>",
                    a.x, a.y, sh.radius, 2.0 * sh.ring_width, hexc(c, 1.0)
                );
            } else if k == kind::SPHERE {
                let fx = 0.5 + LIGHT.x * 0.28;
                let fy = 0.5 + LIGHT.y * 0.28;
                let _ = writeln!(
                    s,
                    "<radialGradient id=\"g{id}\" cx=\"0.5\" cy=\"0.5\" r=\"0.62\" fx=\"{fx}\" fy=\"{fy}\">\
                     <stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"0.3\" stop-color=\"{}\"/>\
                     <stop offset=\"0.65\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></radialGradient>\
                     <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"url(#g{id})\" fill-opacity=\"{op:.3}\"/>",
                    hexc(tint(c, 0.8), 1.0),
                    hexc(c, 1.7),
                    hexc(c, 1.0),
                    hexc(c, 0.42),
                    a.x,
                    a.y,
                    sh.radius
                );
            } else if is_point {
                let _ = writeln!(
                    s,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" fill-opacity=\"{op:.3}\"/>",
                    a.x, a.y, sh.radius, hexc(c, 1.0)
                );
            } else if k == kind::SOFT {
                let _ = writeln!(
                    s,
                    "<filter id=\"g{id}\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"{}\"/></filter>\
                     <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-linecap=\"round\" stroke-width=\"{}\" stroke=\"{}\" stroke-opacity=\"{op:.3}\" filter=\"url(#g{id})\"/>",
                    sh.style[1] * 0.5, a.x, a.y, b.x, b.y, 2.0 * sh.radius, hexc(c, 1.0)
                );
            } else if k == kind::TUBE || k == kind::BRAID || k == kind::METAL {
                let d = (b - a).normalize_or(Vec2::X);
                let mut nrm = Vec2::new(-d.y, d.x);
                if nrm.dot(LIGHT) < 0.0 {
                    nrm = -nrm;
                }
                let m = (a + b) * 0.5;
                let (p0, p1) = (m - nrm * sh.radius, m + nrm * sh.radius);
                let stops: &[(f32, String)] = if k == kind::METAL {
                    &[
                        (0.0, hexc(c, 0.5)),
                        (0.45, hexc(c, 1.0)),
                        (0.78, hexc(c, 1.6)),
                        (0.9, hexc(tint(c, 0.85), 1.0)),
                        (1.0, hexc(c, 1.0)),
                    ]
                } else {
                    &[
                        (0.0, hexc(c, 0.42)),
                        (0.32, hexc(c, 1.0)),
                        (0.6, hexc(c, 1.25)),
                        (0.8, hexc(c, 1.7)),
                        (0.9, hexc(tint(c, 0.6), 1.0)),
                        (1.0, hexc(c, 1.25)),
                    ]
                };
                let _ = write!(
                    s,
                    "<linearGradient id=\"g{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">",
                    p0.x, p0.y, p1.x, p1.y
                );
                for (o, col) in stops {
                    let _ = write!(s, "<stop offset=\"{o}\" stop-color=\"{col}\"/>");
                }
                let _ = writeln!(
                    s,
                    "</linearGradient><line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-linecap=\"round\" stroke-width=\"{}\" stroke=\"url(#g{id})\" stroke-opacity=\"{op:.3}\"/>",
                    a.x, a.y, b.x, b.y, 2.0 * sh.radius
                );
                if k == kind::BRAID && sh.style[1] > 0.0 {
                    // Пряди: диагонали через полшага, направление чередуется.
                    let len = a.distance(b);
                    let step = sh.style[1] * 0.5;
                    let r = sh.radius * 0.85;
                    let lw = (sh.radius * 0.25).max(0.2);
                    let mut t = -sh.radius * 0.6;
                    let mut j = 0;
                    while t < len + sh.radius * 0.6 {
                        let sg = if j % 2 == 0 { 1.0 } else { -1.0 };
                        let q = step * 0.62 * sg;
                        let cpt = a + d * t;
                        let (u, v) = (cpt - d * q + nrm * r, cpt + d * q - nrm * r);
                        let _ = writeln!(
                            s,
                            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-linecap=\"round\" stroke-width=\"{lw}\" stroke=\"{}\" stroke-opacity=\"0.6\"/>",
                            u.x, u.y, v.x, v.y, hexc(c, 0.55)
                        );
                        t += step;
                        j += 1;
                    }
                }
            } else {
                let _ = writeln!(
                    s,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-linecap=\"round\" stroke-width=\"{}\" stroke=\"{}\" stroke-opacity=\"{op:.3}\"/>",
                    a.x, a.y, b.x, b.y, 2.0 * sh.radius, hexc(c, 1.0)
                );
            }
        }
        s
    }

    /// Готовый SVG: тёмный фон, справа от `light_from` — светлая подложка; `zoom` — масштаб вывода.
    pub fn document(list: &DrawList, w: f32, h: f32, light_from: f32, zoom: f32) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {w} {h}\">\n\
             <rect width=\"{w}\" height=\"{h}\" fill=\"#0b0b12\"/>\n\
             <rect x=\"{light_from}\" width=\"{}\" height=\"{h}\" fill=\"#ececf2\"/>\n{}</svg>\n",
            w * zoom,
            h * zoom,
            (w - light_from).max(0.0),
            shapes(list)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::kind;
    use crate::settings::Leather;
    use crate::whip::fx::Fx;
    use crate::whip::skin::leather_palette;

    const N: usize = crate::whip::rope::N;
    const SEG: f32 = 7.4;
    const TONES: [Leather; 6] = [
        Leather::Natural,
        Leather::Cognac,
        Leather::Chocolate,
        Leather::Burgundy,
        Leather::Black,
        Leather::Custom([0x20, 0x40, 0x30]),
    ];

    fn light_left() -> Vec2 {
        Vec2::new(-0.45, -0.89).normalize()
    }

    fn light_right() -> Vec2 {
        Vec2::new(0.45, -0.89).normalize()
    }

    fn tip_of(hand: Vec2, angle: f32, scale: f32) -> Vec2 {
        hand + Vec2::from_angle(angle) * (TIP_X - GRIP_X) * scale
    }

    /// Хлыст из точки `tip` под углом, с плавным изгибом `bend` (рад на всю длину).
    fn arc_rope(tip: Vec2, angle: f32, bend: f32, scale: f32) -> Vec<Vec2> {
        let mut pts = vec![tip];
        let mut a = angle;
        for _ in 1..N {
            let p = *pts.last().unwrap() + Vec2::from_angle(a) * SEG * scale;
            pts.push(p);
            a += bend / (N - 1) as f32;
        }
        pts
    }

    fn all_finite(list: &DrawList) -> bool {
        list.shapes.iter().all(|s| {
            s.a.iter().chain(&s.b).chain(&s.color).chain(&s.style).all(|v| v.is_finite())
                && s.radius.is_finite()
                && s.radius >= 0.0
                && s.ring_width.is_finite()
        })
    }

    fn whip(tone: Leather, hand: Vec2, angle: f32, bend: f32, charge: f32, scale: f32, light: Vec2) -> DrawList {
        let rope = arc_rope(tip_of(hand, angle, scale), angle, bend, scale);
        let pose = WhipPose { hand, angle, rope: &rope, charge, scale };
        let mut list = DrawList::default();
        draw_whip(&mut list, &pose, &leather_palette(tone), light);
        list
    }

    #[test]
    fn draw_whip_finite_for_all_tones_and_scales() {
        for tone in TONES {
            for scale in [0.7, 1.0, 2.0] {
                for (angle, bend, charge) in [(0.0, 0.0, 0.0), (-0.9, 2.0, 0.5), (2.5, -3.0, 1.0)] {
                    let list = whip(tone, Vec2::new(300.0, 200.0), angle, bend, charge, scale, light_left());
                    assert!(list.shapes.len() > 60 && list.shapes.len() < 400, "{}", list.shapes.len());
                    assert!(all_finite(&list));
                }
                // Вырожденный хлыст: все точки совпали — без NaN.
                let rope = vec![Vec2::new(10.0, 10.0); N];
                let pose = WhipPose { hand: Vec2::ZERO, angle: 0.0, rope: &rope, charge: 0.0, scale };
                let mut list = DrawList::default();
                draw_whip(&mut list, &pose, &leather_palette(tone), light_left());
                assert!(all_finite(&list));
            }
        }
    }

    #[test]
    fn rim_only_for_dark_leather() {
        for tone in TONES {
            let pal = leather_palette(tone);
            let list = whip(tone, Vec2::new(300.0, 200.0), 0.3, 1.0, 0.0, 1.0, light_left());
            let flat = list.shapes.iter().filter(|s| s.style[0] == kind::FLAT).count();
            // Без кромки плоские — только 7 нитей хлопушки.
            match pal.rim {
                Some(_) => assert!(flat > 40, "{tone:?}: {flat}"),
                None => assert_eq!(flat, 7, "{tone:?}"),
            }
        }
        assert!(leather_palette(Leather::Black).rim.is_some());
        assert!(leather_palette(Leather::Natural).rim.is_none());
    }

    #[test]
    fn shadow_first_and_opposite_to_light() {
        let hand = Vec2::new(300.0, 200.0);
        let rope = arc_rope(tip_of(hand, 0.0, 1.0), 0.0, 0.0, 1.0);
        for (light, sx) in [(light_left(), 1.0), (light_right(), -1.0)] {
            let list = whip(Leather::Natural, hand, 0.0, 0.0, 0.0, 1.0, light);
            let first = &list.shapes[0];
            assert_eq!(first.style[0], kind::SOFT);
            let off = Vec2::from(first.a) - rope[0];
            assert!((off - Vec2::new(5.0 * sx, 8.0)).length() < 1e-3, "{off}");
            // Все тени идут подряд в начале списка.
            let soft = list.shapes.iter().take_while(|s| s.style[0] == kind::SOFT).count();
            assert_eq!(soft, list.shapes.iter().filter(|s| s.style[0] == kind::SOFT).count());
        }
        // Масштаб тянет и смещение.
        assert_eq!(shadow_offset(light_left(), 2.0), Vec2::new(10.0, 16.0));
    }

    #[test]
    fn charge_ring_last() {
        let list = whip(Leather::Natural, Vec2::new(300.0, 200.0), 0.0, 0.0, 1.0, 1.0, light_left());
        let last = list.shapes.last().unwrap();
        assert_eq!(last.style[0], kind::FLAT);
        assert!((last.color[0] - ACCENT[0]).abs() < 1e-6);
    }

    /// Лист для просмотра: WHIP_SHEET=1 cargo test whip_sheet -- пишет SVG.
    #[test]
    fn whip_sheet() {
        let mut list = DrawList::default();
        let (w, h) = (1000.0, 1000.0);

        // 1. Крупно ×2,2: натуральная и чёрная, свет слева.
        for (tone, oy) in [(Leather::Natural, 70.0), (Leather::Black, 190.0)] {
            let s = 2.2;
            list.shapes.extend(whip(tone, Vec2::new(60.0 + GRIP_X * s, oy), 0.0, 0.9, 0.0, s, light_left()).shapes);
        }

        // 2. Натуральный размер: все тона, прямо / изогнуто / висит / с зарядом.
        let poses: [(Vec2, f32, f32, f32); 3] = [
            (Vec2::new(40.0, 420.0), 0.0, 0.0, 0.0),
            (Vec2::new(270.0, 450.0), -0.6, 2.2, 0.0),
            (Vec2::new(560.0, 450.0), -2.2, -2.5, 1.0),
        ];
        for (k, tone) in TONES.into_iter().take(5).enumerate() {
            for &(hand, angle, bend, charge) in &poses {
                let hand = hand + Vec2::new(0.0, k as f32 * 100.0);
                list.shapes.extend(whip(tone, hand, angle, bend, charge, 1.0, light_left()).shapes);
            }
        }
        // Справа на светлом окне: свет слева и справа (тень меняет сторону).
        for (k, tone) in [Leather::Natural, Leather::Black, Leather::Burgundy].into_iter().enumerate() {
            for (j, light) in [light_left(), light_right()].into_iter().enumerate() {
                let hand = Vec2::new(740.0 + j as f32 * 30.0, 420.0 + k as f32 * 150.0 + j as f32 * 70.0);
                list.shapes.extend(whip(tone, hand, 0.3, 1.5, 0.0, 1.0, light).shapes);
            }
        }

        // 3. Щелчок в разные моменты и усиленный, след.
        let mut fx_list = DrawList::default();
        let ages = [0.03f32, 0.08, 0.15, 0.25, 0.35];
        for (i, age) in ages.into_iter().enumerate() {
            let mut fx = Fx::default();
            fx.crack(Vec2::new(60.0 + i as f32 * 95.0, 950.0), false);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        for (i, age) in [0.06f32, 0.15].into_iter().enumerate() {
            let mut fx = Fx::default();
            fx.crack(Vec2::new(530.0 + i as f32 * 120.0, 950.0), true);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        for (i, age) in [0.1f32, 1.8, 2.3].into_iter().enumerate() {
            let mut fx = Fx::default();
            let p = Vec2::new(780.0 + i as f32 * 70.0, 970.0);
            fx.welt(p, Vec2::new(1.0, -0.4), i == 0);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        list.shapes.extend(fx_list.shapes);

        assert!(all_finite(&list));
        if std::env::var("WHIP_SHEET").is_ok() {
            let out = std::env::var("WHIP_SHEET_OUT").unwrap_or_else(|_| "whip_sheet.svg".into());
            std::fs::write(&out, svg::document(&list, w, h, 720.0, 1.4)).unwrap();
        }
    }

    /// Число фигур на плеть (для отчёта).
    #[test]
    fn shape_counts() {
        for (tone, charge) in [(Leather::Natural, 0.0), (Leather::Black, 0.0), (Leather::Black, 1.0)] {
            let n = whip(tone, Vec2::new(300.0, 200.0), 0.0, 1.0, charge, 1.0, light_left()).shapes.len();
            println!("{tone:?} charge {charge}: {n} shapes");
        }
    }
}
