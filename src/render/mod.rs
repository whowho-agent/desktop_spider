//! Рендер. Все координаты в логических точках окна (левый верх = 0,0, Y вниз).
mod batch;
mod renderer;

pub use batch::{DrawList, Shape};
#[cfg(test)]
pub use batch::kind;
pub use renderer::Renderer;
