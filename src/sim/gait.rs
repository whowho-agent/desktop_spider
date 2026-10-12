//! Походка: соседние ноги не шагают одновременно (любое число пар).
use super::body::Body;
use super::leg::Leg;
use crate::config::LegParams;

/// Ниже этой скорости тела паук «доводит» стойку мелкими шагами.
const SETTLE_SPEED: f32 = 15.0;

/// Индекс ноги: pair * 2 + (0 — левая, 1 — правая).
pub fn leg_index(pair: usize, side: f32) -> usize {
    pair * 2 + usize::from(side > 0.0)
}

/// Соседи ноги `i` при `pairs` парах: та же сторона ±1 и нога напротив.
fn neighbours(i: usize, pairs: usize) -> impl Iterator<Item = usize> {
    let (pair, side) = (i / 2, i % 2);
    let same_side = [pair.checked_sub(1), Some(pair + 1).filter(|&p| p < pairs)];
    same_side
        .into_iter()
        .flatten()
        .map(move |p| p * 2 + side)
        .chain(std::iter::once(pair * 2 + (1 - side)))
}

fn can_step(legs: &[Leg], i: usize) -> bool {
    neighbours(i, legs.len() / 2).all(|n| !legs[n].is_stepping())
}

/// Решает, какие стоящие ноги начинают шаг в этом тике.
/// `order` — переиспользуемый буфер, чтобы тик не аллоцировал.
pub fn plan_steps(legs: &mut [Leg], body: &Body, lp: &LegParams, order: &mut Vec<usize>) {
    let threshold = if body.vel.length() < SETTLE_SPEED {
        lp.settle_threshold
    } else {
        lp.step_threshold
    };
    let reach = lp.segments.iter().sum::<f32>() * 0.98;

    order.clear();
    order.extend((0..legs.len()).filter(|&i| !legs[i].is_stepping()));
    // Сначала самые отставшие; sort_unstable не аллоцирует.
    order.sort_unstable_by(|&a, &b| legs[b].error().total_cmp(&legs[a].error()));

    for &i in order.iter() {
        let overreach = legs[i].foot.distance(legs[i].hip(body)) > reach;
        if overreach || (legs[i].error() > threshold && can_step(legs, i)) {
            legs[i].start_step();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbour_sets() {
        let sorted = |i, pairs| {
            let mut n: Vec<_> = neighbours(i, pairs).collect();
            n.sort();
            n
        };
        assert_eq!(sorted(0, 4), vec![1, 2]);
        assert_eq!(sorted(5, 4), vec![3, 4, 7]);
        assert_eq!(sorted(7, 4), vec![5, 6]);
        assert_eq!(sorted(7, 6), vec![5, 6, 9]);
        assert_eq!(sorted(11, 6), vec![9, 10]);
        assert_eq!(sorted(1, 1), vec![0]);
        assert_eq!(leg_index(3, 1.0), 7);
    }
}
