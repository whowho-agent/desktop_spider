// Инстансный SDF-рендер капсул, кругов и колец. Координаты в точках, Y вниз.
// Цвета — обычный sRGB без premultiply, на выходе premultiplied.

struct Globals {
    viewport: vec2<f32>, // размер окна в точках
    px_per_pt: f32,      // scale factor
    gloss: f32,          // сила блика 0..1
    light: vec2<f32>,    // единичное направление на свет (экран, Y вниз)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;

// Виды фигур (см. batch.rs, kind::*).
const K_FLAT: u32 = 0u;
const K_TUBE: u32 = 1u;
const K_BRAID: u32 = 2u;
const K_METAL: u32 = 3u;
const K_SPHERE: u32 = 4u;
const K_SOFT: u32 = 5u;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    @location(3) @interpolate(flat) rr: vec2<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) style: vec2<f32>,
};

fn kind_of(style: vec2<f32>) -> u32 {
    return u32(max(round(style.x), 0.0));
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) rr: vec2<f32>, // radius, ring_width
    @location(3) color: vec4<f32>,
    @location(4) style: vec2<f32>, // вид, параметр
) -> VsOut {
    // Ориентированный квад вокруг отрезка с запасом под AA.
    let d = b - a;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-4) {
        dir = d / len;
    }
    let n = vec2<f32>(-dir.y, dir.x);
    let k = kind_of(style);
    // Кольцо — только у плоских; у мягкой тени ещё ширина размытия.
    var extra = 0.0;
    if (k == K_FLAT) {
        extra = rr.y;
    } else if (k == K_SOFT) {
        extra = max(style.y, 0.0);
    }
    let ext = rr.x + extra + 1.5 / g.px_per_pt;

    // Порядок triangle strip: (0,0) (1,0) (0,1) (1,1).
    let ux = f32(vi & 1u);
    let uy = f32((vi >> 1u) & 1u) * 2.0 - 1.0;
    let base = mix(a - dir * ext, b + dir * ext, ux);
    let p = base + n * (uy * ext);

    var out: VsOut;
    let ndc = p / g.viewport * 2.0 - 1.0;
    out.clip = vec4<f32>(ndc.x, -ndc.y, 0.0, 1.0);
    out.p = p;
    out.a = a;
    out.b = b;
    out.rr = rr;
    out.color = color;
    out.style = style;
    return out;
}

// ---------- палитра: тона по светлоте в HSL, как makeLeather() в эскизе ----------

fn rgb2hsl(c: vec3<f32>) -> vec3<f32> {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let l = (mx + mn) * 0.5;
    let d = mx - mn;
    if (d < 1e-5) {
        return vec3<f32>(0.0, 0.0, l);
    }
    let s = d / max(1.0 - abs(2.0 * l - 1.0), 1e-5);
    var h: f32;
    if (mx == c.r) {
        h = (c.g - c.b) / d;
        if (h < 0.0) {
            h += 6.0;
        }
    } else if (mx == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    return vec3<f32>(h / 6.0, s, l);
}

fn hsl2rgb(hsl: vec3<f32>) -> vec3<f32> {
    let s = clamp(hsl.y, 0.0, 1.0);
    let l = clamp(hsl.z, 0.0, 1.0);
    let k = clamp(abs((hsl.x * 6.0 + vec3<f32>(0.0, 4.0, 2.0)) % 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let c = (1.0 - abs(2.0 * l - 1.0)) * s;
    return l + c * (k - 0.5);
}

// Тот же тон со светлотой l·k (не ниже minl).
fn tone(hsl: vec3<f32>, k: f32, minl: f32) -> vec3<f32> {
    return hsl2rgb(vec3<f32>(hsl.x, hsl.y, max(minl, hsl.z * k)));
}

// Блик: почти белый с оттенком основы.
fn spec_of(hsl: vec3<f32>, l: f32) -> vec3<f32> {
    return hsl2rgb(vec3<f32>(hsl.x, hsl.y * 0.5, l));
}

// Колокол ширины w вокруг c (0 вне).
fn bump(t: f32, c: f32, w: f32) -> f32 {
    return max(0.0, 1.0 - abs(t - c) / w);
}

// Кожа поперёк трубки, t: 0 — теневой край, 1 — край к свету (градиент tube() эскиза).
fn tube_color(base: vec3<f32>, t: f32) -> vec3<f32> {
    let hsl = rgb2hsl(base);
    let dark = tone(hsl, 0.42, 0.0);
    // Светлые тона чуть приглушены по насыщенности, как в ручной палитре эскиза.
    let mid = tone(vec3<f32>(hsl.x, hsl.y * 0.9, hsl.z), 1.25, 0.0);
    let light = tone(vec3<f32>(hsl.x, hsl.y * 0.88, hsl.z), 1.7, 0.3);
    var c: vec3<f32>;
    if (t < 0.32) {
        c = mix(dark, base, t / 0.32);
    } else if (t < 0.6) {
        c = mix(base, mid, (t - 0.32) / 0.28);
    } else if (t < 0.8) {
        c = mix(mid, light, (t - 0.6) / 0.2);
    } else {
        c = mix(light, mid, (t - 0.8) / 0.2);
    }
    // Стоп блика 0.9.
    let w = bump(t, 0.9, 0.1) * min(1.0, 1.6 * g.gloss);
    return mix(c, spec_of(hsl, 0.93), w);
}

// Полоса блика у света поверх всего (spec-линия эскиза на v ≈ 0.5).
fn tube_spec(base: vec3<f32>, c: vec3<f32>, t: f32) -> vec3<f32> {
    let w = smoothstep(0.0, 1.0, bump(t, 0.76, 0.09)) * min(1.0, 1.1 * g.gloss) * 0.85;
    return mix(c, spec_of(rgb2hsl(base), 0.93), w);
}

// Металл поперёк трубки (градиент metal() эскиза): резче, узкий яркий блик.
fn metal_color(base: vec3<f32>, t: f32) -> vec3<f32> {
    let hsl = rgb2hsl(base);
    let dark = tone(hsl, 0.55, 0.0);
    let light = tone(hsl, 1.45, 0.0);
    var c: vec3<f32>;
    if (t < 0.45) {
        c = mix(dark, base, t / 0.45);
    } else if (t < 0.78) {
        c = mix(base, light, (t - 0.45) / 0.33);
    } else {
        c = mix(light, base, (t - 0.78) / 0.22);
    }
    let spec = hsl2rgb(vec3<f32>(hsl.x, hsl.y * 0.6, 0.96));
    let w = bump(t, 0.9, 0.1) * (0.4 + 0.6 * g.gloss);
    return mix(c, spec, w);
}

// Оплётка: две семьи диагональных прядей (ромбы), тёмная линия + светлая рядом к свету.
// u — вдоль оси в периодах, v — поперёк (-1..1, +1 к свету).
// du, dv — fwidth(u), fwidth(v), посчитаны заранее в равномерном потоке.
fn braid_color(base: vec3<f32>, c: vec3<f32>, u: f32, v: f32, du: f32, dv: f32, period_px: f32) -> vec3<f32> {
    let hsl = rgb2hsl(base);
    let strand_d = tone(hsl, 0.55, 0.0);
    let strand_l = tone(hsl, 1.9, 0.32);
    // Мельче ~2.5 px узор не читается — гасим, чтобы не было муара.
    let fade = smoothstep(1.8, 3.2, period_px);
    if (fade <= 0.0) {
        return c;
    }
    var out = c;
    for (var i = 0; i < 2; i++) {
        let s = select(-1.0, 1.0, i == 0);
        let f = u + s * v * 0.6;
        let aa = max(du + 0.5 * dv, 1e-4);
        // Тёмная прядь: ~21% периода.
        let dd = abs(fract(f + 0.5) - 0.5);
        let dark = 1.0 - smoothstep(0.105 - aa * 0.5, 0.105 + aa * 0.5, dd);
        // Светлая — ~10%, сдвинута к свету (рост v → рост s·f).
        let fl = f - s * 0.19;
        let dl = abs(fract(fl + 0.5) - 0.5);
        let lit = 1.0 - smoothstep(0.05 - aa * 0.5, 0.05 + aa * 0.5, dl);
        out = mix(out, strand_d, dark * 0.65 * fade);
        out = mix(out, strand_l, lit * 0.5 * fade);
    }
    return out;
}

// Сфера: радиальный градиент с фокусом, смещённым к свету (навершие эскиза).
// matte 0 — металл с бликом, 1 — кожаный узел без блика.
fn sphere_color(base: vec3<f32>, q: vec2<f32>, r: f32, matte: f32) -> vec3<f32> {
    let hsl = rgb2hsl(base);
    let f = g.light * (0.3 * r);
    // Фокальный градиент: ищем t, при котором q лежит на окружности
    // с центром mix(f, 0, t) и радиусом t·rg.
    let rg = r * 1.02;
    let qf = q - f;
    let e = -f;
    let aq = dot(e, e) - rg * rg;
    let qe = dot(qf, e);
    let t = clamp((qe - sqrt(max(qe * qe - aq * dot(qf, qf), 0.0))) / aq, 0.0, 1.0);

    // Металл: [0 блик, 0.25 свет, 0.6 основа, 1 тень].
    let m_dark = tone(hsl, 0.53, 0.0);
    let m_light = tone(hsl, 1.45, 0.0);
    let m_spec = hsl2rgb(vec3<f32>(hsl.x, hsl.y * 0.6, 0.96));
    let s0 = mix(m_light, m_spec, 0.35 + 0.65 * g.gloss);
    var m: vec3<f32>;
    if (t < 0.25) {
        m = mix(s0, m_light, t / 0.25);
    } else if (t < 0.6) {
        m = mix(m_light, base, (t - 0.25) / 0.35);
    } else {
        m = mix(base, m_dark, (t - 0.6) / 0.4);
    }
    // Пятно блика в фокусе: размер и сила от gloss.
    let sr = r * (0.1 + 0.22 * g.gloss);
    let sd = length(q - g.light * (0.38 * r));
    let spot = (1.0 - smoothstep(sr * 0.4, sr, sd)) * (0.35 + 0.65 * g.gloss);
    m = mix(m, m_spec, spot);

    // Кожа: [0 свет, 0.55 основа, 1 тень].
    let k_dark = tone(hsl, 0.42, 0.0);
    let k_light = tone(hsl, 1.7, 0.3);
    var k: vec3<f32>;
    if (t < 0.55) {
        k = mix(k_light, base, t / 0.55);
    } else {
        k = mix(base, k_dark, (t - 0.55) / 0.45);
    }
    return mix(m, k, clamp(matte, 0.0, 1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Расстояние до отрезка a..b.
    let pa = in.p - in.a;
    let ba = in.b - in.a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    let dist = length(pa - ba * h);
    let k = kind_of(in.style);

    // Координаты трубки (до любых discard/ветвлений — ради fwidth).
    let len = length(ba);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-4) {
        dir = ba / len;
    }
    // Нормаль НЕ разворачиваем к свету: иначе у звеньев почти вдоль света блик перескакивал
    // с края на край. Освещение считаем как у настоящего цилиндра (см. lit ниже) — оно
    // непрерывно при любом повороте, и соседние отрезки хлыста стыкуются без скачков.
    let n = vec2<f32>(-dir.y, dir.x);
    let r = max(in.rr.x, 1e-4);
    // Поперёк: -1..1; у круглых концов та же проекция.
    let v = clamp(dot(pa, n) / r, -1.0, 1.0);
    // Нормаль поверхности цилиндра в этой точке и свет сверху-сбоку (z — к зрителю).
    let nz = sqrt(max(0.0, 1.0 - v * v));
    let l3 = normalize(vec3<f32>(g.light * 0.8, 0.6));
    let lit = dot(vec3<f32>(n * v, nz), l3);
    // Для пряди «к свету» — знак стороны, плавно уходящий в 0, когда звено вдоль света.
    let side = clamp(dot(n, g.light) * 4.0, -1.0, 1.0);
    // Вдоль оси от a (+ фаза в ring_width, pt), в периодах оплётки.
    let u = (dot(pa, dir) + in.rr.y) / max(in.style.y, 1e-4);
    let du = fwidth(u);
    let dv = fwidth(v);

    if (k == K_FLAT) {
        // Плоская заливка — ровно как раньше.
        var sd = dist - in.rr.x;
        if (in.rr.y > 0.0) {
            sd = abs(dist - in.rr.x) - in.rr.y;
        }
        let alpha = clamp(0.5 - sd * g.px_per_pt, 0.0, 1.0) * in.color.a;
        if (alpha <= 0.0) {
            discard;
        }
        // Premultiplied на выходе.
        return vec4<f32>(in.color.rgb * alpha, alpha);
    }

    if (k == K_SOFT) {
        // Мягкая тень: край размыт на ±param.
        let s = max(in.style.y, 0.0);
        var cov: f32;
        if (s > 0.0) {
            cov = 1.0 - smoothstep(in.rr.x - s, in.rr.x + s, dist);
        } else {
            cov = clamp(0.5 - (dist - in.rr.x) * g.px_per_pt, 0.0, 1.0);
        }
        let alpha = cov * in.color.a;
        if (alpha <= 0.0) {
            discard;
        }
        return vec4<f32>(in.color.rgb * alpha, alpha);
    }

    // Объёмные виды: обычный AA-край капсулы (кольцо не поддерживается).
    let alpha = clamp(0.5 - (dist - r) * g.px_per_pt, 0.0, 1.0) * in.color.a;
    if (alpha <= 0.0) {
        discard;
    }
    let base = in.color.rgb;
    var c: vec3<f32>;
    if (k == K_SPHERE) {
        c = sphere_color(base, pa, r, in.style.y);
    } else {
        // 0 — теневая сторона, 1 — самое освещённое место (было (v+1)/2 с разворотом нормали).
        // lit ∈ [-0.8, 1]; степень держит середину трубки в основном тоне (t ≈ 0,6), как в эскизе,
        // а самое освещённое место — на стопе блика (0,9).
        let t = 0.9 * pow(clamp((lit + 0.8) / 1.8, 0.0, 1.0), 1.6);
        if (k == K_METAL) {
            c = metal_color(base, t);
        } else {
            c = tube_color(base, t);
            if (k == K_BRAID && in.style.y > 0.0) {
                c = braid_color(base, c, u, v * side, du, dv, in.style.y * g.px_per_pt);
            }
            c = tube_spec(base, c, t);
        }
    }
    return vec4<f32>(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * alpha, alpha);
}
