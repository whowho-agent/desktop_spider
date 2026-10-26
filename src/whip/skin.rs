//! Палитры плети. Значения — из согласованного эскиза `design/whip_demo.html`.

use crate::settings::WhipSkin;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Заливка рукояти и перехода.
    pub handle: [f32; 4],
    /// Кромка рукояти.
    pub edge: [f32; 4],
    /// Навершие.
    pub knob: [f32; 4],
    /// Обмотка рукояти.
    pub braid: [f32; 4],
    /// Хлыст: заливка, кромка, насечки плетения.
    pub lash: [f32; 4],
    pub lash_edge: [f32; 4],
    pub lash_braid: [f32; 4],
    /// Хлопушка на кончике.
    pub tuft: [f32; 4],
}

const fn hex(rgb: u32, a: f32) -> [f32; 4] {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        a,
    ]
}

pub const LIGHT: Palette = Palette {
    handle: hex(0x2a1a10, 1.0),
    edge: hex(0xd89a5a, 1.0),
    knob: hex(0xd89a5a, 1.0),
    braid: hex(0xf3cf9a, 0.75),
    lash: hex(0xc98a4c, 1.0),
    lash_edge: hex(0xd89a5a, 1.0),
    lash_braid: hex(0xf3cf9a, 0.7),
    tuft: hex(0xffffff, 1.0),
};

/// У чёрной светлая кромка, иначе на тёмном рабочем столе её не видно.
pub const BLACK: Palette = Palette {
    handle: hex(0x0c0c0f, 1.0),
    edge: hex(0x6c6c7a, 1.0),
    knob: hex(0x9a9aa8, 1.0),
    braid: hex(0x5a5a68, 0.75),
    lash: hex(0x141418, 1.0),
    lash_edge: hex(0x76768a, 1.0),
    lash_braid: hex(0x4a4a58, 0.7),
    tuft: hex(0xe8e8ee, 1.0),
};

pub fn palette(skin: WhipSkin) -> &'static Palette {
    match skin {
        WhipSkin::Light => &LIGHT,
        WhipSkin::Black => &BLACK,
    }
}
