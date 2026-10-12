// Инстансный SDF-рендер капсул, кругов и колец. Координаты в точках, Y вниз.

struct Globals {
    viewport: vec2<f32>, // размер окна в точках
    px_per_pt: f32,      // scale factor
    _pad: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) p: vec2<f32>,
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    @location(3) @interpolate(flat) rr: vec2<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) rr: vec2<f32>, // radius, ring_width
    @location(3) color: vec4<f32>,
) -> VsOut {
    // Ориентированный квад вокруг отрезка с запасом под AA.
    let d = b - a;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-4) {
        dir = d / len;
    }
    let n = vec2<f32>(-dir.y, dir.x);
    let ext = rr.x + rr.y + 1.5 / g.px_per_pt;

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
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Расстояние до отрезка a..b.
    let pa = in.p - in.a;
    let ba = in.b - in.a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-8), 0.0, 1.0);
    let dist = length(pa - ba * h);

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
