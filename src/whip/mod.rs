//! Плеть: физика (rope, hand), эффекты (fx), палитры (skin), отрисовка (draw).
//! Эталон — `design/whip_demo.html` и PLAN.md, раздел 8.
//!
//! Владельцы файлов: mod.rs, rope.rs, hand.rs — агент физики (W2a);
//! draw.rs, fx.rs, skin.rs — агент отрисовки (W2b). Публичный API ниже — контракт для `app.rs`.

pub mod draw;
pub mod fx;
pub mod hand;
pub mod rope;
pub mod skin;

use glam::Vec2;

use crate::input::WhipInput;
use crate::render::DrawList;
use crate::settings::WhipSkin;

/// Длина рукояти от точки курсора до кончика: TIP_X − GRIP_X.
pub const HANDLE_REACH: f32 = 38.0;

/// Поза для отрисовки (только чтение).
pub struct WhipPose<'a> {
    /// Положение кисти (курсор + вынос кисти при ударе/раскрутке).
    pub hand: Vec2,
    /// Угол рукояти, рад (0 = вправо, Y вниз).
    pub angle: f32,
    /// Точки хлыста от кончика рукояти до хлопушки (26 звеньев → 27 точек).
    pub rope: &'a [Vec2],
    /// Заряд раскрутки 0..1 (0 — кольцо не рисуется).
    pub charge: f32,
}

/// Что произошло за `update`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WhipEvent {
    /// Щелчок кончика в точке `pos`, `dir` — направление кончика; `big` — захлёст.
    Crack { pos: Vec2, dir: Vec2, big: bool },
    /// Захлёст поймал цель.
    Caught,
    /// Пока поймана: цель должна быть в `pos` (плеть подтягивает её к курсору).
    Hold { pos: Vec2 },
    /// Цель отпущена (≈ 1 с после поимки).
    Released,
}

/// Плеть. При щелчке сама вызывает `fx.crack`; след (`fx.welt`) добавляет `app.rs` в режиме «Просто плеть».
pub struct Whip {
    fx: fx::Fx,
}

impl Whip {
    pub fn new(_cursor: Vec2) -> Self {
        Self { fx: fx::Fx::default() }
    }

    /// Сбросить хлыст висящим под курсором (при включении плети).
    pub fn reset(&mut self, _cursor: Vec2) {}

    /// Шаг физики. `dt` — шаг сима (1/120), внутри 2 подшага по 1/240.
    /// `target` — позиция паука для захлёста (`None` в режиме «Просто плеть»).
    /// События дописываются в `events`.
    pub fn update(&mut self, _dt: f32, _cursor: Vec2, _input: WhipInput, _target: Option<Vec2>, _events: &mut Vec<WhipEvent>) {}

    /// Идёт ли анимация (удар, раскрутка, захват или хлыст ещё качается).
    pub fn is_animating(&self) -> bool {
        false
    }

    pub fn fx_mut(&mut self) -> &mut fx::Fx {
        &mut self.fx
    }

    /// Рисует плеть (если `visible`) и эффекты.
    pub fn draw(&self, _list: &mut DrawList, _skin: WhipSkin, _visible: bool) {}
}
