use std::sync::Arc;

use winit::dpi::PhysicalSize;
use winit::window::Window;

use super::{DrawList, Shape};

/// Uniform для шейдера: размер окна в точках и масштаб.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    viewport: [f32; 2],
    px_per_pt: f32,
    _pad: f32,
}

const SHAPE_SIZE: u64 = std::mem::size_of::<Shape>() as u64;

pub struct Renderer {
    /// Держим окно живым, пока жива surface (на Windows она может быть создана не из окна).
    _window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    scale_factor: f64,
    pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize, // в инстансах
    /// DirectComposition-визуал, если surface создана через него.
    #[cfg(windows)]
    composition: Option<crate::platform::Composition>,
}

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
        // Uniform-буфер и bind group.
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
        // Раскладка Shape: a@0, b@8, radius+ring_width@16, _pad@24, color@32.
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

        let capacity = 256;
        let instances = Self::create_instances(&device, capacity);

        let mut r = Self {
            _window: window,
            surface,
            device,
            queue,
            config,
            scale_factor,
            pipeline,
            globals_buf,
            bind_group,
            instances,
            capacity,
            #[cfg(windows)]
            composition,
        };
        r.configure_surface();
        r.write_globals();
        r
    }

    /// (Пере)конфигурирует swapchain; на Windows с DirectComposition — ещё и Commit,
    /// иначе новый swapchain (SetContent) не появится на экране.
    fn configure_surface(&self) {
        self.surface.configure(&self.device, &self.config);
        #[cfg(windows)]
        if let Some(c) = &self.composition {
            c.commit();
        }
    }

    /// Вызывать на `WindowEvent::Resized` и `ScaleFactorChanged`.
    pub fn resize(&mut self, size: PhysicalSize<u32>, scale_factor: f64) {
        self.scale_factor = scale_factor;
        if size.width > 0 && size.height > 0 {
            self.config.width = size.width;
            self.config.height = size.height;
            self.configure_surface();
        }
        self.write_globals();
    }

    /// Очищает кадр в полностью прозрачный цвет и рисует все фигуры.
    pub fn render(&mut self, list: &DrawList) {
        let frame = match self.surface.get_current_texture() {
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
                    view: &view,
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
        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    fn create_instances(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shapes"),
            size: capacity as u64 * SHAPE_SIZE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Viewport в точках = физический размер / scale factor.
    fn write_globals(&mut self) {
        let s = self.scale_factor as f32;
        let g = Globals {
            viewport: [self.config.width as f32 / s, self.config.height as f32 / s],
            px_per_pt: s,
            _pad: 0.0,
        };
        self.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));
    }
}
