//! Verlet-цепочка хлыста. Реализует агент физики (W2a).
//! Константы — один в один из `design/whip_demo.html` (там же N = 26 точек).

use glam::Vec2;

/// Точек в хлысте (точка 0 — кончик рукояти, последняя — хлопушка).
pub const N: usize = 26;
/// Длина звена.
pub const SEG: f32 = 7.4;
/// Длина хлыста, как в эскизе: N·SEG.
pub const LASH: f32 = N as f32 * SEG;

const G: f32 = 800.0;
const DAMP: f32 = 0.998;
const ITER: usize = 14;
/// Изгиб: точки через одну не ближе 1,6 звена, жёсткость 0,15.
const BEND_MIN: f32 = SEG * 1.6;
const BEND_K: f32 = 0.15;
/// Наведение захлёста, pt/с² на хлопушке (к середине хлыста спадает до 0). В эскизе захлёст
/// ловил в ≈ 45% бросков (дальше 200 pt — редко): зависело от фазы раскрутки при отпускании.
/// С наведением — ≈ 98% на 150–260 pt; на вид — просто меткий бросок.
const HOME: f32 = 50_000.0;
/// Захлёст: последние 9 точек витками вокруг цели.
const COIL: usize = 9;
const COIL_R: f32 = 13.0;
const COIL_STEP: f32 = 0.75;

pub(crate) struct Rope {
    pub p: [Vec2; N],
    pp: [Vec2; N],
    /// Последний шаг — для скоростей.
    dt: f32,
}

impl Rope {
    /// Хлыст висит отвесно под кончиком рукояти.
    pub fn hanging(tip: Vec2) -> Self {
        let mut r = Self { p: [Vec2::ZERO; N], pp: [Vec2::ZERO; N], dt: 1.0 / 240.0 };
        r.reset(tip);
        r
    }

    pub fn reset(&mut self, tip: Vec2) {
        for i in 0..N {
            self.p[i] = tip + Vec2::new(0.0, i as f32 * SEG);
            self.pp[i] = self.p[i];
        }
    }

    /// Подшаг: Verlet, корень (две точки) прибит к рукояти, связи, изгиб, витки захлёста.
    /// `gravity_k` — доля тяжести (при раскрутке 0,25). `home` — цель захлёста: дальняя половина
    /// тянется к ней (в эскизе этого нет, см. `HOME`). `coil` — (центр, фаза) витков.
    pub fn step(&mut self, dt: f32, gravity_k: f32, tip: Vec2, dir: Vec2, home: Option<Vec2>, coil: Option<(Vec2, f32)>) {
        self.dt = dt;
        let g = Vec2::new(0.0, G * gravity_k * dt * dt);
        for i in 2..N {
            let v = (self.p[i] - self.pp[i]) * DAMP;
            self.pp[i] = self.p[i];
            self.p[i] += v + g;
            if let Some(t) = home.filter(|_| i >= N / 2) {
                let w = (i - N / 2) as f32 / (N - 1 - N / 2) as f32;
                self.p[i] += (t - self.p[i]).normalize_or_zero() * (HOME * w * dt * dt);
            }
        }
        self.pin(0, tip);
        self.pin(1, tip + dir * SEG);

        for _ in 0..ITER {
            // Длины звеньев; звено 0–1 задано рукоятью, точка 1 не двигается.
            for i in 1..N - 1 {
                let d = self.p[i + 1] - self.p[i];
                let len = d.length().max(1e-6);
                let diff = d * ((len - SEG) / len);
                let (wa, wb) = if i <= 1 { (0.0, 1.0) } else { (0.5, 0.5) };
                self.p[i] += diff * wa;
                self.p[i + 1] -= diff * wb;
            }
            // Лёгкая жёсткость на изгиб — волна идёт плавнее.
            for i in 1..N - 2 {
                let d = self.p[i + 2] - self.p[i];
                let len = d.length().max(1e-6);
                if len < BEND_MIN {
                    let diff = d * ((len - BEND_MIN) / len * BEND_K);
                    if i > 1 {
                        self.p[i] += diff;
                    }
                    self.p[i + 2] -= diff;
                }
            }
            if let Some((c, phase)) = coil {
                for j in 0..COIL {
                    let a = phase + j as f32 * COIL_STEP;
                    self.p[N - COIL + j] = c + Vec2::from_angle(a) * COIL_R;
                }
                self.p[0] = tip;
            }
        }
    }

    fn pin(&mut self, i: usize, at: Vec2) {
        self.pp[i] = self.p[i];
        self.p[i] = at;
    }

    pub fn end(&self) -> Vec2 {
        self.p[N - 1]
    }

    /// Скорость хлопушки за последний подшаг, pt/с.
    pub fn tip_speed(&self) -> f32 {
        (self.p[N - 1] - self.pp[N - 1]).length() / self.dt
    }

    /// Направление кончика по последним точкам.
    pub fn tip_dir(&self) -> Vec2 {
        (self.p[N - 1] - self.p[N - 4]).try_normalize().unwrap_or(Vec2::X)
    }

    /// Прошла ли дальняя половина хлыста ближе `r` к точке (`lashNear` в эскизе).
    pub fn near(&self, p: Vec2, r: f32) -> bool {
        self.p[N / 2..].iter().any(|q| q.distance(p) < r)
    }

    /// Задать точки (неподвижными) — для тестов.
    #[cfg(test)]
    pub fn set(&mut self, f: impl Fn(usize) -> Vec2) {
        for i in 0..N {
            self.p[i] = f(i);
            self.pp[i] = self.p[i];
        }
    }

    #[cfg(test)]
    pub fn is_finite(&self) -> bool {
        self.p.iter().all(|q| q.is_finite())
    }
}
