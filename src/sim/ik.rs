//! IK: аналитика для двух сегментов, FABRIK для цепочек из 3+.
use glam::Vec2;

const EPS: f32 = 1e-3;

/// Возвращает (колено, достигнутая ступня). `bend` — знак выгиба:
/// колено смещается вдоль `perp(dir) = (-dir.y, dir.x) * bend`.
pub fn two_bone_ik(hip: Vec2, target: Vec2, l1: f32, l2: f32, bend: f32) -> (Vec2, Vec2) {
    let to = target - hip;
    let dir = to.try_normalize().unwrap_or(Vec2::X);
    let d = to
        .length()
        .clamp((l1 - l2).abs() + EPS, (l1 + l2) * 0.999);
    // Теорема косинусов: проекция колена на ось hip→foot и высота над ней.
    let a = (l1 * l1 - l2 * l2 + d * d) / (2.0 * d);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let perp = Vec2::new(-dir.y, dir.x) * bend.signum();
    let knee = hip + dir * a + perp * h;
    (knee, hip + dir * d)
}

/// Допуск попадания FABRIK в цель.
const FABRIK_TOL: f32 = 0.05;

/// Направление `from → to`; при совпадении точек — `fallback` (без NaN).
fn dir_or(from: Vec2, to: Vec2, fallback: Vec2) -> Vec2 {
    (to - from).try_normalize().unwrap_or(fallback)
}

/// FABRIK: `joints` (N+1 точек, бедро..ступня) — начальное приближение
/// (обычно решение прошлого кадра) и результат. `lengths` — N длин сегментов.
/// Недостижимая цель → прямая цепочка, направленная на цель.
pub fn fabrik(joints: &mut [Vec2], lengths: &[f32], root: Vec2, target: Vec2, iterations: usize) {
    let n = joints.len();
    debug_assert_eq!(n, lengths.len() + 1);
    if n == 0 {
        return;
    }
    joints[0] = root;
    if n == 1 {
        return;
    }
    let total: f32 = lengths.iter().sum();
    let to = target - root;
    if to.length() >= total {
        let fallback = dir_or(root, joints[n - 1], Vec2::X);
        let dir = to.try_normalize().unwrap_or(fallback);
        for i in 0..lengths.len() {
            joints[i + 1] = joints[i] + dir * lengths[i];
        }
        return;
    }
    // Запасное направление при слипшихся суставах: от корня к цели.
    let fb = to.try_normalize().unwrap_or(Vec2::X);
    for _ in 0..iterations {
        // Обратный проход: от цели к корню.
        joints[n - 1] = target;
        for i in (0..n - 1).rev() {
            joints[i] = joints[i + 1] + dir_or(joints[i + 1], joints[i], -fb) * lengths[i];
        }
        // Прямой проход: от корня к цели — длины точные, корень на месте.
        joints[0] = root;
        for i in 0..n - 1 {
            joints[i + 1] = joints[i] + dir_or(joints[i], joints[i + 1], fb) * lengths[i];
        }
        if joints[n - 1].distance_squared(target) < FABRIK_TOL * FABRIK_TOL {
            break;
        }
    }
}

/// Один проход лапласового сглаживания внутренних суставов (концы не трогаем).
/// Сглаживается отклонение от `base` (гладкий шаблон того же размера), поэтому
/// зигзаги гаснут, а сама дуга не сжимается. Длины потом восстанавливает `fabrik`.
pub fn smooth(joints: &mut [Vec2], base: &[Vec2], k: f32) {
    debug_assert_eq!(joints.len(), base.len());
    for i in 1..joints.len().saturating_sub(1) {
        let e = |j: usize, p: Vec2| p - base[j];
        let mid = (e(i - 1, joints[i - 1]) + e(i + 1, joints[i + 1])) * 0.5;
        joints[i] = base[i] + e(i, joints[i]).lerp(mid, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_lengths(j: &[Vec2], lengths: &[f32]) {
        for (i, l) in lengths.iter().enumerate() {
            let d = j[i].distance(j[i + 1]);
            assert!((d - l).abs() < 1e-3, "segment {i}: {d} vs {l}");
        }
    }

    fn zigzag(root: Vec2, lengths: &[f32]) -> Vec<Vec2> {
        let mut j = vec![root];
        for (i, l) in lengths.iter().enumerate() {
            let a = if i % 2 == 0 { 0.4 } else { -0.4 };
            let p = *j.last().unwrap() + Vec2::from_angle(a) * *l;
            j.push(p);
        }
        j
    }

    #[test]
    fn fabrik_reaches_and_keeps_lengths() {
        for lengths in [vec![20.0, 25.0, 18.0], vec![7.0; 14]] {
            let root = Vec2::new(5.0, -3.0);
            let mut j = zigzag(root, &lengths);
            for t in [Vec2::new(30.0, 20.0), Vec2::new(-20.0, 15.0), Vec2::new(10.0, 2.0)] {
                fabrik(&mut j, &lengths, root, t, 30);
                assert!(j.iter().all(|p| p.is_finite()));
                assert_eq!(j[0], root);
                check_lengths(&j, &lengths);
                assert!(j.last().unwrap().distance(t) < 0.5, "miss {t}");
            }
        }
    }

    #[test]
    fn fabrik_unreachable_is_straight() {
        let lengths = [20.0, 25.0, 18.0];
        let root = Vec2::ZERO;
        let mut j = zigzag(root, &lengths);
        let t = Vec2::new(0.0, 500.0);
        fabrik(&mut j, &lengths, root, t, 10);
        check_lengths(&j, &lengths);
        for p in &j[1..] {
            assert!(p.x.abs() < 1e-3 && p.y > 0.0);
        }
        assert!((j[3].y - 63.0).abs() < 1e-3);
    }

    #[test]
    fn fabrik_degenerate_no_nan() {
        let lengths = [10.0, 10.0, 10.0];
        // Все суставы в одной точке, цель — в корне.
        let mut j = vec![Vec2::ZERO; 4];
        fabrik(&mut j, &lengths, Vec2::ZERO, Vec2::ZERO, 10);
        assert!(j.iter().all(|p| p.is_finite()));
        check_lengths(&j, &lengths);
        // Нулевые длины и цель далеко.
        let mut j = vec![Vec2::ZERO; 4];
        fabrik(&mut j, &[0.0; 3], Vec2::ZERO, Vec2::new(5.0, 5.0), 10);
        assert!(j.iter().all(|p| p.is_finite()));
    }

    #[test]
    fn smooth_keeps_ends_and_base() {
        let lengths = [7.0; 6];
        let mut j = zigzag(Vec2::ZERO, &lengths);
        let (a, b) = (j[0], j[6]);
        let flat = vec![Vec2::ZERO; 7];
        smooth(&mut j, &flat, 0.5);
        assert_eq!((j[0], j[6]), (a, b));
        // Цепочка, совпадающая с шаблоном, не меняется.
        let base = zigzag(Vec2::ZERO, &lengths);
        let mut j = base.clone();
        smooth(&mut j, &base, 0.5);
        assert_eq!(j, base);
    }

    #[test]
    fn lengths_preserved() {
        let hip = Vec2::new(10.0, -5.0);
        for t in [Vec2::new(40.0, 20.0), Vec2::new(-200.0, 3.0), Vec2::new(10.0, -5.0)] {
            for bend in [1.0, -1.0] {
                let (k, f) = two_bone_ik(hip, t, 36.0, 38.0, bend);
                assert!((k.distance(hip) - 36.0).abs() < 1e-3);
                assert!((f.distance(k) - 38.0).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn reachable_target_hit() {
        let hip = Vec2::ZERO;
        let t = Vec2::new(30.0, 40.0);
        let (_, f) = two_bone_ik(hip, t, 36.0, 38.0, 1.0);
        assert!(f.distance(t) < 1e-4);
    }

    #[test]
    fn bend_sign_picks_side() {
        // Ось вдоль +X: perp = (0, 1), т.е. bend=+1 даёт колено с y > 0.
        let (k, _) = two_bone_ik(Vec2::ZERO, Vec2::new(50.0, 0.0), 36.0, 38.0, 1.0);
        assert!(k.y > 0.0);
        let (k, _) = two_bone_ik(Vec2::ZERO, Vec2::new(50.0, 0.0), 36.0, 38.0, -1.0);
        assert!(k.y < 0.0);
    }
}
