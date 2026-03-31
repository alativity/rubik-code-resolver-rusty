use wgpu::util::DeviceExt;
use cgmath::*;
use crate::cube::{RubiksCube, Face, Move};
use std::time::Instant;

pub struct HudInfo {
    pub fps: u32,
    pub solver_status: String,
    pub solver_time_ms: u64,
    pub solver_depth: u32,
    pub solver_nodes: u64,
    pub current_move_name: String,
    pub moves_done: usize,
    pub moves_total: usize,
    pub queue_size: usize,
    /// Total logical CPU threads on this machine.
    pub cpu_logical: u32,
    /// Threads the solver actually uses
    pub solver_threads: u32,
    /// Per-button state for the control bar.
    pub buttons: Vec<ButtonInfo>,
}

/// Button state passed from the app to the renderer each frame.
pub struct ButtonInfo {
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub enabled: bool,
    pub hovered: bool,
}

// Button layout constants (all in physical pixels).
const BTN_H: f32 = 28.0;
const BTN_BOTTOM_OFFSET: f32 = 46.0;
const BTN_GAP: f32 = 8.0;
const BTN_MARGIN: f32 = 10.0;
/// Widths for: SCRAMBLE, SOLVE, PAUSE/RESUME, STOP, RESET
const BTN_WIDTHS: [f32; 5] = [108.0, 72.0, 88.0, 64.0, 72.0];

/// Compute (x, y, w, h) for each button given the window height.
pub fn button_rects(win_h: f32) -> [(f32, f32, f32, f32); 5] {
    let y = win_h - BTN_BOTTOM_OFFSET;
    let mut x = BTN_MARGIN;
    let mut out = [(0.0f32, 0.0, 0.0, BTN_H); 5];
    for (i, &bw) in BTN_WIDTHS.iter().enumerate() {
        out[i] = (x, y, bw, BTN_H);
        x += bw + BTN_GAP;
    }
    out
}

/// Format a large integer with K / M / G suffix (e.g. 1_086_366 → "1.0M").
fn fmt_num(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}G", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        format!("{}", n)
    }
}

fn font_glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0E,0x11,0x13,0x15,0x19,0x11,0x0E],
        '1' => [0x04,0x0C,0x04,0x04,0x04,0x04,0x0E],
        '2' => [0x0E,0x11,0x01,0x02,0x04,0x08,0x1F],
        '3' => [0x1F,0x02,0x04,0x02,0x01,0x11,0x0E],
        '4' => [0x02,0x06,0x0A,0x12,0x1F,0x02,0x02],
        '5' => [0x1F,0x10,0x1E,0x01,0x01,0x11,0x0E],
        '6' => [0x06,0x08,0x10,0x1E,0x11,0x11,0x0E],
        '7' => [0x1F,0x01,0x02,0x04,0x08,0x08,0x08],
        '8' => [0x0E,0x11,0x11,0x0E,0x11,0x11,0x0E],
        '9' => [0x0E,0x11,0x11,0x0F,0x01,0x02,0x0C],
        'A' => [0x0E,0x11,0x11,0x1F,0x11,0x11,0x11],
        'B' => [0x1E,0x11,0x11,0x1E,0x11,0x11,0x1E],
        'C' => [0x0E,0x11,0x10,0x10,0x10,0x11,0x0E],
        'D' => [0x1C,0x12,0x11,0x11,0x11,0x12,0x1C],
        'E' => [0x1F,0x10,0x10,0x1E,0x10,0x10,0x1F],
        'F' => [0x1F,0x10,0x10,0x1E,0x10,0x10,0x10],
        'G' => [0x0E,0x11,0x10,0x17,0x11,0x11,0x0F],
        'H' => [0x11,0x11,0x11,0x1F,0x11,0x11,0x11],
        'I' => [0x0E,0x04,0x04,0x04,0x04,0x04,0x0E],
        'J' => [0x07,0x02,0x02,0x02,0x02,0x12,0x0C],
        'K' => [0x11,0x12,0x14,0x18,0x14,0x12,0x11],
        'L' => [0x10,0x10,0x10,0x10,0x10,0x10,0x1F],
        'M' => [0x11,0x1B,0x15,0x15,0x11,0x11,0x11],
        'N' => [0x11,0x11,0x19,0x15,0x13,0x11,0x11],
        'O' => [0x0E,0x11,0x11,0x11,0x11,0x11,0x0E],
        'P' => [0x1E,0x11,0x11,0x1E,0x10,0x10,0x10],
        'Q' => [0x0E,0x11,0x11,0x11,0x15,0x12,0x0D],
        'R' => [0x1E,0x11,0x11,0x1E,0x14,0x12,0x11],
        'S' => [0x0F,0x10,0x10,0x0E,0x01,0x01,0x1E],
        'T' => [0x1F,0x04,0x04,0x04,0x04,0x04,0x04],
        'U' => [0x11,0x11,0x11,0x11,0x11,0x11,0x0E],
        'V' => [0x11,0x11,0x11,0x11,0x11,0x0A,0x04],
        'W' => [0x11,0x11,0x11,0x15,0x15,0x15,0x0A],
        'X' => [0x11,0x11,0x0A,0x04,0x0A,0x11,0x11],
        'Y' => [0x11,0x11,0x0A,0x04,0x04,0x04,0x04],
        'Z' => [0x1F,0x01,0x02,0x04,0x08,0x10,0x1F],
        'a' => [0x00,0x00,0x0E,0x01,0x0F,0x11,0x0F],
        'b' => [0x10,0x10,0x16,0x19,0x11,0x11,0x1E],
        'c' => [0x00,0x00,0x0E,0x10,0x10,0x11,0x0E],
        'd' => [0x01,0x01,0x0D,0x13,0x11,0x11,0x0F],
        'e' => [0x00,0x00,0x0E,0x11,0x1F,0x10,0x0E],
        'f' => [0x06,0x09,0x08,0x1C,0x08,0x08,0x08],
        'g' => [0x00,0x0F,0x11,0x11,0x0F,0x01,0x0E],
        'h' => [0x10,0x10,0x16,0x19,0x11,0x11,0x11],
        'i' => [0x04,0x00,0x0C,0x04,0x04,0x04,0x0E],
        'j' => [0x02,0x00,0x06,0x02,0x02,0x12,0x0C],
        'k' => [0x10,0x10,0x12,0x14,0x18,0x14,0x12],
        'l' => [0x0C,0x04,0x04,0x04,0x04,0x04,0x0E],
        'm' => [0x00,0x00,0x1A,0x15,0x15,0x11,0x11],
        'n' => [0x00,0x00,0x16,0x19,0x11,0x11,0x11],
        'o' => [0x00,0x00,0x0E,0x11,0x11,0x11,0x0E],
        'p' => [0x00,0x00,0x1E,0x11,0x1E,0x10,0x10],
        'q' => [0x00,0x00,0x0D,0x13,0x0F,0x01,0x01],
        'r' => [0x00,0x00,0x16,0x19,0x10,0x10,0x10],
        's' => [0x00,0x00,0x0E,0x10,0x0E,0x01,0x1E],
        't' => [0x08,0x08,0x1C,0x08,0x08,0x09,0x06],
        'u' => [0x00,0x00,0x11,0x11,0x11,0x13,0x0D],
        'v' => [0x00,0x00,0x11,0x11,0x11,0x0A,0x04],
        'w' => [0x00,0x00,0x11,0x11,0x15,0x15,0x0A],
        'x' => [0x00,0x00,0x11,0x0A,0x04,0x0A,0x11],
        'y' => [0x00,0x00,0x11,0x11,0x0F,0x01,0x0E],
        'z' => [0x00,0x00,0x1F,0x02,0x04,0x08,0x1F],
        ' ' => [0x00,0x00,0x00,0x00,0x00,0x00,0x00],
        ':' => [0x00,0x0C,0x0C,0x00,0x0C,0x0C,0x00],
        '.' => [0x00,0x00,0x00,0x00,0x00,0x0C,0x0C],
        ',' => [0x00,0x00,0x00,0x00,0x04,0x04,0x08],
        '/' => [0x00,0x01,0x02,0x04,0x08,0x10,0x00],
        '-' => [0x00,0x00,0x00,0x1F,0x00,0x00,0x00],
        '+' => [0x00,0x04,0x04,0x1F,0x04,0x04,0x00],
        '(' => [0x02,0x04,0x08,0x08,0x08,0x04,0x02],
        ')' => [0x08,0x04,0x02,0x02,0x02,0x04,0x08],
        '!' => [0x04,0x04,0x04,0x04,0x00,0x00,0x04],
        '\''=> [0x04,0x04,0x08,0x00,0x00,0x00,0x00],
        '%' => [0x18,0x19,0x02,0x04,0x08,0x13,0x03],
        '=' => [0x00,0x00,0x1F,0x00,0x1F,0x00,0x00],
        '[' => [0x0C,0x08,0x08,0x08,0x08,0x08,0x0C],
        ']' => [0x06,0x02,0x02,0x02,0x02,0x02,0x06],
        '#' => [0x0A,0x0A,0x1F,0x0A,0x1F,0x0A,0x0A],
        '>' => [0x10,0x08,0x04,0x02,0x04,0x08,0x10],
        '<' => [0x01,0x02,0x04,0x08,0x04,0x02,0x01],
        '*' => [0x00,0x0A,0x04,0x1F,0x04,0x0A,0x00],
        '@' => [0x0E,0x11,0x17,0x15,0x17,0x10,0x0E],
        '|' => [0x04,0x04,0x04,0x04,0x04,0x04,0x04],
        _   => [0x1F,0x11,0x11,0x11,0x11,0x11,0x1F],
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    overlay_bind_group: wgpu::BindGroup,
    depth_texture: wgpu::Texture,
    depth_texture_view: wgpu::TextureView,
    camera: Camera,
    start_time: Instant,
}

struct Camera {
    eye: Point3<f32>,
    target: Point3<f32>,
    up: Vector3<f32>,
    aspect: f32,
    fovy: f32,
    znear: f32,
    zfar: f32,
}

impl Camera {
    fn build_view_projection_matrix(&self) -> Matrix4<f32> {
        let view = Matrix4::look_at_rh(self.eye, self.target, self.up);
        let proj = cgmath::perspective(Deg(self.fovy), self.aspect, self.znear, self.zfar);
        proj * view
    }
}

impl Renderer {
    pub async fn new(window: std::sync::Arc<winit::window::Window>) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: Default::default(),
                    memory_hints: Default::default(),
                    label: None,
                    trace: Default::default(),
                },
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let camera = Camera {
            eye: Point3::new(5.0, 5.0, 5.0),
            target: Point3::new(0.0, 0.0, 0.0),
            up: Vector3::unit_y(),
            aspect: config.width as f32 / config.height as f32,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
        };

        let uniforms = Uniforms {
            view_proj: camera.build_view_projection_matrix().into(),
            model: Matrix4::identity().into(),
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::cast_slice(&[uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let uniform_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("uniform_bind_group_layout"),
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
                //label: Some("uniform_bind_group"),
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&uniform_bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Option::from("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Option::from("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            label: Some("depth_texture"),
            view_formats: &[],
        });

        let depth_texture_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let overlay_uniforms = Uniforms {
            view_proj: Matrix4::identity().into(),
            model: Matrix4::identity().into(),
        };
        let overlay_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Overlay Uniform Buffer"),
            contents: bytemuck::cast_slice(&[overlay_uniforms]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let overlay_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Overlay Bind Group"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: overlay_uniform_buffer.as_entire_binding(),
            }],
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Placeholder Vertex Buffer"),
            size: 0,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Placeholder Index Buffer"),
            size: 0,
            usage: wgpu::BufferUsages::INDEX,
            mapped_at_creation: false,
        });

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            vertex_buffer,
            index_buffer,
            index_count: 0,
            uniform_buffer,
            uniform_bind_group,
            overlay_bind_group,
            depth_texture,
            depth_texture_view,
            camera,
            start_time: Instant::now(),
        }
    }

    pub fn size(&self) -> winit::dpi::PhysicalSize<u32> { self.size }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            self.camera.aspect = self.config.width as f32 / self.config.height as f32;

            self.depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
                size: wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                label: Some("depth_texture"),
                view_formats: &[],
            });
            self.depth_texture_view = self.depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
        }
    }

    fn update(&mut self) {
        let time = self.start_time.elapsed().as_secs_f32();
        let angle = time * 30.0; // 30 degrees per second
        let rotation_axis = Vector3::new(1.0, 1.0, 1.0).normalize();
        let model = Matrix4::from_axis_angle(rotation_axis, Deg(angle));

        let uniforms = Uniforms {
            view_proj: self.camera.build_view_projection_matrix().into(),
            model: model.into(),
        };

        self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&[uniforms]));
    }

    fn update_cube_geometry(&mut self, cube: &RubiksCube, current_move: Option<Move>, animation_progress: f32) {
        let (vertex_buffer, index_buffer, index_count) = Self::create_cube_buffers_dynamic(cube, &self.device, current_move, animation_progress);
        self.vertex_buffer = vertex_buffer;
        self.index_buffer = index_buffer;
        self.index_count = index_count;
    }

    fn create_cube_buffers_dynamic(cube: &RubiksCube, device: &wgpu::Device, current_move: Option<Move>, progress: f32) -> (wgpu::Buffer, wgpu::Buffer, u32) {
        let cube_size = 2.0;
        let gap = 0.1; // Smaller gap for visible borders
        let small_cube_size = (cube_size - 2.0 * gap) / 3.0;

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let mut index_offset = 0u32;

        for x in 0..3 {
            for y in 0..3 {
                for z in 0..3 {
                    let px = (x as f32 - 1.5) * (small_cube_size + gap) + small_cube_size / 2.0;
                    let py = (y as f32 - 1.5) * (small_cube_size + gap) + small_cube_size / 2.0;
                    let pz = (z as f32 - 1.5) * (small_cube_size + gap) + small_cube_size / 2.0;

                    let in_layer = if let Some(m) = current_move {
                        match m.rotation_axis() {
                            Vector3 { x: 1.0, .. } if x == m.layer_index() => true,
                            Vector3 { y: 1.0, .. } if y == m.layer_index() => true,
                            Vector3 { z: 1.0, .. } if z == m.layer_index() => true,
                            _ => false,
                        }
                    } else { false };

                    let rotation_matrix = if in_layer && progress > 0.0 {
                        if let Some(m) = current_move {
                            let angle = Deg(m.rotation_angle_deg() * progress);
                            Matrix4::from_axis_angle(m.rotation_axis(), angle)
                        } else {
                            Matrix4::identity()
                        }
                    } else {
                        Matrix4::identity()
                    };

                    let up_color = if y == 2 { Self::face_to_color(cube.get_face(Face::Up)[x][z]) } else { [0.0, 0.0, 0.0] };
                    let down_color = if y == 0 { Self::face_to_color(cube.get_face(Face::Down)[x][2 - z]) } else { [0.0, 0.0, 0.0] };
                    let front_color = if z == 2 { Self::face_to_color(cube.get_face(Face::Front)[x][2 - y]) } else { [0.0, 0.0, 0.0] };
                    let back_color = if z == 0 { Self::face_to_color(cube.get_face(Face::Back)[x][y]) } else { [0.0, 0.0, 0.0] };
                    let right_color = if x == 2 { Self::face_to_color(cube.get_face(Face::Right)[2 - z][2 - y]) } else { [0.0, 0.0, 0.0] };
                    let left_color = if x == 0 { Self::face_to_color(cube.get_face(Face::Left)[z][2 - y]) } else { [0.0, 0.0, 0.0] };

                    let colors = [front_color, back_color, left_color, right_color, up_color, down_color];

                    Self::add_cube(&mut vertices, &mut indices, px, py, pz, small_cube_size / 2.0, &colors, index_offset, rotation_matrix);
                    index_offset += 24; // 4 verts per face * 6
                }
            }
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        (vertex_buffer, index_buffer, indices.len() as u32)
    }

    fn add_cube(vertices: &mut Vec<Vertex>, indices: &mut Vec<u32>, x: f32, y: f32, z: f32, r: f32, colors: &[[f32; 3]; 6], index_offset: u32, rotation_matrix: Matrix4<f32>) {
        let positions = [
            // Front
            [x - r, y - r, z + r], [x + r, y - r, z + r], [x + r, y + r, z + r], [x - r, y + r, z + r],
            // Back
            [x - r, y - r, z - r], [x + r, y - r, z - r], [x + r, y + r, z - r], [x - r, y + r, z - r],
            // Left
            [x - r, y - r, z - r], [x - r, y - r, z + r], [x - r, y + r, z + r], [x - r, y + r, z - r],
            // Right
            [x + r, y - r, z - r], [x + r, y - r, z + r], [x + r, y + r, z + r], [x + r, y + r, z - r],
            // Top
            [x - r, y + r, z - r], [x + r, y + r, z - r], [x + r, y + r, z + r], [x - r, y + r, z + r],
            // Bottom
            [x - r, y - r, z - r], [x + r, y - r, z - r], [x + r, y - r, z + r], [x - r, y - r, z + r],
        ];

        for face in 0..6 {
            let base = (face * 4) as usize;
            let color = colors[face];
            for i in 0..4 {
                let mut pos = positions[base + i];
                let rotated = rotation_matrix * vec4(pos[0], pos[1], pos[2], 1.0);
                pos = [rotated.x, rotated.y, rotated.z];
                vertices.push(Vertex {
                    position: pos,
                    color,
                });
            }
            let ind = [0, 1, 2, 0, 2, 3];
            for &i in &ind {
                indices.push(index_offset + (base as u32) + i);
            }
        }
    }

    fn face_to_color(face: Face) -> [f32; 3] {
        match face {
            Face::Up => [1.0, 1.0, 1.0],
            Face::Down => [1.0, 1.0, 0.0],
            Face::Front => [1.0, 0.0, 0.0],
            Face::Back => [1.0, 0.5, 0.0],
            Face::Right => [0.0, 1.0, 0.0],
            Face::Left => [0.0, 0.0, 1.0],
        }
    }

    fn generate_text_vertices(
        text: &str,
        start_x: f32,
        start_y: f32,
        scale: f32,
        color: [f32; 3],
        screen_w: f32,
        screen_h: f32,
    ) -> (Vec<Vertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let pixel = scale;
        let char_w = 5.0 * pixel;
        let gap = pixel;

        let mut cursor_x = start_x;

        for ch in text.chars() {
            let glyph = font_glyph(ch);
            for row in 0..7 {
                let bits = glyph[row];
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        let px = cursor_x + col as f32 * pixel;
                        let py = start_y + row as f32 * pixel;

                        let x0 = 2.0 * px / screen_w - 1.0;
                        let y0 = 1.0 - 2.0 * py / screen_h;
                        let x1 = 2.0 * (px + pixel) / screen_w - 1.0;
                        let y1 = 1.0 - 2.0 * (py + pixel) / screen_h;

                        let idx = vertices.len() as u32;
                        vertices.push(Vertex { position: [x0, y0, 0.0], color });
                        vertices.push(Vertex { position: [x1, y0, 0.0], color });
                        vertices.push(Vertex { position: [x1, y1, 0.0], color });
                        vertices.push(Vertex { position: [x0, y1, 0.0], color });
                        indices.extend_from_slice(&[idx, idx+1, idx+2, idx, idx+2, idx+3]);
                    }
                }
            }
            cursor_x += char_w + gap;
        }
        (vertices, indices)
    }

    /// Push a filled quad (in screen-pixel coords) into the vertex/index buffers.
    /// `z` = 0.001 for backgrounds, 0.0 for text (depth test Less ensures text wins).
    fn add_filled_rect(
        verts: &mut Vec<Vertex>,
        idxs:  &mut Vec<u32>,
        x0: f32, y0: f32, x1: f32, y1: f32,
        color: [f32; 3],
        sw: f32, sh: f32,
        z: f32,
    ) {
        let nx0 =  2.0 * x0 / sw - 1.0;
        let ny0 =  1.0 - 2.0 * y0 / sh;
        let nx1 =  2.0 * x1 / sw - 1.0;
        let ny1 =  1.0 - 2.0 * y1 / sh;
        let base = verts.len() as u32;
        verts.push(Vertex { position: [nx0, ny0, z], color });
        verts.push(Vertex { position: [nx1, ny0, z], color });
        verts.push(Vertex { position: [nx1, ny1, z], color });
        verts.push(Vertex { position: [nx0, ny1, z], color });
        idxs.extend_from_slice(&[base, base+1, base+2, base, base+2, base+3]);
    }

    fn build_hud_geometry(&self, hud: &HudInfo) -> (wgpu::Buffer, wgpu::Buffer, u32) {
        let w = self.size.width as f32;
        let h = self.size.height as f32;
        let scale = 2.0;
        let line_h = 9.0 * scale;   // pixels between line baselines
        let margin = 10.0;

        // Palette
        let green  = [0.1f32, 1.0, 0.3];
        let yellow = [1.0f32, 1.0, 0.2];
        let red    = [1.0f32, 0.3, 0.2];
        let cyan   = [0.3f32, 1.0, 1.0];
        let white  = [0.92f32,0.92,0.92];
        let gray   = [0.35f32,0.35,0.35];
        let orange = [1.0f32, 0.6, 0.0];
        let blue   = [0.4f32, 0.75, 1.0];
        let lime   = [0.7f32, 1.0, 0.2];

        let sep = "================".to_string();

        // Derived values
        let fps_color = if hud.fps >= 60 { green } else if hud.fps >= 30 { yellow } else { red };
        let time_s    = hud.solver_time_ms as f64 / 1000.0;

        // Solver phase line: "BFS  1.23s" or "done  13 moves  0.20s"
        let solver_line = if hud.solver_status == "done" || hud.solver_status == "failed" {
            format!(
                "{}  {} moves  {:.2}s",
                hud.solver_status, hud.moves_total, time_s
            )
        } else {
            format!("{}  {:.2}s", hud.solver_status, time_s)
        };

        // Nodes / depth line (only meaningful while solving)
        let nodes_line = format!(
            "Depth:{}  Nodes:{}",
            hud.solver_depth,
            fmt_num(hud.solver_nodes)
        );

        // Current move line
        let move_label = if hud.current_move_name.is_empty() {
            " -".to_string()
        } else {
            format!(" {}", hud.current_move_name)
        };
        let move_line = format!("Move:{}   {}/{}", move_label, hud.moves_done, hud.moves_total);

        // CPU section
        let pct = if hud.cpu_logical > 0 {
            hud.solver_threads * 100 / hud.cpu_logical
        } else {
            0
        };
        let cpu_line = format!("CPU: {} logical", hud.cpu_logical);
        let thr_line = format!(
            "Solver: {} threads ({}%)",
            hud.solver_threads, pct
        );

        let lines: Vec<(String, [f32; 3])> = vec![
            // ── FPS ──────────────────────────────────────────────────
            (format!("FPS: {}", hud.fps),          fps_color),
            (sep.clone(),                           gray),
            // ── SOLVER ───────────────────────────────────────────────
            ("[SOLVER]".to_string(),                orange),
            (solver_line,                           yellow),
            (nodes_line,                            white),
            (sep.clone(),                           gray),
            // ── SOLUTION ─────────────────────────────────────────────
            ("[SOLUTION]".to_string(),              lime),
            (move_line,                             cyan),
            (format!("Queue: {}", hud.queue_size), cyan),
            (sep.clone(),                           gray),
            // ── CPU ──────────────────────────────────────────────────
            ("[CPU]".to_string(),                   blue),
            (cpu_line,                              white),
            (thr_line,                              blue),
        ];

        let mut all_verts: Vec<Vertex> = Vec::new();
        let mut all_idxs: Vec<u32> = Vec::new();

        // ── 1. Button backgrounds (z=0.001 so text at z=0.0 passes depth-Less) ──
        for btn in &hud.buttons {
            let (bg, border) = if !btn.enabled {
                ([0.10f32, 0.10, 0.12], [0.22f32, 0.22, 0.24])
            } else if btn.hovered {
                ([0.35f32, 0.38, 0.50], [0.65f32, 0.72, 1.00])
            } else {
                ([0.18f32, 0.20, 0.28], [0.35f32, 0.40, 0.55])
            };
            // Outer border
            Self::add_filled_rect(
                &mut all_verts, &mut all_idxs,
                btn.x, btn.y, btn.x + btn.w, btn.y + btn.h,
                border, w, h, 0.001,
            );
            // Inner fill (2px inset)
            Self::add_filled_rect(
                &mut all_verts, &mut all_idxs,
                btn.x + 2.0, btn.y + 2.0, btn.x + btn.w - 2.0, btn.y + btn.h - 2.0,
                bg, w, h, 0.001,
            );
        }

        // ── 2. HUD text lines (top-left, z=0.0) ──────────────────────────────────
        for (i, (text, color)) in lines.iter().enumerate() {
            let y = margin + i as f32 * line_h;
            let (verts, idxs) = Self::generate_text_vertices(text, margin, y, scale, *color, w, h);
            let offset = all_verts.len() as u32;
            all_verts.extend(verts);
            all_idxs.extend(idxs.iter().map(|&idx| idx + offset));
        }

        // ── 3. Button label text (centered, z=0.0) ────────────────────────────────
        for btn in &hud.buttons {
            let char_advance = 6.0 * scale;  // 5px + 1px gap, at scale=2 → 12px
            let text_w = btn.label.len() as f32 * char_advance;
            let text_x = btn.x + (btn.w - text_w + scale) * 0.5;
            let text_y = btn.y + (btn.h - 7.0 * scale) * 0.5;
            let label_color: [f32; 3] = if btn.enabled {
                if btn.hovered { [1.0, 1.0, 1.0] } else { [0.85, 0.90, 1.0] }
            } else {
                [0.35, 0.35, 0.40]
            };
            let (verts, idxs) = Self::generate_text_vertices(
                btn.label, text_x, text_y, scale, label_color, w, h,
            );
            let offset = all_verts.len() as u32;
            all_verts.extend(verts);
            all_idxs.extend(idxs.iter().map(|&idx| idx + offset));
        }

        if all_verts.is_empty() {
            let empty_vb = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Empty Text VB"),
                size: 4,
                usage: wgpu::BufferUsages::VERTEX,
                mapped_at_creation: false,
            });
            let empty_ib = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Empty Text IB"),
                size: 4,
                usage: wgpu::BufferUsages::INDEX,
                mapped_at_creation: false,
            });
            return (empty_vb, empty_ib, 0);
        }

        let vb = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text VB"),
            contents: bytemuck::cast_slice(&all_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ib = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Text IB"),
            contents: bytemuck::cast_slice(&all_idxs),
            usage: wgpu::BufferUsages::INDEX,
        });
        (vb, ib, all_idxs.len() as u32)
    }

    pub fn render(&mut self, cube: &RubiksCube, current_move: Option<Move>, animation_progress: f32, hud: &HudInfo) -> Result<(), wgpu::SurfaceError> {
        self.update();

        self.update_cube_geometry(cube, current_move, animation_progress);
        let (text_vb, text_ib, text_ic) = self.build_hud_geometry(hud);

        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.render_pipeline);

            render_pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..self.index_count, 0, 0..1);

            if text_ic > 0 {
                render_pass.set_bind_group(0, &self.overlay_bind_group, &[]);
                render_pass.set_vertex_buffer(0, text_vb.slice(..));
                render_pass.set_index_buffer(text_ib.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..text_ic, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}