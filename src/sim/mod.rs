//! Симуляция существа. Координаты в логических точках, Y вниз.
//! Контракт: `Creature::new(&Preset, Vec2)`, `update(dt, target)`, `draw(&mut DrawList, debug)`.
mod body;
mod creature;
mod draw;
mod gait;
mod ik;
mod leg;

pub use creature::Creature;
