use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use super::{DrawList, Shape};

/// Uniform для шейдера: размер окна в точках, масштаб и свет.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    px_per_pt: f32,
    gloss: f32,
    light: [f32; 2],
    _pad: [f32; 2],
}

/// Свет по умолчанию: сверху слева.
fn default_light() -> glam::Vec2 {
    glam::Vec2::new(-0.45, -0.89).normalize()
}

const SHAPE_SIZE: u64 = std::mem::size_of::<Shape>() as u64;

pub struct Renderer {
    target: Target,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Физический размер цели в пикселях.
    width: u32,
    height: u32,
    scale_factor: f64,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize, // в инстансах
    /// Единичное направление на свет (экран, Y вниз) и сила блика 0..1.
    light: glam::Vec2,
    gloss: f32,
}

/// Куда рисуем: swapchain окна или offscreen-текстура (headless, для записи GIF).
enum Target {
    Surface {
        /// Держим окно живым, пока жива surface (на Windows она может быть создана не из окна).
        _window: Arc<Window>,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        /// DirectComposition-визуал, если surface создана через него.
        #[cfg(windows)]
        composition: Option<crate::platform::Composition>,
    },
    Texture {
        texture: wgpu::Texture,
        /// Буфер для чтения кадра; строки выровнены до 256 байт.
        readback: wgpu::Buffer,
        padded_row: u32,
    },
}

/// Формат offscreen-текстуры: не sRGB, как Bgra8Unorm у окна.
const HEADLESS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Режимы альфы, при которых оверлей прозрачен (в порядке предпочтения).
const TRANSPARENT_MODES: [wgpu::CompositeAlphaMode; 2] = [
    wgpu::CompositeAlphaMode::PreMultiplied,
    wgpu::CompositeAlphaMode::PostMultiplied,
];

fn is_transparent(caps: &wgpu::SurfaceCapabilities) -> bool {
    caps.alpha_modes.iter().any(|m| TRANSPARENT_MODES.contains(m))
}

/// Обычная surface из окна. На macOS (Metal) она прозрачная (PostMultiplied).
#[cfg(not(windows))]
fn window_surface(
    instance: &wgpu::Instance,
    window: Arc<Window>,
) -> (wgpu::Surface<'static>, wgpu::Adapter) {
    let surface = instance
        .create_surface(window)
        .expect("не удалось создать surface");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))
    .expect("нет подходящего GPU-адаптера");
    (surface, adapter)
}

/// Windows, запасной путь: surface из HWND. DX12 здесь всегда Opaque, Vulkan — как
/// повезёт с драйвером, поэтому перебираем адаптеры DX12 и Vulkan и берём первый
/// с прозрачной альфой (при равенстве — интегрированный GPU).
#[cfg(windows)]
fn window_surface(
    instance: &wgpu::Instance,
    window: Arc<Window>,
) -> (wgpu::Surface<'static>, wgpu::Adapter) {
    let surface = instance
        .create_surface(window)
        .expect("не удалось создать surface");
    let mut adapters: Vec<_> = instance
        .enumerate_adapters(wgpu::Backends::DX12 | wgpu::Backends::VULKAN)
        .into_iter()
        .filter(|a| a.is_surface_supported(&surface))
        .collect();
    adapters.sort_by_key(|a| {
        (
            !is_transparent(&surface.get_capabilities(a)),
            a.get_info().device_type != wgpu::DeviceType::IntegratedGpu,
        )
    });
    let adapter = adapters
        .into_iter()
        .next()
        .expect("нет подходящего GPU-адаптера (DX12/Vulkan)");
    (surface, adapter)
}

/// Windows, основной путь: swapchain для DirectComposition (DX12), поддерживает
/// PreMultiplied. `None` — DirectComposition недоступна, берём `window_surface`.
#[cfg(windows)]
fn composition_surface(
    instance: &wgpu::Instance,
    window: &Window,
) -> Option<(wgpu::Surface<'static>, wgpu::Adapter, crate::platform::Composition)> {
    let comp = crate::platform::Composition::new(window)
        .map_err(|e| eprintln!("render: DirectComposition недоступна: {e}"))
        .ok()?;
    // SAFETY: визуал живой; wgpu делает AddRef и держит его, пока жива surface.
    let surface = unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::CompositionVisual(
            comp.visual_ptr(),
        ))
    }
    .map_err(|e| eprintln!("render: surface из DirectComposition: {e}"))
    .ok()?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))
    .map_err(|e| eprintln!("render: нет DX12-адаптера для DirectComposition: {e}"))
    .ok()?;
    if !is_transparent(&surface.get_capabilities(&adapter)) {
        eprintln!("render: DirectComposition-surface без прозрачной альфы, пробую HWND");
        return None;
    }
    Some((surface, adapter, comp))
}

impl Renderer {
    /// Синхронно (через pollster) создаёт device и прозрачную surface для окна.
    pub fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();
        let scale_factor = window.scale_factor();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        #[cfg(windows)]
        let (surface, adapter, composition) = match composition_surface(&instance, &window) {
            Some((s, a, c)) => (s, a, Some(c)),
            None => {
                let (s, a) = window_surface(&instance, window.clone());
                (s, a, None)
            }
        };
        #[cfg(not(windows))]
        let (surface, adapter) = window_surface(&instance, window.clone());

        let info = adapter.get_info();
        eprintln!("render: {:?} / {}", info.backend, info.name);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("spider device"),
            ..Default::default()
        }))
        .expect("не удалось создать device");

        // Формат и режим альфы swapchain.
        let caps = surface.get_capabilities(&adapter);
        let format = if caps.formats.contains(&wgpu::TextureFormat::Bgra8Unorm) {
            wgpu::TextureFormat::Bgra8Unorm
        } else {
            caps.formats[0]
        };
        let alpha_mode = TRANSPARENT_MODES
            .into_iter()
            .find(|m| caps.alpha_modes.contains(m))
            .unwrap_or(caps.alpha_modes[0]);
        eprintln!("render: format {format:?}, alpha_mode {alpha_mode:?}");
        if !is_transparent(&caps) {
            eprintln!(
                "render: ВНИМАНИЕ: поверхность {:?} не поддерживает попиксельную альфу \
                 (есть только {:?}) — у оверлея будет чёрный фон вместо прозрачного",
                info.backend, caps.alpha_modes
            );
        }

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        let (width, height) = (config.width, config.height);
        let target = Target::Surface {
            _window: window,
            surface,
            config,
            #[cfg(windows)]
            composition,
        };
        let r = Self::with_target(device, queue, target, format, width, height, scale_factor);
        r.configure_surface();
        r
    }

    /// Без окна: рисует в текстуру width×height физических пикселей; scale_factor = пикселей на pt.
    pub fn headless(width: u32, height: u32, scale_factor: f64) -> Self {
        let (width, height) = (width.max(1), height.max(1));
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .expect("нет подходящего GPU-адаптера");
        let info = adapter.get_info();
        eprintln!("render (headless): {:?} / {}", info.backend, info.name);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("spider headless device"),
            ..Default::default()
        }))
        .expect("не удалось создать device");
        let target = Self::texture_target(&device, width, height);
        Self::with_target(device, queue, target, HEADLESS_FORMAT, width, height, scale_factor)
    }

    /// Offscreen-текстура и буфер для чтения под размер width×height.
    fn texture_target(device: &wgpu::Device, width: u32, height: u32) -> Target {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HEADLESS_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (width * 4).div_ceil(align) * align;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded_row as u64 * height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Target::Texture {
            texture,
            readback,
            padded_row,
        }
    }

    /// Общая часть для окна и headless: uniform, пайплайн, буфер инстансов.
    fn with_target(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target: Target,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        scale_factor: f64,
    ) -> Self {
        let (pipeline, globals_buf, bind_group) = create_pipeline(&device, format);
        let capacity = 256;
        let instances = Self::create_instances(&device, capacity);
        let mut r = Self {
            target,
            device,
            queue,
            width,
            height,
            scale_factor,
            pipeline,
            globals_buf,
            bind_group,
            instances,
            capacity,
            light: default_light(),
            gloss: 0.5,
        };
        r.write_globals();
        r
    }

    /// (Пере)конфигурирует swapchain; на Windows с DirectComposition — ещё и Commit,
    /// иначе новый swapchain (SetContent) не появится на экране.
    fn configure_surface(&self) {
        let Target::Surface {
            surface,
            config,
            #[cfg(windows)]
            composition,
            ..
        } = &self.target
        else {
            return;
        };
        surface.configure(&self.device, config);
        #[cfg(windows)]
        if let Some(c) = composition {
            c.commit();
        }
    }

    /// Вызывать на `WindowEvent::Resized` и `ScaleFactorChanged`.
    pub fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.scale_factor = scale_factor;
        if size.width > 0 && size.height > 0 {
            self.width = size.width;
            self.height = size.height;
            match &mut self.target {
                Target::Surface { config, .. } => {
                    config.width = size.width;
                    config.height = size.height;
                    self.configure_surface();
                }
                Target::Texture { .. } => {
                    self.target = Self::texture_target(&self.device, size.width, size.height);
                }
            }
        }
        self.write_globals();
    }

    /// Очищает кадр в полностью прозрачный цвет и рисует все фигуры.
    pub fn render(&mut self, list: &DrawList) {
        let Target::Surface { surface, .. } = &self.target else {
            panic!("Renderer::render — только для оконного рендера, для headless есть render_to_rgba");
        };
        let frame = match surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.configure_surface();
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(e) => {
                eprintln!("render: surface error: {e}");
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let encoder = self.encode_shapes(&view, list);
        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    /// Очищает в прозрачный (0,0,0,0), рисует список и возвращает кадр: RGBA8, НЕ premultiplied
    /// (straight alpha, чтобы ffmpeg мог наложить на фон), строки подряд без выравнивания (width*4 байт).
    pub fn render_to_rgba(&mut self, list: &DrawList) -> Vec<u8> {
        let Target::Texture { texture, .. } = &self.target else {
            panic!("Renderer::render_to_rgba — только для headless-рендера (Renderer::headless)");
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.encode_shapes(&view, list);
        let Target::Texture {
            texture,
            readback,
            padded_row,
        } = &self.target
        else {
            unreachable!()
        };
        let (w, h, padded_row) = (self.width, self.height, *padded_row);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        // Синхронно ждём map.
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait)
            .expect("render: device.poll не дождался GPU");
        rx.recv()
            .expect("render: map_async не вызвал callback")
            .expect("render: не удалось прочитать кадр");

        // Убираем выравнивание строк и снимаем premultiply.
        let row = w as usize * 4;
        let mut out = Vec::with_capacity(row * h as usize);
        {
            let data = slice.get_mapped_range();
            for y in 0..h as usize {
                let start = y * padded_row as usize;
                out.extend_from_slice(&data[start..start + row]);
            }
        }
        readback.unmap();
        for px in out.chunks_exact_mut(4) {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = if a > 0 {
                    ((*c as u32 * 255 + a / 2) / a).min(255) as u8
                } else {
                    0
                };
            }
        }
        out
    }

    /// Заливает инстансы и записывает проход: очистка в прозрачный + все фигуры.
    fn encode_shapes(&mut self, view: &wgpu::TextureView, list: &DrawList) -> wgpu::CommandEncoder {
        // Буфер инстансов растёт степенями двойки.
        let n = list.shapes.len();
        if n > self.capacity {
            self.capacity = n.next_power_of_two();
            self.instances = Self::create_instances(&self.device, self.capacity);
        }
        if n > 0 {
            self.queue
                .write_buffer(&self.instances, 0, bytemuck::cast_slice(&list.shapes));
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shapes"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if n > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..n as u64 * SHAPE_SIZE));
                pass.draw(0..4, 0..n as u32);
            }
        }
        encoder
    }

    fn create_instances(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shapes"),
            size: capacity as u64 * SHAPE_SIZE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Свет для объёмных фигур: направление на источник (экран, Y вниз; нормализуется)
    /// и сила блика 0..1. По умолчанию — сверху слева, 0.5.
    pub fn set_lighting(&mut self, light: glam::Vec2, gloss: f32) {
        let light = light.try_normalize().unwrap_or_else(default_light);
        let gloss = if gloss.is_finite() { gloss.clamp(0.0, 1.0) } else { 0.5 };
        if light != self.light || gloss != self.gloss {
            self.light = light;
            self.gloss = gloss;
            self.write_globals();
        }
    }

    /// Viewport в точках = физический размер / scale factor.
    fn write_globals(&mut self) {
        let s = self.scale_factor as f32;
        let g = Globals {
            viewport: [self.width as f32 / s, self.height as f32 / s],
            px_per_pt: s,
            gloss: self.gloss,
            light: self.light.to_array(),
            _pad: [0.0; 2],
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));
    }
}

/// Uniform-буфер, bind group и пайплайн фигур — общие для окна и headless.
fn create_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::Buffer, wgpu::BindGroup) {
    let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("globals"),
        size: std::mem::size_of::<Globals>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: globals_buf.as_entire_binding(),
        }],
    });

    // Пайплайн: один инстанс = одна фигура, квад из 4 вершин (triangle strip).
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shapes"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shapes.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shapes layout"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    // Раскладка Shape: a@0, b@8, radius+ring_width@16, style@24, color@32.
    let attr = |shader_location, offset, format| wgpu::VertexAttribute {
        format,
        offset,
        shader_location,
    };
    let attributes = [
        attr(0, 0, wgpu::VertexFormat::Float32x2),
        attr(1, 8, wgpu::VertexFormat::Float32x2),
        attr(2, 16, wgpu::VertexFormat::Float32x2),
        attr(3, 32, wgpu::VertexFormat::Float32x4),
        attr(4, 24, wgpu::VertexFormat::Float32x2),
    ];
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shapes"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: SHAPE_SIZE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            }],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    });
    (pipeline, globals_buf, bind_group)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    /// Нужен GPU: `cargo test -- --ignored renderer`.
    #[test]
    #[ignore]
    fn renderer_headless_rgba() {
        let (w, h) = (64u32, 48u32);
        let mut r = Renderer::headless(w, h, 2.0);
        let mut list = DrawList::default();
        list.dot(Vec2::new(10.0, 10.0), 5.0, [1.0, 0.0, 0.0, 1.0]);
        // Полупрозрачный отрезок по y = 18 pt (36 px), толщина 4 pt.
        let seg = [0.2, 0.6, 1.0, 0.5];
        list.segment(Vec2::new(5.0, 18.0), Vec2::new(28.0, 18.0), 4.0, seg);
        let img = r.render_to_rgba(&list);
        assert_eq!(img.len(), (w * h * 4) as usize);
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [img[i], img[i + 1], img[i + 2], img[i + 3]]
        };
        assert_eq!(px(20, 20), [255, 0, 0, 255], "центр точки — непрозрачный красный");
        assert_eq!(px(0, 0), [0, 0, 0, 0], "угол прозрачный");
        assert_eq!(px(w - 1, h - 1), [0, 0, 0, 0], "угол прозрачный");
        // Середина отрезка: straight-цвет ≈ исходному, альфа ≈ 0.5.
        let p = px(32, 36);
        for c in 0..4 {
            let want = seg[c] * 255.0;
            assert!(
                (p[c] as f32 - want).abs() <= 3.0,
                "отрезок: канал {c} = {}, ожидали ≈ {want}",
                p[c]
            );
        }
    }

    fn hex(c: u32, a: f32) -> [f32; 4] {
        [((c >> 16) & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, (c & 255) as f32 / 255.0, a]
    }

    /// Пиксель (x, y) в pt при scale 2: RGBA straight.
    fn px_at(img: &[u8], w: u32, x: f32, y: f32) -> [u8; 4] {
        let (x, y) = ((x * 2.0) as u32, (y * 2.0) as u32);
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn luma(p: [u8; 4]) -> f32 {
        0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
    }

    const LEFT: Vec2 = Vec2::new(-0.45, -0.89);
    const RIGHT: Vec2 = Vec2::new(0.45, -0.89);

    /// Свойства объёмных видов. Нужен GPU: `cargo test -- --ignored shading`.
    #[test]
    #[ignore]
    fn shading_kinds() {
        let (w, h) = (220u32, 240u32);
        let mut r = Renderer::headless(w, h, 2.0);
        let base = hex(0x77461f, 1.0);
        let mut list = DrawList::default();
        // Вертикальная трубка x = 20, ширина 12.
        list.tube(Vec2::new(20.0, 10.0), Vec2::new(20.0, 50.0), 12.0, base);
        list.metal(Vec2::new(40.0, 10.0), Vec2::new(40.0, 50.0), 12.0, hex(0xa9853b, 1.0));
        list.sphere(Vec2::new(70.0, 25.0), 10.0, hex(0xa9853b, 1.0));
        list.soft(Vec2::new(10.0, 80.0), Vec2::new(60.0, 80.0), 10.0, [0.0, 0.0, 0.0, 0.5], 3.0);
        list.braid(Vec2::new(10.0, 105.0), Vec2::new(90.0, 105.0), 10.0, base, 6.0);
        let flat = [0.2, 0.6, 1.0, 1.0];
        list.segment(Vec2::new(70.0, 50.0), Vec2::new(90.0, 50.0), 6.0, flat);

        let img = r.render_to_rgba(&list);
        // Свет слева: левая сторона трубки светлее правой.
        let (l, rr) = (luma(px_at(&img, w, 17.0, 30.0)), luma(px_at(&img, w, 23.0, 30.0)));
        assert!(l > rr + 20.0, "tube, свет слева: {l} vs {rr}");
        let (ml, mr) = (luma(px_at(&img, w, 37.0, 30.0)), luma(px_at(&img, w, 43.0, 30.0)));
        assert!(ml > mr + 20.0, "metal, свет слева: {ml} vs {mr}");
        // Сфера: со стороны света (вверх-влево) светлее.
        let (sl, sr) = (luma(px_at(&img, w, 66.0, 19.0)), luma(px_at(&img, w, 74.0, 31.0)));
        assert!(sl > sr + 30.0, "sphere: {sl} vs {sr}");
        // Мягкая тень: внутри полная альфа, на краю — частичная, снаружи — ноль.
        let a_in = px_at(&img, w, 35.0, 80.0)[3];
        let a_edge = px_at(&img, w, 35.0, 85.0)[3];
        let a_out = px_at(&img, w, 35.0, 88.5)[3];
        assert!(a_in >= 125, "soft: внутри {a_in}");
        assert!(a_edge > 20 && a_edge < 110, "soft: край {a_edge}");
        assert_eq!(a_out, 0, "soft: снаружи");
        // Оплётка: вдоль оси яркость гуляет заметно сильнее, чем у гладкой трубки.
        let row: Vec<f32> = (0..80).map(|i| luma(px_at(&img, w, 20.0 + i as f32 * 0.5, 103.0))).collect();
        let (mn, mx) = row.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(mx - mn > 25.0, "braid: узора нет ({mn}..{mx})");
        // Плоская заливка без изменений.
        let p = px_at(&img, w, 80.0, 50.0);
        assert_eq!(p, [51, 153, 255, 255], "flat");

        // Свет справа: стороны меняются местами.
        r.set_lighting(RIGHT, 0.5);
        let img = r.render_to_rgba(&list);
        let (l, rr) = (luma(px_at(&img, w, 17.0, 30.0)), luma(px_at(&img, w, 23.0, 30.0)));
        assert!(rr > l + 20.0, "tube, свет справа: {l} vs {rr}");
        assert_eq!(px_at(&img, w, 80.0, 50.0), [51, 153, 255, 255], "flat");
    }

    /// Лист образцов всех видов → `SHADER_SHEET` (сырой RGBA поверх тёмного фона).
    /// `SHADER_SHEET=/path/sheet.rgba cargo test -- --ignored shader_sheet`
    #[test]
    #[ignore]
    fn shader_sheet() {
        let Ok(path) = std::env::var("SHADER_SHEET") else {
            return;
        };
        let (cw, ch) = (230u32, 250u32); // клетка в pt
        let scale = 2u32;
        let (pw, ph) = (cw * scale, ch * scale);
        let mut r = Renderer::headless(pw, ph, scale as f64);
        let (cols, rows) = (3u32, 2u32);
        let (sw, shh) = (pw * cols, ph * rows);
        let bg = [20u8, 20, 28];
        let mut sheet = vec![0u8; (sw * shh * 4) as usize];
        for (j, &light) in [LEFT, RIGHT].iter().enumerate() {
            for (i, &gloss) in [0.0f32, 0.5, 1.0].iter().enumerate() {
                r.set_lighting(light, gloss);
                let img = r.render_to_rgba(&sheet_list());
                for y in 0..ph {
                    for x in 0..pw {
                        let s = ((y * pw + x) * 4) as usize;
                        let d = (((j as u32 * ph + y) * sw + i as u32 * pw + x) * 4) as usize;
                        let a = img[s + 3] as f32 / 255.0;
                        for c in 0..3 {
                            sheet[d + c] = (img[s + c] as f32 * a + bg[c] as f32 * (1.0 - a)).round() as u8;
                        }
                        sheet[d + 3] = 255;
                    }
                }
            }
        }
        std::fs::write(&path, &sheet).unwrap();
        eprintln!("sheet {sw}x{shh} → {path}");
    }

    /// Образцы: трубки, оплётка (прямая и изогнутый хлыст), металл, сферы, тень, чёрная кожа.
    fn sheet_list() -> DrawList {
        let natural = hex(0x77461f, 1.0);
        let black = hex(0x121217, 1.0);
        let brass = hex(0xa9853b, 1.0);
        let mut l = DrawList::default();
        l.tube(Vec2::new(15.0, 15.0), Vec2::new(215.0, 15.0), 16.0, natural);
        l.braid(Vec2::new(15.0, 42.0), Vec2::new(215.0, 42.0), 16.0, natural, 0.6 * 16.0);
        // Хлыст: синусоида с сужением, сегменты ~6 pt, период подогнан под длину, фаза по дуге.
        let whip = |l: &mut DrawList, y0: f32, color: [f32; 4]| {
            let n = 36;
            let pts: Vec<Vec2> = (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    Vec2::new(15.0 + 200.0 * t, y0 + 14.0 * (t * 5.0).sin())
                })
                .collect();
            for i in 0..n {
                let t = (i as f32 + 0.5) / n as f32;
                let width = 9.0 - 7.0 * t;
                let (a, b) = (pts[i], pts[i + 1]);
                let len = a.distance(b);
                let m = (len / (0.6 * width)).round().max(1.0);
                let period = len / m;
                l.soft(a + Vec2::new(-3.0, 5.0), b + Vec2::new(-3.0, 5.0), width, [0.0, 0.0, 0.0, 0.3], 2.5);
                l.braid(a, b, width, color, period);
            }
        };
        whip(&mut l, 85.0, natural);
        // Металл, сферы.
        l.metal(Vec2::new(15.0, 130.0), Vec2::new(60.0, 130.0), 12.0, brass);
        l.metal(Vec2::new(80.0, 122.0), Vec2::new(80.0, 138.0), 5.0, brass);
        l.metal(Vec2::new(95.0, 122.0), Vec2::new(95.0, 138.0), 5.0, hex(0x565c67, 1.0));
        l.sphere(Vec2::new(125.0, 130.0), 11.0, brass);
        l.sphere(Vec2::new(160.0, 130.0), 11.0, hex(0x565c67, 1.0));
        l.push_styled_test(Vec2::new(195.0, 130.0), 7.0, natural);
        // Тень + трубка над ней.
        l.soft(Vec2::new(12.0, 172.0), Vec2::new(212.0, 172.0), 12.0, [0.0, 0.0, 0.0, 0.35], 3.0);
        l.tube(Vec2::new(15.0, 165.0), Vec2::new(215.0, 165.0), 12.0, hex(0x8a4719, 1.0));
        // Чёрная кожа: оплётка и хлыст.
        l.braid(Vec2::new(15.0, 195.0), Vec2::new(215.0, 195.0), 12.0, black, 0.6 * 12.0);
        whip(&mut l, 225.0, hex(0x5e1c24, 1.0));
        l
    }

    trait SphereMatte {
        fn push_styled_test(&mut self, c: Vec2, r: f32, color: [f32; 4]);
    }
    impl SphereMatte for DrawList {
        /// Матовая сфера (кожаный узел): param = 1.
        fn push_styled_test(&mut self, c: Vec2, r: f32, color: [f32; 4]) {
            self.shapes.push(Shape {
                a: c.to_array(),
                b: c.to_array(),
                radius: r,
                ring_width: 0.0,
                style: [crate::render::kind::SPHERE, 1.0],
                color,
            });
        }
    }
}
