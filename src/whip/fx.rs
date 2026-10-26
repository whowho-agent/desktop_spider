//! Эффекты: вспышка щелчка и следы ударов. Реализует агент отрисовки (W2b).

use glam::Vec2;

use crate::render::DrawList;

/// Живые эффекты на оверлее. Время жизни: вспышка ≈ 0,4 с, след 2,5 с (последняя секунда — затухание).
#[derive(Default)]
pub struct Fx {}

impl Fx {
    /// Вспышка, 8 лучей, расходящееся кольцо. `big` — усиленный (захлёст): крупнее в 1,6 раза. Без текста.
    pub fn crack(&mut self, _pos: Vec2, _big: bool) {}

    /// Красный след вдоль `dir` (направление кончика хлыста) — только в режиме «Просто плеть».
    pub fn welt(&mut self, _pos: Vec2, _dir: Vec2, _big: bool) {}

    pub fn update(&mut self, _dt: f32) {}

    /// Есть ли живые эффекты (нужна ли перерисовка).
    pub fn is_active(&self) -> bool {
        false
    }

    pub fn clear(&mut self) {}

    pub fn draw(&self, _list: &mut DrawList) {}
}
