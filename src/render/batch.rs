use glam::Vec2;

/// Один примитив = один инстанс в GPU.
/// - капсула: отрезок `a..b` толщиной `2 * radius`, `ring_width == 0`;
/// - круг: `a == b`, `ring_width == 0`;
/// - кольцо: `a == b`, линия толщиной `2 * ring_width` на расстоянии `radius` от центра.
/// Цвет НЕ premultiplied (обычный RGBA), рендер сам домножает.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Shape {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub radius: f32,
    pub ring_width: f32,
    pub _pad: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Default)]
pub struct DrawList {
    pub shapes: Vec<Shape>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.shapes.clear();
    }

    /// Отрезок полной толщины `width`.
    pub fn segment(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4]) {
        self.push(a, b, width * 0.5, 0.0, color);
    }

    /// Залитый круг.
    pub fn dot(&mut self, c: Vec2, radius: f32, color: [f32; 4]) {
        self.push(c, c, radius, 0.0, color);
    }

    /// Окружность радиуса `radius` с линией полной толщины `width`.
    pub fn ring(&mut self, c: Vec2, radius: f32, width: f32, color: [f32; 4]) {
        self.push(c, c, radius, width * 0.5, color);
    }

    fn push(&mut self, a: Vec2, b: Vec2, radius: f32, ring_width: f32, color: [f32; 4]) {
        self.shapes.push(Shape {
            a: a.to_array(),
            b: b.to_array(),
            radius,
            ring_width,
            _pad: [0.0; 2],
            color,
        });
    }
}
