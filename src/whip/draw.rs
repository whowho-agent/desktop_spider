//! Отрисовка рукояти и хлыста в DrawList. Реализует агент отрисовки (W2b).
//!
//! Рендер умеет только капсулы, круги и кольца, поэтому всё собирается из них:
//! «кромка + заливка» = капсула цвета кромки чуть толще, поверх — капсула заливки.

use glam::Vec2;

use super::skin::Palette;
use super::WhipPose;
use crate::render::DrawList;

/// Точка курсора на рукояти (локальная x).
pub const GRIP_X: f32 = 8.0;
/// Кончик рукояти (локальная x) = начало хлыста.
pub const TIP_X: f32 = 46.0;

/// Цвет кольца заряда (#ffd36b).
const ACCENT: [f32; 3] = [1.0, 0.827, 0.42];

/// Рисует плеть в позе `pose` палитрой `pal`. Геометрия — как в `design/whip_demo.html`:
/// рукоять в локальных координатах вдоль +x, курсор держит её в точке GRIP_X = 8,
/// кончик рукояти TIP_X = 46 = `pose.rope[0]`; хлыст сужается 3,4 → 1,1 pt (полуширина) до 82% длины,
/// дальше фол 0,7; насечки плетения через звено; хлопушка — 4 коротких луча на кончике.
/// `pose.charge > 0` — пунктирное кольцо заряда вокруг курсора радиусом 38 + 6·charge.
pub fn draw_whip(list: &mut DrawList, pose: &WhipPose, pal: &Palette) {
    // Порядок как в эскизе: кольцо заряда, хлыст, рукоять поверх.
    if pose.charge > 0.0 {
        draw_charge(list, pose.hand, pose.charge.min(1.0), pose.scale);
    }
    draw_lash(list, pose.rope, pal, pose.scale);
    draw_handle(list, pose.hand, pose.angle, pal, pose.scale);
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

/// Рукоять. `scale` — размер плети (и увеличение на тестовой схеме).
pub fn draw_handle(list: &mut DrawList, hand: Vec2, angle: f32, pal: &Palette, scale: f32) {
    let rot = Vec2::from_angle(angle);
    let to_world = |x: f32, y: f32| hand + rot.rotate(Vec2::new((x - GRIP_X) * scale, y * scale));
    let seg = |list: &mut DrawList, a: (f32, f32), b: (f32, f32), w: f32, c: [f32; 4]| {
        list.segment(to_world(a.0, a.1), to_world(b.0, b.1), w * scale, c);
    };

    // Переход (трапеция 34→46, полувысота 3,5→1,6, обводка 1,2):
    // три продольные капсулы — сверху, по центру, снизу.
    for (pad, col) in [(0.6, pal.edge), (-0.6, pal.handle)] {
        let r: f32 = if pad > 0.0 { 2.0 } else { 1.3 };
        let h0 = (3.5 + pad - r).max(0.0);
        let h1 = (1.6 + pad - r).max(0.0);
        let (x0, x1) = (34.0 + r * 0.5, TIP_X + pad - r);
        for k in [-1.0, 0.0, 1.0] {
            seg(list, (x0, h0 * k), (x1, h1 * k), 2.0 * r, col);
        }
    }

    // Обмотка — скруглённый прямоугольник −2..34 × ±3,6 (rx 2,5, обводка 1,3):
    // объединение двух капсул на y = ±(h − r) даёт ровно такой прямоугольник.
    let (gx0, gx1, gh, rx) = (-2.0, 34.0, 3.6, 2.5);
    for (pad, col) in [(0.65, pal.edge), (-0.65, pal.handle)] {
        let r = rx + pad;
        let y = gh - rx;
        for k in [-1.0, 1.0] {
            seg(list, (gx0 + rx, y * k), (gx1 - rx, y * k), 2.0 * r, col);
        }
    }

    // Диагональные полосы плетения через 4,5.
    let mut x = 1.0;
    while x < 33.0 {
        seg(list, (x + 0.35, -2.85), (x + 2.65, 2.85), 0.9, pal.braid);
        x += 4.5;
    }

    // Навершие: круг r 4,6 с обводкой цвета плетения и тёмная точка в центре.
    let knob = to_world(-6.0, 0.0);
    list.dot(knob, 5.0 * scale, opaque(pal.braid));
    list.dot(knob, 4.2 * scale, pal.knob);
    list.dot(knob, 1.6 * scale, pal.handle);
}

/// Хлыст по точкам `pts` (pts[0] — кончик рукояти). `k` — масштаб толщины (схема в тестах).
pub fn draw_lash(list: &mut DrawList, pts: &[Vec2], pal: &Palette, k: f32) {
    let n = pts.len();
    if n < 2 {
        return;
    }
    const FALL: f32 = 0.82;
    let half_w = |i: usize| {
        let s = i as f32 / (n - 1) as f32;
        (if s < FALL { lerp(3.4, 1.1, s / FALL) } else { 0.7 }) * k
    };
    // Касательная в точке — по соседям, как в эскизе.
    let tangent = |i: usize| (pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)]).normalize_or(Vec2::X);

    // Звено i→i+1 берёт полуширину своей начальной точки, на фоле — 0,7.
    let link_w = |i: usize| {
        let s1 = (i + 1) as f32 / (n - 1) as f32;
        if s1 > FALL { half_w(n - 1) } else { 0.5 * (half_w(i) + half_w(i + 1)) }
    };

    // Кромка: все звенья, на 0,3 pt шире.
    for i in 0..n - 1 {
        list.segment(pts[i], pts[i + 1], 2.0 * (link_w(i) + 0.3 * k), pal.lash_edge);
    }
    // Заливка поверх: на 0,3 pt уже (как центрированная обводка 0,6).
    for i in 0..n - 1 {
        let w = (link_w(i) - 0.3 * k).max(0.3 * k);
        list.segment(pts[i], pts[i + 1], 2.0 * w, pal.lash);
    }
    // Насечки плетения: на каждой второй точке до фола, поперёк с наклоном.
    for i in (1..n).step_by(2) {
        let s = i as f32 / (n - 1) as f32;
        if s >= FALL {
            break;
        }
        let d = tangent(i);
        let nrm = Vec2::new(-d.y, d.x);
        let bw = half_w(i) * 0.9;
        // Капсула длиннее на ширину линии — укорачиваем концы.
        let a = pts[i] + nrm * bw - d * 1.2 * k;
        let b = pts[i] - nrm * bw + d * 1.2 * k;
        let lw = 0.8 * k;
        let shrink = (b - a).normalize_or_zero() * lw * 0.5;
        list.segment(a + shrink, b - shrink, lw, pal.lash_braid);
    }
    // Хлопушка: 4 луча от кончика.
    let tip = pts[n - 1];
    let ang = (tip - pts[n - 2]).to_angle();
    for a in [-0.5f32, -0.15, 0.2, 0.55] {
        let l = (6.0 + a.abs() * 3.0) * k;
        list.segment(tip, tip + Vec2::from_angle(ang + a) * l, 1.0 * k, pal.tuft);
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn opaque(c: [f32; 4]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

/// Выгрузка DrawList в SVG — проверка картинки без окна.
#[cfg(test)]
pub(crate) mod svg {
    use crate::render::DrawList;
    use std::fmt::Write;

    fn rgba(c: [f32; 4]) -> String {
        format!(
            "rgb({},{},{})\" {}-opacity=\"{:.3}",
            (c[0] * 255.0).round(),
            (c[1] * 255.0).round(),
            (c[2] * 255.0).round(),
            "%K%",
            c[3]
        )
    }

    /// Фигуры списка как SVG-элементы (в порядке вставки).
    pub fn shapes(list: &DrawList) -> String {
        let mut s = String::new();
        for sh in &list.shapes {
            let col = rgba(sh.color);
            let is_point = sh.a == sh.b;
            if is_point && sh.ring_width > 0.0 {
                let _ = writeln!(
                    s,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"none\" stroke-width=\"{}\" stroke=\"{}\"/>",
                    sh.a[0], sh.a[1], sh.radius, 2.0 * sh.ring_width, col.replace("%K%", "stroke")
                );
            } else if is_point {
                let _ = writeln!(
                    s,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>",
                    sh.a[0], sh.a[1], sh.radius, col.replace("%K%", "fill")
                );
            } else {
                let _ = writeln!(
                    s,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke-linecap=\"round\" stroke-width=\"{}\" stroke=\"{}\"/>",
                    sh.a[0], sh.a[1], sh.b[0], sh.b[1], 2.0 * sh.radius, col.replace("%K%", "stroke")
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
    use crate::whip::fx::Fx;
    use crate::whip::skin::{BLACK, LIGHT};

    const N: usize = crate::whip::rope::N;
    const SEG: f32 = 7.4;

    /// Кубическая Безье, пересэмплированная на N звеньев примерно равной длины.
    fn bezier_rope(p: [Vec2; 4], links: usize) -> Vec<Vec2> {
        let at = |t: f32| {
            let u = 1.0 - t;
            p[0] * u * u * u + p[1] * 3.0 * u * u * t + p[2] * 3.0 * u * t * t + p[3] * t * t * t
        };
        let dense: Vec<Vec2> = (0..=400).map(|i| at(i as f32 / 400.0)).collect();
        let mut cum = vec![0.0];
        for w in dense.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let total = *cum.last().unwrap();
        (0..=links)
            .map(|k| {
                let target = total * k as f32 / links as f32;
                let j = cum.partition_point(|&c| c < target).clamp(1, dense.len() - 1);
                let t = (target - cum[j - 1]) / (cum[j] - cum[j - 1]).max(1e-6);
                dense[j - 1].lerp(dense[j], t)
            })
            .collect()
    }

    fn tip_of(hand: Vec2, angle: f32) -> Vec2 {
        hand + Vec2::from_angle(angle) * (TIP_X - GRIP_X)
    }

    /// Хлыст из точки `tip` под углом, с плавным изгибом `bend` (рад на всю длину).
    fn arc_rope(tip: Vec2, angle: f32, bend: f32) -> Vec<Vec2> {
        let mut pts = vec![tip];
        let mut a = angle;
        for _ in 1..N {
            let p = *pts.last().unwrap() + Vec2::from_angle(a) * SEG;
            pts.push(p);
            a += bend / (N - 1) as f32;
        }
        pts
    }

    fn all_finite(list: &DrawList) -> bool {
        list.shapes.iter().all(|s| {
            s.a.iter().chain(&s.b).chain(&s.color).all(|v| v.is_finite())
                && s.radius.is_finite()
                && s.radius >= 0.0
                && s.ring_width.is_finite()
        })
    }

    #[test]
    fn draw_whip_finite_for_both_palettes() {
        for pal in [&LIGHT, &BLACK] {
            for (angle, bend, charge) in [(0.0, 0.0, 0.0), (-0.9, 2.0, 0.5), (2.5, -3.0, 1.0)] {
                let hand = Vec2::new(300.0, 200.0);
                let rope = arc_rope(tip_of(hand, angle), angle, bend);
                let pose = WhipPose { hand, angle, rope: &rope, charge, scale: 1.0 };
                let mut list = DrawList::default();
                draw_whip(&mut list, &pose, pal);
                assert!(list.shapes.len() > 60);
                assert!(all_finite(&list));
            }
            // Вырожденный хлыст: все точки совпали — без NaN.
            let rope = vec![Vec2::new(10.0, 10.0); N];
            let pose = WhipPose { hand: Vec2::ZERO, angle: 0.0, rope: &rope, charge: 0.0, scale: 1.0 };
            let mut list = DrawList::default();
            draw_whip(&mut list, &pose, pal);
            assert!(all_finite(&list));
        }
    }

    /// Лист для просмотра: WHIP_SHEET=1 cargo test whip_sheet -- пишет SVG.
    #[test]
    fn whip_sheet() {
        let mut list = DrawList::default();
        let (w, h) = (1000.0, 760.0);

        // 1. Схема как в эскизе: рукоять ×2,2, хлыст толщины ×2 по кубической Безье.
        for (pal, oy) in [(&LIGHT, 90.0), (&BLACK, 230.0)] {
            let s = 2.2;
            let ox = 70.0;
            let hand = Vec2::new(ox + GRIP_X * s, oy);
            draw_handle(&mut list, hand, 0.0, pal, s);
            let o = Vec2::new(ox, oy);
            let p = [
                o + Vec2::new(TIP_X * s, 0.0),
                o + Vec2::new(TIP_X * s + 160.0, -10.0),
                o + Vec2::new(TIP_X * s + 300.0, 110.0),
                o + Vec2::new(TIP_X * s + 520.0, 40.0),
            ];
            let pts = bezier_rope(p, 40);
            // Как в эскизе: хлыст до рукояти, но рукоять уже нарисована — перерисуем её сверху.
            draw_lash(&mut list, &pts, pal, 2.0);
            draw_handle(&mut list, hand, 0.0, pal, s);
        }

        // 2. Натуральный размер: прямо, изогнуто, висит, с зарядом.
        let poses: [(Vec2, f32, f32, f32); 4] = [
            (Vec2::new(60.0, 400.0), 0.0, 0.0, 0.0),
            (Vec2::new(330.0, 440.0), -0.6, 2.2, 0.0),
            (Vec2::new(80.0, 470.0), 1.2, 0.6, 0.0),
            (Vec2::new(620.0, 430.0), -2.2, -2.5, 1.0),
        ];
        for (k, pal) in [&LIGHT, &BLACK].into_iter().enumerate() {
            for &(hand, angle, bend, charge) in &poses {
                let hand = hand + Vec2::new(0.0, k as f32 * 150.0);
                let rope = arc_rope(tip_of(hand, angle), angle, bend);
                let pose = WhipPose { hand, angle, rope: &rope, charge, scale: 1.0 };
                draw_whip(&mut list, &pose, pal);
            }
        }
        // Та же светлая/чёрная поверх светлого окна.
        for (k, pal) in [&LIGHT, &BLACK].into_iter().enumerate() {
            let hand = Vec2::new(760.0, 420.0 + k as f32 * 150.0);
            let rope = arc_rope(tip_of(hand, 0.3), 0.3, 1.5);
            draw_whip(&mut list, &WhipPose { hand, angle: 0.3, rope: &rope, charge: 0.6, scale: 1.0 }, pal);
        }

        // 3. Щелчок в разные моменты и усиленный, след.
        let mut fx_list = DrawList::default();
        let ages = [0.03f32, 0.08, 0.15, 0.25, 0.35];
        for (i, age) in ages.into_iter().enumerate() {
            let mut fx = Fx::default();
            fx.crack(Vec2::new(60.0 + i as f32 * 95.0, 680.0), false);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        for (i, age) in [0.06f32, 0.15].into_iter().enumerate() {
            let mut fx = Fx::default();
            fx.crack(Vec2::new(530.0 + i as f32 * 120.0, 680.0), true);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        for (i, age) in [0.1f32, 1.8, 2.3].into_iter().enumerate() {
            let mut fx = Fx::default();
            let p = Vec2::new(780.0 + i as f32 * 70.0, 700.0);
            fx.welt(p, Vec2::new(1.0, -0.4), i == 0);
            fx.update(age);
            fx.draw(&mut fx_list);
        }
        list.shapes.extend(fx_list.shapes);

        assert!(all_finite(&list));
        if std::env::var("WHIP_SHEET").is_ok() {
            let out = std::env::var("WHIP_SHEET_OUT").unwrap_or_else(|_| "whip_sheet.svg".into());
            std::fs::write(&out, svg::document(&list, w, h, 700.0, 1.4)).unwrap();
        }
    }
}
