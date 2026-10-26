//! Палитры плети. Значения — из согласованного эскиза `design/whip_v2.html`.

use crate::settings::Leather;

const fn hex(rgb: u32, a: f32) -> [f32; 4] {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        a,
    ]
}

/// Палитра v2 из тона кожи. Объём (тень, свет, блик, пряди) шейдер строит сам из базового цвета,
/// здесь — только базовые цвета деталей.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeatherPalette {
    /// Кожа хлыста, рукояти, темляка, узлов.
    pub leather: [f32; 4],
    /// Фол — гладкая кожа светлее основной.
    pub fall: [f32; 4],
    /// Металл: латунь у светлых тонов, воронёная сталь у тёмных.
    pub metal: [f32; 4],
    /// Светлая кромка для тёмной кожи (иначе на тёмном рабочем столе плети не видно).
    pub rim: Option<[f32; 4]>,
    /// Хлопушка.
    pub cracker: [f32; 4],
}

/// Как `makeLeather` в design/whip_v2.html: оттенки по светлоте в HSL.
pub fn leather_palette(l: Leather) -> LeatherPalette {
    let [r, g, b] = l.rgb().map(|c| c as f32 / 255.0);
    let (h, s, li) = rgb_to_hsl(r, g, b);
    let dark = li < 0.16;
    LeatherPalette {
        leather: [r, g, b, 1.0],
        fall: hsl_to_rgba(h, s, li * 1.3),
        metal: if dark { hex(0x565c67, 1.0) } else { hex(0xa9853b, 1.0) },
        rim: dark.then(|| hsl_to_rgba(h, s * 0.4, 0.45)),
        cracker: hex(0xf3ece0, 1.0),
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    if d <= 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    (h * 60.0, s, l)
}

fn hsl_to_rgba(h: f32, s: f32, l: f32) -> [f32; 4] {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [r + m, g + m, b + m, 1.0]
}
