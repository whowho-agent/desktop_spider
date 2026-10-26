use glam::Vec2;

/// Один примитив = один инстанс в GPU.
/// - капсула: отрезок `a..b` толщиной `2 * radius`, `ring_width == 0`;
/// - круг: `a == b`, `ring_width == 0`;
/// - кольцо: `a == b`, линия толщиной `2 * ring_width` на расстоянии `radius` от центра.
/// Цвет НЕ premultiplied (обычный RGBA), рендер сам домножает.
///
/// `style = [вид, параметр]` — как закрашивать (см. `kind::*`). Свет для объёмных видов —
/// общий для кадра: `Renderer::set_lighting(направление на свет, сила блика)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Shape {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub radius: f32,
    pub ring_width: f32,
    pub style: [f32; 2],
    pub color: [f32; 4],
}

/// Виды фигур (`Shape::style[0]`).
pub mod kind {
    /// Плоская заливка `color` (отрезки, точки, кольца паука и эффектов).
    pub const FLAT: f32 = 0.0;
    /// Кожаная трубка: цилиндрическое затенение из базового цвета (тень → основа → свет → блик со стороны света).
    pub const TUBE: f32 = 1.0;
    /// Трубка с плетением: диагональные пряди двух тонов. Параметр — шаг прядей вдоль оси, pt.
    pub const BRAID: f32 = 2.0;
    /// Металлическая трубка: резче контраст, яркий узкий блик.
    pub const METAL: f32 = 3.0;
    /// Сфера (`a == b`): металл/кожа с бликом, смещённым к свету.
    pub const SPHERE: f32 = 4.0;
    /// Мягкая тень: капсула с размытым краем. Параметр — ширина размытия, pt.
    pub const SOFT: f32 = 5.0;
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

    /// Кожаная трубка полной толщины `width` из базового цвета `color`.
    pub fn tube(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4]) {
        self.push_styled(a, b, width * 0.5, color, kind::TUBE, 0.0);
    }

    /// Плетёная трубка: пряди с шагом около `period` pt вдоль оси. Шаг подгоняется так, чтобы
    /// в отрезок укладывалось целое число прядей: узор отсчитывается от `a`, и тогда соседние
    /// отрезки хлыста стыкуются без швов.
    pub fn braid(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4], period: f32) {
        let len = a.distance(b);
        let fitted = if len > 1e-3 && period > 0.0 { len / (len / period).round().max(1.0) } else { period };
        self.push_styled(a, b, width * 0.5, color, kind::BRAID, fitted);
    }

    /// Металлическая трубка (кольца, навершие сбоку).
    pub fn metal(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4]) {
        self.push_styled(a, b, width * 0.5, color, kind::METAL, 0.0);
    }

    /// Металлическая сфера с бликом к свету (навершие).
    pub fn sphere(&mut self, c: Vec2, radius: f32, color: [f32; 4]) {
        self.push_styled(c, c, radius, color, kind::SPHERE, 0.0);
    }

    /// Матовая кожаная сфера (узлы): параметр 1 — без металлического блика.
    pub fn leather_sphere(&mut self, c: Vec2, radius: f32, color: [f32; 4]) {
        self.push_styled(c, c, radius, color, kind::SPHERE, 1.0);
    }

    /// Мягкая тень: капсула толщиной `width`, край размыт на `softness` pt.
    pub fn soft(&mut self, a: Vec2, b: Vec2, width: f32, color: [f32; 4], softness: f32) {
        self.push_styled(a, b, width * 0.5, color, kind::SOFT, softness);
    }

    fn push(&mut self, a: Vec2, b: Vec2, radius: f32, ring_width: f32, color: [f32; 4]) {
        self.shapes.push(Shape {
            a: a.to_array(),
            b: b.to_array(),
            radius,
            ring_width,
            style: [kind::FLAT, 0.0],
            color,
        });
    }

    fn push_styled(&mut self, a: Vec2, b: Vec2, radius: f32, color: [f32; 4], k: f32, param: f32) {
        self.shapes.push(Shape { a: a.to_array(), b: b.to_array(), radius, ring_width: 0.0, style: [k, param], color });
    }
}
