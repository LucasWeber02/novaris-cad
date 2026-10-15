//! 3D viewport: renders the part offscreen with wgpu, shown as an egui image

use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};
use nova_kernel::RenderMesh;
use wgpu::util::DeviceExt;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const SHADER: &str = r#"
struct U {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
    base_color: vec4<f32>,
    sel: vec4<i32>,
    line_color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct MeshIn {
    @location(0) pos: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) face: u32,
};
struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nor: vec3<f32>,
    @location(1) @interpolate(flat) face: u32,
};

@vertex
fn vs_mesh(v: MeshIn) -> MeshOut {
    var o: MeshOut;
    o.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    o.nor = v.nor;
    o.face = v.face;
    return o;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    let n = normalize(i.nor);
    let l = normalize(u.light_dir.xyz);
    var c = u.base_color.rgb;
    if (i32(i.face) == u.sel.x) {
        c = vec3<f32>(0.25, 0.55, 1.0);
    } else if (i32(i.face) == u.sel.y) {
        c = mix(c, vec3<f32>(1.0, 0.55, 0.15), 0.45);
    }
    let diffuse = max(dot(n, l), 0.0);
    let h = normalize(l + vec3<f32>(0.0, 0.0, 1.0));
    let spec = pow(max(dot(n, l), 0.0), 40.0) * 0.25;
    return vec4<f32>(c * (0.28 + 0.72 * diffuse) + vec3<f32>(spec), 1.0);
}

@vertex
fn vs_line(@location(0) pos: vec3<f32>) -> @builtin(position) vec4<f32> {
    var clip = u.view_proj * vec4<f32>(pos, 1.0);
    // Pull edges slightly towards the viewer so they win the depth test
    clip.z = clip.z - 0.0008 * clip.w;
    return clip;
}

@fragment
fn fs_line() -> @location(0) vec4<f32> {
    return u.line_color;
}

struct BgOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) t: f32,
};

@vertex
fn vs_bg(@builtin(vertex_index) i: u32) -> BgOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var o: BgOut;
    o.clip = vec4<f32>(x, y, 1.0, 1.0);
    o.t = y * 0.5 + 0.5;
    return o;
}

@fragment
fn fs_bg(i: BgOut) -> @location(0) vec4<f32> {
    let top = vec3<f32>(0.52, 0.60, 0.72);
    let bottom = vec3<f32>(0.93, 0.94, 0.96);
    return vec4<f32>(mix(bottom, top, clamp(i.t, 0.0, 1.0)), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
    base_color: [f32; 4],
    sel: [i32; 4],
    line_color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    nor: [f32; 3],
    face: u32,
}

/// What to draw this frame
pub struct FrameParams {
    pub view_proj: glam::Mat4,
    /// Direction towards the light (world)
    pub light_dir: glam::Vec3,
    pub selected_face: Option<u32>,
    pub hover_face: Option<u32>,
}

struct Target {
    size: [u32; 2],
    color: wgpu::TextureView,
    depth: wgpu::TextureView,
}

pub struct Viewport {
    mesh_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    bg_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    mesh: Option<(wgpu::Buffer, u32)>,
    edges: Option<(wgpu::Buffer, u32)>,
    target: Option<Target>,
    texture_id: Option<egui::TextureId>,
}

impl Viewport {
    pub fn new(rs: &egui_wgpu::RenderState) -> Self {
        let device = &rs.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport"),
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
            label: Some("viewport"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout<'_>],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool,
                        compare: wgpu::CompareFunction| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: vs,
                    buffers,
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth_write,
                    depth_compare: compare,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: fs,
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview: None,
            })
        };

        let mesh_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32],
        };
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        };
        let mesh_pipeline = pipeline(
            "mesh",
            "vs_mesh",
            "fs_mesh",
            &[mesh_layout],
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::CompareFunction::Less,
        );
        let line_pipeline = pipeline(
            "edges",
            "vs_line",
            "fs_line",
            &[line_layout],
            wgpu::PrimitiveTopology::LineList,
            true,
            wgpu::CompareFunction::LessEqual,
        );
        let bg_pipeline = pipeline(
            "background",
            "vs_bg",
            "fs_bg",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            false,
            wgpu::CompareFunction::Always,
        );

        Viewport {
            mesh_pipeline,
            line_pipeline,
            bg_pipeline,
            uniforms,
            bind_group,
            mesh: None,
            edges: None,
            target: None,
            texture_id: None,
        }
    }

    /// Upload a new tessellation (or clear it)
    pub fn set_mesh(&mut self, rs: &egui_wgpu::RenderState, mesh: Option<&RenderMesh>) {
        let Some(mesh) = mesh.filter(|m| !m.indices.is_empty()) else {
            self.mesh = None;
            self.edges = None;
            return;
        };
        let vertices: Vec<MeshVertex> = mesh
            .indices
            .iter()
            .enumerate()
            .map(|(k, &i)| MeshVertex {
                pos: mesh.positions[i as usize],
                nor: mesh.normals[i as usize],
                face: mesh.triangle_faces[k / 3],
            })
            .collect();
        let buffer = rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.mesh = Some((buffer, vertices.len() as u32));

        let lines: Vec<[f32; 3]> = mesh
            .edges
            .iter()
            .flat_map(|poly| poly.windows(2).flat_map(|w| [w[0], w[1]]))
            .collect();
        self.edges = (!lines.is_empty()).then(|| {
            let buffer = rs.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edges"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, lines.len() as u32)
        });
    }

    /// Render at `size` pixels and return the egui texture showing the result
    pub fn paint(&mut self, rs: &egui_wgpu::RenderState, size: [u32; 2], params: &FrameParams) -> egui::TextureId {
        let size = [size[0].clamp(1, 8192), size[1].clamp(1, 8192)];
        let device = &rs.device;
        if self.target.as_ref().map(|t| t.size) != Some(size) {
            let make = |format, label| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let target = Target { size, color: make(COLOR_FORMAT, "viewport color"), depth: make(DEPTH_FORMAT, "viewport depth") };
            let mut renderer = rs.renderer.write();
            match self.texture_id {
                Some(id) => renderer.update_egui_texture_from_wgpu_texture(device, &target.color, wgpu::FilterMode::Linear, id),
                None => {
                    self.texture_id = Some(renderer.register_native_texture(device, &target.color, wgpu::FilterMode::Linear))
                }
            }
            self.target = Some(target);
        }
        let target = self.target.as_ref().expect("target created above");

        let to_i = |f: Option<u32>| f.map(|v| v as i32).unwrap_or(-1);
        let u = Uniforms {
            view_proj: params.view_proj.to_cols_array_2d(),
            light_dir: params.light_dir.extend(0.0).to_array(),
            base_color: [0.62, 0.64, 0.68, 1.0],
            sel: [to_i(params.selected_face), to_i(params.hover_face), 0, 0],
            line_color: [0.02, 0.02, 0.03, 1.0],
        };
        rs.queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("viewport") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_pipeline(&self.bg_pipeline);
            pass.draw(0..3, 0..1);
            if let Some((buffer, count)) = &self.mesh {
                pass.set_pipeline(&self.mesh_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*count, 0..1);
            }
            if let Some((buffer, count)) = &self.edges {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*count, 0..1);
            }
        }
        rs.queue.submit(Some(encoder.finish()));
        self.texture_id.expect("registered above")
    }
}
