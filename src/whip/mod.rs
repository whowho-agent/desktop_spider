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
use hand::{ease_out, Hand, Mode};
use rope::Rope;

/// Длина рукояти от точки курсора до кончика: TIP_X − GRIP_X.
pub const HANDLE_REACH: f32 = 38.0;

/// Поза для отрисовки (только чтение).
pub struct WhipPose<'a> {
    /// Положение кисти (курсор + вынос кисти при ударе/раскрутке).
    pub hand: Vec2,
    /// Угол рукояти, рад (0 = вправо, Y вниз).
    pub angle: f32,
    /// Точки хлыста от кончика рукояти до хлопушки (`rope::N` = 26 точек, как в эскизе).
    pub rope: &'a [Vec2],
    /// Заряд раскрутки 0..1 (0 — кольцо не рисуется).
    pub charge: f32,
    /// Размер плети: множитель толщин, рукояти и кольца заряда (точки `hand`/`rope` уже в мире).
    pub scale: f32,
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

/// Досягаемость захлёста: хлыст + рукоять + запас на вынос кисти.
pub const REACH: f32 = rope::LASH + HANDLE_REACH + 30.0;
/// Щелчок, только когда петля почти развернулась: кончик впереди кисти хотя бы на столько.
const CRACK_AHEAD: f32 = 160.0;
/// До какого времени броска захлёст ещё может поймать (ключи захлёста кончаются в 0,42 с).
const LASSO_LATE: f32 = 0.4;
/// Подшаг физики (константы подобраны под него).
const SUB_DT: f32 = 1.0 / 240.0;
/// Через столько после удара плеть считается успокоившейся.
const QUIET: f32 = 1.5;
/// Прицел по умолчанию, рад (вверх-вправо).
const AIM0: f32 = -0.9;

/// Плеть. При щелчке сама вызывает `fx.crack`; след (`fx.welt`) добавляет `app.rs` в режиме «Просто плеть».
///
/// Отпустили хоткей (`held == false`): новых действий нет, раскрутка отменяется без удара,
/// захват сразу отпускает цель (`Released`); начатый удар доигрывается (может щёлкнуть).
/// Хлыст продолжает считаться, чтобы при повторном нажатии не дёргался.
pub struct Whip {
    fx: fx::Fx,
    hand: Hand,
    rope: Rope,
    cursor: Vec2,
    /// Сглаженная скорость курсора и время с его последнего сдвига.
    vel: Vec2,
    since_move: f32,
    prev: WhipInput,
    cracked: bool,
    /// Захлёст целится в паука.
    lasso: bool,
    /// Где держим пойманную цель.
    hold: Vec2,
    /// Сколько рукоять уже в покое, с.
    quiet: f32,
    /// Размер плети. Физика считается в эталонных единицах эскиза (scale = 1):
    /// курсор и цель делятся на scale на входе, точки событий и позы умножаются на выходе.
    /// Так при любом размере удар занимает то же время, просто плеть крупнее и бьёт дальше.
    scale: f32,
}

impl Whip {
    pub fn new(cursor: Vec2) -> Self {
        let hand = Hand::new(AIM0);
        let rope = Rope::hanging(hand.tip(cursor));
        Self {
            fx: fx::Fx::default(),
            hand,
            rope,
            cursor,
            vel: Vec2::ZERO,
            since_move: 0.0,
            prev: WhipInput::default(),
            cracked: false,
            lasso: false,
            hold: cursor,
            quiet: QUIET,
            scale: 1.0,
        }
    }

    /// Сбросить хлыст висящим под курсором (при включении плети).
    /// Сменить размер (множитель к эталону). Хлыст пересобирается висящим под курсором.
    pub fn set_scale(&mut self, scale: f32, cursor: Vec2) {
        let scale = scale.clamp(0.25, 4.0);
        if (scale - self.scale).abs() < 1e-4 {
            return;
        }
        self.scale = scale;
        self.fx.set_scale(scale);
        self.reset(cursor);
    }

    /// Навести прицел на точку (для записи демо: как если бы мышь долго вели в её сторону).
    /// Действует только в покое, до клика.
    pub fn aim_at(&mut self, cursor: Vec2, point: Vec2) {
        let d = point - cursor;
        if d.length_squared() > 1.0 {
            self.hand.aim = d.y.atan2(d.x);
        }
    }

    pub fn reset(&mut self, cursor: Vec2) {
        let cursor = cursor / self.scale;
        self.hand.set_ready();
        self.hand.off = Vec2::ZERO;
        self.hand.angle = self.hand.aim;
        self.cursor = cursor;
        self.vel = Vec2::ZERO;
        self.since_move = 0.0;
        self.quiet = QUIET;
        self.rope.reset(self.hand.tip(cursor));
    }

    /// Шаг физики. `dt` — шаг сима (1/120), внутри 2 подшага по 1/240.
    /// `target` — позиция паука для захлёста (`None` в режиме «Просто плеть»).
    /// События дописываются в `events`.
    pub fn update(&mut self, dt: f32, cursor: Vec2, input: WhipInput, target: Option<Vec2>, events: &mut Vec<WhipEvent>) {
        self.fx.update(dt);
        // В эталонные единицы.
        let cursor = cursor / self.scale;
        let target = target.map(|t| t / self.scale);

        // Скорость курсора, сглаженная как в эскизе (по сдвигам, а не по кадрам).
        self.since_move += dt;
        let from = self.cursor;
        if cursor != from {
            let raw = (cursor - from) / self.since_move.max(1e-3);
            self.vel = self.vel.lerp(raw, 0.3);
            self.since_move = 0.0;
        }
        self.cursor = cursor;

        // Кнопки приходят уровнями — фронты считаем сами.
        let lmb_down = input.lmb && !self.prev.lmb;
        let rmb_down = input.rmb && !self.prev.rmb;
        let rmb_up = !input.rmb && self.prev.rmb;
        self.prev = input;
        if !input.held {
            match self.hand.mode {
                Mode::Spin => self.hand.set_ready(),
                Mode::Wrap => {
                    self.hand.set_ready();
                    events.push(WhipEvent::Released);
                }
                _ => {}
            }
        } else if self.hand.mode == Mode::Ready && lmb_down {
            self.start_strike(false, false);
        } else if self.hand.mode == Mode::Ready && rmb_down {
            self.hand.start_spin();
        } else if self.hand.mode == Mode::Spin && rmb_up {
            self.release_spin(target);
        }

        let n = ((dt / SUB_DT).round() as usize).max(1);
        let sub = dt / n as f32;
        for k in 0..n {
            let c = from.lerp(cursor, (k + 1) as f32 / n as f32);
            self.substep(sub, c, target, events);
        }
        if self.hand.mode == Mode::Wrap {
            events.push(WhipEvent::Hold { pos: self.hold * self.scale });
        }
        self.quiet = if self.hand.settled() { self.quiet + dt } else { 0.0 };
    }

    fn start_strike(&mut self, power: bool, lasso: bool) {
        self.hand.start_strike(power);
        self.cracked = false;
        self.lasso = lasso;
    }

    /// Отпустили ПКМ: захлёст в паука, если он в досягаемости, иначе усиленный щелчок.
    fn release_spin(&mut self, target: Option<Vec2>) {
        let lasso = match target {
            Some(t) if t.distance(self.cursor) < REACH => {
                let d = t - self.cursor;
                self.hand.aim = d.y.atan2(d.x);
                true
            }
            _ => false,
        };
        self.start_strike(true, lasso);
    }

    fn substep(&mut self, dt: f32, cursor: Vec2, target: Option<Vec2>, events: &mut Vec<WhipEvent>) {
        self.hand.step(dt, self.vel);
        self.vel *= 0.98;

        // Захват: тянем цель к кисти, через 1 с отпускаем.
        if let Some(w) = self.hand.wrap {
            let goal = cursor + self.hand.aim_dir() * 70.0;
            self.hold = w.from.lerp(goal, ease_out((w.t / 0.7).clamp(0.0, 1.0)));
            if w.t > 1.0 {
                self.hand.set_ready();
                events.push(WhipEvent::Released);
            }
        }

        let gravity = if self.hand.mode == Mode::Spin { 0.25 } else { 1.0 };
        let coil = self.hand.wrap.map(|w| (self.hold, w.phase));
        let home = target.filter(|_| self.hand.mode == Mode::Strike && self.hand.power && self.lasso);
        self.rope.step(dt, gravity, self.hand.tip(cursor), self.hand.dir(), home, coil);

        // Щелчок: кончик превысил порог скорости, когда петля развернулась.
        let power = self.hand.power;
        if self.hand.mode == Mode::Strike && !self.cracked && self.hand.t > if power { 0.04 } else { 0.15 } {
            let e = self.rope.end();
            let lasso = power && self.lasso;
            let fast = self.rope.tip_speed() > if power { 1500.0 } else { 1800.0 };
            let ahead = (e - self.hand.pos(cursor)).dot(self.hand.aim_dir());
            match target {
                Some(t) if lasso && self.rope.near(t, 30.0) => {
                    self.cracked = true;
                    let d = e - t;
                    self.hand.start_wrap(d.y.atan2(d.x), t);
                    self.hold = t;
                    self.fx.crack(e * self.scale, false);
                    events.push(WhipEvent::Caught);
                }
                _ => {
                    // Захлёст ловит до конца броска (в эскизе — до 0,3 с), потом — щелчок мимо.
                    let late = if lasso { LASSO_LATE } else if power { 0.3 } else { 0.34 };
                    if (!lasso && fast && ahead > CRACK_AHEAD) || self.hand.t > late {
                        self.cracked = true;
                        self.fx.crack(e * self.scale, power);
                        events.push(WhipEvent::Crack { pos: e * self.scale, dir: self.rope.tip_dir(), big: power });
                    }
                }
            }
        }

        if let Some(w) = &mut self.hand.wrap {
            w.phase += dt * 2.0;
        }
    }

    /// Идёт ли анимация: удар, раскрутка, захват, 1,5 с после них (хлыст докачивается) или эффекты.
    /// Качание хлыста само по себе не в счёт: при damp 0.998 оно гаснет ≈ 7 с, а без хоткея хлыст не виден.
    pub fn is_animating(&self) -> bool {
        self.quiet < QUIET || self.fx.is_active()
    }

    pub fn fx_mut(&mut self) -> &mut fx::Fx {
        &mut self.fx
    }

    /// Рисует плеть (если `visible`) и эффекты.
    pub fn draw(&self, list: &mut DrawList, skin: WhipSkin, visible: bool) {
        // Порядок слоёв: следы под плетью, вспышки щелчков поверх.
        self.fx.draw_welts(list);
        if visible {
            // Из эталонных единиц в мировые.
            let k = self.scale;
            let rope: [Vec2; rope::N] = std::array::from_fn(|i| self.rope.p[i] * k);
            let pose = WhipPose {
                hand: self.hand.pos(self.cursor) * k,
                angle: self.hand.angle,
                rope: &rope,
                charge: self.hand.charge(),
                scale: k,
            };
            draw::draw_whip(list, &pose, skin::palette(skin));
        }
        self.fx.draw_cracks(list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;
    const C: Vec2 = Vec2::ZERO;

    fn inp(lmb: bool, rmb: bool) -> WhipInput {
        WhipInput { held: true, lmb, rmb }
    }

    /// Плеть в покое с прицелом `aim`, хлыст успокоился (как `record` в эскизе).
    fn settled(aim: f32) -> Whip {
        let mut w = Whip::new(C);
        w.hand.aim = aim;
        w.hand.angle = aim;
        w.reset(C);
        w.rope.set(|i| Vec2::new(30.0 + i as f32 * 2.0, i as f32 * rope::SEG));
        let mut ev = Vec::new();
        for _ in 0..240 {
            w.update(DT, C, inp(false, false), None, &mut ev);
        }
        assert!(ev.is_empty());
        w
    }

    /// Прогон `steps` шагов; `f(i)` — ввод на шаге i. Возвращает события с номером шага.
    fn run(w: &mut Whip, steps: usize, target: Option<Vec2>, f: impl Fn(usize) -> WhipInput) -> Vec<(usize, WhipEvent)> {
        let mut out = Vec::new();
        let mut tgt = target;
        for i in 0..steps {
            let mut ev = Vec::new();
            w.update(DT, C, f(i), tgt, &mut ev);
            assert!(w.rope.is_finite() && w.hand.angle.is_finite() && w.hand.off.is_finite());
            for e in ev {
                // Как app.rs: пойманный паук едет туда, куда велит Hold.
                if let WhipEvent::Hold { pos } = e {
                    tgt = Some(pos);
                }
                out.push((i, e));
            }
        }
        out
    }

    fn cracks(ev: &[(usize, WhipEvent)]) -> Vec<(usize, Vec2, bool)> {
        ev.iter()
            .filter_map(|(i, e)| match *e {
                WhipEvent::Crack { pos, big, .. } => Some((*i, pos, big)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn strike_cracks_once_in_range() {
        for aim in [-0.8f32, -0.35, 0.0, 2.5] {
            let mut w = settled(aim);
            let ev = run(&mut w, 120, None, |i| inp(i < 5, false));
            let c = cracks(&ev);
            assert_eq!(c.len(), 1, "aim {aim}: {ev:?}");
            let (i, pos, big) = c[0];
            let t = (i + 1) as f32 * DT;
            let d = pos.distance(C);
            println!("aim {aim}: crack at {:.0} ms, {d:.0} pt", t * 1000.0);
            assert!(!big);
            assert!((0.2..=0.34).contains(&t), "aim {aim}: t = {t}");
            assert!((150.0..=280.0).contains(&d), "aim {aim}: d = {d}");
        }
    }

    fn link_error(w: &Whip) -> f32 {
        (0..rope::N - 1).map(|k| (w.rope.p[k + 1].distance(w.rope.p[k]) - rope::SEG).abs()).fold(0.0, f32::max)
    }

    /// Звенья держат длину. Во время броска цепочка тянется до ≈ 5 pt на звено и до ≈ 1 с
    /// отыгрывает отдачей (редкие всплески до ≈ 0,7 pt) — так же и в эскизе (там до 5,9 pt):
    /// 14 итераций Гаусса — Зейделя, как в эталоне. После удара — в пределах 0,5 pt.
    #[test]
    fn links_keep_length() {
        for aim in [-0.8f32, -0.35, 0.0, 2.5] {
            let mut w = settled(aim);
            let mut during = 0.0f32;
            let mut recoil = 0.0f32;
            for i in 0..240 {
                let mut ev = Vec::new();
                w.update(DT, C, inp(i < 3, false), None, &mut ev);
                if i < 120 { during = during.max(link_error(&w)) } else { recoil = recoil.max(link_error(&w)) }
            }
            let after = link_error(&w);
            println!("aim {aim}: link error during strike {during:.2}, 1–2 s {recoil:.2}, at 2 s {after:.3}");
            assert!(after < 0.5, "aim {aim}: {after}");
            assert!(recoil < 1.0, "aim {aim}: {recoil}");
            assert!(during < 8.0, "aim {aim}: {during}");
        }
    }

    #[test]
    fn power_strike_without_target_cracks_big() {
        let mut w = settled(-0.35);
        // ПКМ держим 1,25 с, отпускаем.
        let ev = run(&mut w, 300, None, |i| inp(false, i < 150));
        let c = cracks(&ev);
        assert_eq!(c.len(), 1, "{ev:?}");
        assert!(c[0].2);
        println!("power crack at {:.0} ms after release", (c[0].0 + 1 - 150) as f32 * DT * 1000.0);
    }

    #[test]
    fn lasso_catches_and_releases() {
        let up = -std::f32::consts::FRAC_PI_2;
        for (aim, dist) in [(-0.35f32, 150.0f32), (-0.35, 208.0), (0.0, 253.0), (up, 200.0), (up, 225.0), (2.5, 180.0), (1.2, 240.0)] {
            let mut w = settled(aim);
            let target = C + Vec2::from_angle(aim) * dist;
            let ev = run(&mut w, 400, Some(target), |i| inp(false, i < 150));
            let kinds: Vec<&WhipEvent> = ev.iter().map(|(_, e)| e).collect();
            assert!(cracks(&ev).is_empty(), "aim {aim} d {dist}: {kinds:?}");
            let caught = ev.iter().position(|(_, e)| *e == WhipEvent::Caught).unwrap_or_else(|| panic!("aim {aim} d {dist}: not caught"));
            let released = ev.iter().position(|(_, e)| *e == WhipEvent::Released).expect("released");
            assert!(released > caught + 1);
            assert!(ev[caught + 1..released].iter().all(|(_, e)| matches!(e, WhipEvent::Hold { .. })));
            let held = (ev[released].0 - ev[caught].0) as f32 * DT;
            assert!((0.95..1.1).contains(&held), "held {held}");
            // Цель подтянута к кисти: курсор + прицел·70.
            let WhipEvent::Hold { pos } = ev[released - 1].1 else { unreachable!() };
            assert!((pos.distance(C) - 70.0).abs() < 3.0, "hold at {pos}");
            println!("aim {aim} d {dist}: caught at {:.0} ms after release", (ev[caught].0 + 1 - 150) as f32 * DT * 1000.0);
        }
    }

    #[test]
    fn lasso_out_of_reach_is_power_crack() {
        let mut w = settled(0.0);
        let ev = run(&mut w, 300, Some(Vec2::new(400.0, 0.0)), |i| inp(false, i < 150));
        let c = cracks(&ev);
        assert_eq!(c.len(), 1);
        assert!(c[0].2);
    }

    #[test]
    fn release_hotkey_cancels() {
        let mut w = settled(0.0);
        let ev = run(&mut w, 300, None, |i| WhipInput { held: i < 60, lmb: false, rmb: i < 150 });
        assert!(ev.is_empty(), "{ev:?}");
        assert_eq!(w.hand.mode, Mode::Ready);
    }

    #[test]
    fn idle_stops_animating() {
        let mut w = settled(-0.35);
        assert!(!w.is_animating());
        let _ = run(&mut w, 1, None, |_| inp(true, false));
        let mut n = 1;
        while w.is_animating() {
            let _ = run(&mut w, 1, None, |_| inp(false, false));
            n += 1;
            assert!(n < 300, "still animating");
        }
        // Удар 0,5 с + 1,5 с покоя.
        let t = n as f32 * DT;
        println!("idle {t:.2} s after strike");
        assert!((1.9..2.2).contains(&t), "{t}");
    }

    #[test]
    fn aim_follows_cursor() {
        let mut w = settled(-0.9);
        let mut ev = Vec::new();
        for i in 0..60 {
            w.update(DT, Vec2::new(i as f32 * 8.0, 0.0), inp(false, false), None, &mut ev);
        }
        assert!(w.hand.aim.abs() < 0.2, "aim {}", w.hand.aim);
    }

    /// Размер — чистый масштаб: при ×2 щелчок на том же шаге и ровно вдвое дальше.
    #[test]
    fn size_scales_reach_not_timing() {
        let crack_at = |scale: f32| {
            let mut w = Whip::new(C);
            w.set_scale(scale, C);
            w.hand.aim = -0.35;
            w.hand.angle = -0.35;
            w.reset(C);
            let mut ev = Vec::new();
            for _ in 0..240 {
                w.update(DT, C, inp(false, false), None, &mut ev);
            }
            let c = cracks(&run(&mut w, 120, None, |i| inp(i < 5, false)));
            assert_eq!(c.len(), 1, "scale {scale}");
            (c[0].0, c[0].1)
        };
        let (i1, p1) = crack_at(1.0);
        for k in [0.7f32, 2.0] {
            let (ik, pk) = crack_at(k);
            assert_eq!(ik, i1, "scale {k}: другой шаг щелчка");
            assert!(pk.distance(p1 * k) < 0.5, "scale {k}: {pk} vs {}", p1 * k);
        }
    }
}
