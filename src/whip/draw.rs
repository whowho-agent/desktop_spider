//! Отрисовка рукояти и хлыста в DrawList. Реализует агент отрисовки (W2b).

use super::skin::Palette;
use super::WhipPose;
use crate::render::DrawList;

/// Рисует плеть в позе `pose` палитрой `pal`. Геометрия — как в `design/whip_demo.html`:
/// рукоять в локальных координатах вдоль +x, курсор держит её в точке GRIP_X = 8,
/// кончик рукояти TIP_X = 46 = `pose.rope[0]`; хлыст сужается 3,4 → 1,1 pt (полуширина) до 82% длины,
/// дальше фол 0,7; насечки плетения через звено; хлопушка — 4 коротких луча на кончике.
/// `pose.charge > 0` — пунктирное кольцо заряда вокруг курсора радиусом 38 + 6·charge.
pub fn draw_whip(_list: &mut DrawList, _pose: &WhipPose, _pal: &Palette) {}
