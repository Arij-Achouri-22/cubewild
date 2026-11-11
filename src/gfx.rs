//! wgpu renderer: chunk pass (opaque + transparent), entity pass, HUD pass.

use crate::textures;
use crate::world::MeshData;
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::sync::Arc;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct EntVert {
    pub pos: [f32; 3],
    pub color: [u8; 4],
}

/// A textured vertex for items lying in the world (camera-relative position).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ItemVert {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub layer: u32,
    pub light: f32,
    /// 0..1 share of daylight reaching it
    pub sky: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct UiVert {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
    pub layer: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    fog: [f32; 4],
    /// x = daylight, y = light count, z = entity brightness
    misc: [f32; 4],
    lights: [[f32; 4]; MAX_LIGHTS],
}

pub const MAX_LIGHTS: usize = 16;
/// EntVert alpha that marks a vertex as glowing (drawn opaque, ignores darkness)
pub const EMISSIVE: u8 = 250;

pub struct ChunkMesh {
    vbuf: wgpu::Buffer,
    opaque_quads: u32,
    /// the last of the opaque quads: buried cave walls (see `MeshData::deep_quads`)
    deep_quads: u32,
    cut_quads: u32,
    trans_quads: u32,
    /// shaped blocks, in their own vertex format (see `MeshData::detail`)
    detail: Option<(wgpu::Buffer, u32)>,
    pub top: f32,
}

impl ChunkMesh {
    /// Quads drawn for this chunk, with or without its buried cave walls.
    pub fn quads(&self, deep: bool) -> u32 {
        self.opaque_quads - if deep { 0 } else { self.deep_quads } + self.cut_quads + self.trans_quads + self.detail.as_ref().map_or(0, |d| d.1)
    }
}

pub struct Frame<'a> {
    pub view_proj: Mat4,
    /// linear RGB
    pub fog_color: [f32; 3],
    pub fog_start: f32,
    pub fog_end: f32,
    /// light everywhere regardless of the sky (the Emberdeep), 0..1
    pub ambient: f32,
    /// visible chunks sorted near -> far, with camera-relative origin
    /// (mesh, origin, draw its plants, draw its buried cave walls)
    pub chunks: &'a [(&'a ChunkMesh, [f32; 3], bool, bool)],
    /// 0..1 brightness of the sky
    pub daylight: f32,
    /// brightness of animals and monsters
    pub ent_light: f32,
    /// height of the camera in the world (the deep is darker)
    pub eye_y: f32,
    /// camera-relative light positions, w = reach in blocks
    pub lights: &'a [[f32; 4]],
    /// sun, moon and stars: drawn first, behind everything
    pub sky: &'a [EntVert],
    pub ents: &'a [EntVert],
    /// dropped items: quads of four vertices
    pub items: &'a [ItemVert],
    /// how many of the last `items` vertices are the item in the player's hand
    pub hand: usize,
    pub ui: &'a [UiVert],
}

const MAX_QUADS: u32 = 1 << 17;
const MAX_INSTANCES: usize = 2048;
/// side length of the square glyph atlas
pub const FONT_ATLAS: u32 = 1024;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const SHADER: &str = r#"
struct Globals { view_proj: mat4x4<f32>, fog_color: vec4<f32>, fog: vec4<f32>, misc: vec4<f32>, lights: array<vec4<f32>, 16> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var tex: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var ftex: texture_2d<f32>;
@group(0) @binding(4) var fsamp: sampler;

fn fogf(d: f32) -> f32 { return clamp((d - g.fog.x) / (g.fog.y - g.fog.x), 0.0, 1.0); }

struct COut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) layer: u32,
  @location(2) light: f32,
  @location(3) p: vec3<f32>,
  @location(4) sky: f32,
};

@vertex fn vs_chunk(@location(0) v: u32, @location(1) off: vec4<f32>) -> COut {
  let xz = v & 511u;
  let x = f32(xz % 17u); let y = f32((v >> 9u) & 511u); let z = f32(xz / 17u);
  let face = (v >> 18u) & 7u;
  var ao = f32((v >> 28u) & 3u);
  var p = vec3<f32>(x, y, z) + off.xyz;
  var o: COut;
  o.layer = (v >> 21u) & 127u;
  if (o.layer == 12u || o.layer == 56u) {
    // water and lava: the shading bits say how far this corner of the surface sinks
    p.y -= (3.0 - ao) * 0.25;
    ao = 3.0;
  }
  if (face == 7u) {
    // window panes: an upright quad moved from the cell boundary to the middle of the cell
    var dirs = array<vec3<f32>, 4>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 0.0, -1.0));
    p += dirs[u32(ao)] * 0.5;
  }
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  if (face == 7u) {
    if (ao < 2.0) { o.uv = vec2<f32>(z, -y); } else { o.uv = vec2<f32>(x, -y); }
    ao = 3.0;
  }
  else if (face < 2u) { o.uv = vec2<f32>(z, -y); }
  else if (face < 4u) { o.uv = vec2<f32>(x, z); }
  else { o.uv = vec2<f32>(x, -y); }
  var shade = array<f32, 8>(0.80, 0.80, 1.0, 0.50, 0.66, 0.66, 0.95, 0.95);
  o.light = shade[face] * (0.45 + ao * 0.1834);
  o.p = p;
  var skyl = array<f32, 4>(0.2, 0.52, 1.0, 1.0);
  o.sky = skyl[(v >> 30u) & 3u];
  return o;
}

// shaped blocks: positions in sixteenths of a block, textures laid out by world position
@vertex fn vs_detail(@location(0) v: vec2<u32>, @location(1) off: vec4<f32>) -> COut {
  let q = vec3<f32>(f32(v.x & 511u), f32((v.x >> 9u) & 8191u), f32((v.x >> 22u) & 511u)) / 16.0;
  let face = v.y & 7u;
  let p = q + off.xyz;
  var o: COut;
  o.layer = (v.y >> 3u) & 255u;
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  if (face < 2u) { o.uv = vec2<f32>(q.z, -q.y); }
  else if (face < 4u) { o.uv = vec2<f32>(q.x, q.z); }
  else { o.uv = vec2<f32>(q.x, -q.y); }
  var shade = array<f32, 6>(0.80, 0.80, 1.0, 0.50, 0.66, 0.66);
  o.light = shade[face];
  o.p = p;
  var skyl = array<f32, 4>(0.2, 0.52, 1.0, 1.0);
  o.sky = skyl[(v.y >> 11u) & 3u];
  return o;
}
@vertex fn vs_item(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) light: f32, @location(4) sky: f32) -> COut {
  var o: COut;
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  o.uv = uv;
  o.layer = layer;
  o.light = light;
  o.p = p;
  o.sky = sky;
  return o;
}
// the arms and the held item: squeezed to the front of the depth range, so they sort
// among themselves but nothing in the world cuts into them
@vertex fn vs_hand(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) layer: u32, @location(3) light: f32, @location(4) sky: f32) -> COut {
  var o: COut;
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  o.pos.z = o.pos.z * 0.05;
  o.uv = uv;
  o.layer = layer;
  o.light = light;
  o.p = p;
  o.sky = sky;
  return o;
}

fn chunk_color(i: COut, c: vec4<f32>) -> vec4<f32> {
  var glow = 0.0;
  let n = i32(g.misc.y);
  for (var k = 0; k < n; k++) {
    let l = g.lights[k];
    glow = max(glow, 1.0 - distance(i.p, l.xyz) / l.w);
  }
  glow = glow * glow * 1.15;
  // what daylight seeps into enclosed places fades out on the way down
  // (world heights: the old y 12..44, lifted onto the deep slate by 64)
  let dim = mix(0.1, 1.0, smoothstep(76.0, 108.0, i.p.y + g.misc.w));
  var sky = i.sky;
  if (sky < 0.6) { sky = sky * dim; }
  // (the Emberdeep has no daylight: a warm glow fills it instead, fog.z strong)
  let ambient = vec3<f32>(1.0, 0.78, 0.62) * g.fog.z;
  var lit = min(max(vec3<f32>(max(sky * g.misc.x, 0.035 * mix(0.3, 1.0, dim))), ambient) + vec3<f32>(1.0, 0.8, 0.52) * glow, vec3<f32>(1.15)) * i.light;
  if (i.layer == 27u) { lit = vec3<f32>(1.0, 0.97, 0.9); }
  // sunshard and glowcap shine; smouldering rock glows dimly
  if (i.layer == 106u || i.layer == 108u) { lit = vec3<f32>(1.05, 0.98, 0.9); }
  if (i.layer == 101u) { lit = max(lit, vec3<f32>(0.75, 0.6, 0.5)); }
  // pyre ore smoulders faintly, so it can be found in the dark
  if (i.layer == 114u) { lit = max(lit, vec3<f32>(0.62, 0.42, 0.42)); }
  if (i.layer == 56u) { lit = vec3<f32>(1.0, 0.92, 0.85); }
  return vec4<f32>(mix(c.rgb * lit, g.fog_color.rgb, fogf(length(i.p))), c.a);
}
@fragment fn fs_chunk(i: COut) -> @location(0) vec4<f32> {
  return chunk_color(i, textureSample(tex, samp, i.uv, i.layer));
}
@fragment fn fs_chunk_cut(i: COut) -> @location(0) vec4<f32> {
  let c = textureSampleLevel(tex, samp, i.uv, i.layer, 0.0);
  if (c.a < 0.5) { discard; }
  return chunk_color(i, vec4<f32>(c.rgb, 1.0));
}

struct EOut { @builtin(position) pos: vec4<f32>, @location(0) color: vec4<f32>, @location(1) dist: f32 };

@vertex fn vs_ent(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> EOut {
  var o: EOut;
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  let glowing = c.a > 0.97 && c.a < 0.99;
  let k = select(g.misc.z, 1.0, glowing);
  o.color = vec4<f32>(pow(c.rgb, vec3<f32>(2.2)) * k, select(c.a, 1.0, glowing));
  o.dist = length(p);
  return o;
}
@fragment fn fs_ent(i: EOut) -> @location(0) vec4<f32> {
  return vec4<f32>(mix(i.color.rgb, g.fog_color.rgb, fogf(i.dist)), i.color.a);
}
@vertex fn vs_sky(@location(0) p: vec3<f32>, @location(1) c: vec4<f32>) -> EOut {
  var o: EOut;
  o.pos = g.view_proj * vec4<f32>(p, 1.0);
  o.color = vec4<f32>(pow(c.rgb, vec3<f32>(2.2)), c.a);
  o.dist = 0.0;
  return o;
}
@fragment fn fs_sky(i: EOut) -> @location(0) vec4<f32> { return i.color; }

struct UOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) color: vec4<f32>,
  @location(2) @interpolate(flat) layer: i32,
};
@vertex fn vs_ui(@location(0) p: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) c: vec4<f32>, @location(3) layer: f32) -> UOut {
  var o: UOut;
  o.pos = vec4<f32>(p, 0.0, 1.0);
  o.uv = uv;
  o.color = vec4<f32>(pow(c.rgb, vec3<f32>(2.2)), c.a);
  o.layer = i32(layer);
  return o;
}
@fragment fn fs_ui(i: UOut) -> @location(0) vec4<f32> {
  let t = textureSampleLevel(tex, samp, i.uv, max(i.layer, 0), 0.0);
  let f = textureSampleLevel(ftex, fsamp, i.uv, 0.0).r;
  if (i.layer == -2) { return vec4<f32>(i.color.rgb, i.color.a * f); }
  if (i.layer < 0) { return i.color; }
  return vec4<f32>(t.rgb * i.color.rgb, i.color.a * t.a);
}
"#;

const BLIT_SHADER: &str = r#"
@group(0) @binding(0) var t: texture_2d<f32>;
@group(0) @binding(1) var s: sampler;
struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> O {
  var o: O;
  let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
  o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
  o.uv = vec2<f32>(p.x, 1.0 - p.y);
  return o;
}
@fragment fn fs(i: O) -> @location(0) vec4<f32> { return textureSampleLevel(t, s, i.uv, 0.0); }
"#;

/// Reduced-resolution target the world is drawn into before being stretched to the window.
struct Scene {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

pub struct Gfx {
    /// fraction of the window resolution the world is rendered at (the HUD stays sharp)
    scale: f32,
    scene: Option<Scene>,
    blit_pipe: wgpu::RenderPipeline,
    blit_bgl: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: wgpu::TextureView,
    chunk_pipe: wgpu::RenderPipeline,
    trans_pipe: wgpu::RenderPipeline,
    cut_pipe: wgpu::RenderPipeline,
    detail_pipe: wgpu::RenderPipeline,
    item_pipe: wgpu::RenderPipeline,
    hand_pipe: wgpu::RenderPipeline,
    item_buf: wgpu::Buffer,
    ent_pipe: wgpu::RenderPipeline,
    sky_pipe: wgpu::RenderPipeline,
    sky_buf: wgpu::Buffer,
    ui_pipe: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    bind: wgpu::BindGroup,
    quad_ibuf: wgpu::Buffer,
    inst_buf: wgpu::Buffer,
    ent_buf: wgpu::Buffer,
    ui_buf: wgpu::Buffer,
    inst_scratch: Vec<[f32; 4]>,
    font_tex: wgpu::Texture,
    pub vsync: bool,
    pub adapter_name: String,
}

fn depth_view(device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn vbuf(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Gfx {
    pub async fn new(window: Arc<Window>, vsync: bool) -> Gfx {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .expect("no GPU adapter");
        let adapter_name = format!("{} ({:?})", adapter.get_info().name, adapter.get_info().backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .expect("request device");

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .expect("surface unsupported");
        let caps = surface.get_capabilities(&adapter);
        if let Some(f) = caps.formats.iter().find(|f| f.is_srgb()) {
            config.format = *f;
        }
        config.present_mode =
            if vsync { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        // block texture array
        let mips = textures::build();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: textures::TEX as u32,
                height: textures::TEX as u32,
                depth_or_array_layers: textures::LAYERS as u32,
            },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, (sz, bytes)) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * sz),
                    rows_per_image: Some(*sz),
                },
                wgpu::Extent3d {
                    width: *sz,
                    height: *sz,
                    depth_or_array_layers: textures::LAYERS as u32,
                },
            );
        }
        let tex_view = tex.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // glyph atlas for UI text (single channel coverage)
        let font_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: FONT_ATLAS, height: FONT_ATLAS, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let font_view = font_tex.create_view(&Default::default());
        let font_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&font_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&font_sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });

        let chunk_attrs = wgpu::vertex_attr_array![0 => Uint32];
        let inst_attrs = wgpu::vertex_attr_array![1 => Float32x4];
        let ent_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4];
        let ui_attrs =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32];
        let chunk_bufs = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 4,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &chunk_attrs,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &inst_attrs,
            }),
        ];
        let detail_attrs = wgpu::vertex_attr_array![0 => Uint32x2];
        let detail_bufs = [
            Some(wgpu::VertexBufferLayout {
                array_stride: 8,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &detail_attrs,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &inst_attrs,
            }),
        ];
        let item_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Uint32, 3 => Float32, 4 => Float32];
        let item_bufs = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ItemVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &item_attrs,
        })];
        let ent_bufs = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<EntVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ent_attrs,
        })];
        let ui_bufs = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVert>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ui_attrs,
        })];

        let format = config.format;
        let make = |vs: &str,
                    fs: &str,
                    bufs: &[Option<wgpu::VertexBufferLayout>],
                    blend: bool,
                    cull: bool,
                    depth: Option<bool>,
                    has_depth: bool| {
            let compare = if depth.is_some() { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Always };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(vs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: bufs,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: if cull { Some(wgpu::Face::Back) } else { None },
                    ..Default::default()
                },
                depth_stencil: has_depth.then(|| wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth.unwrap_or(false)),
                    depth_compare: Some(compare),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: if blend { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None },
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let chunk_pipe = make("vs_chunk", "fs_chunk", &chunk_bufs, false, true, Some(true), true);
        let cut_pipe = make("vs_chunk", "fs_chunk_cut", &chunk_bufs, false, false, Some(true), true);
        let detail_pipe = make("vs_detail", "fs_chunk_cut", &detail_bufs, false, false, Some(true), true);
        let item_pipe = make("vs_item", "fs_chunk_cut", &item_bufs, false, false, Some(true), true);
        // the held item is drawn over the world, never cut into by a wall
        let hand_pipe = make("vs_hand", "fs_chunk_cut", &item_bufs, false, false, Some(true), true);
        let trans_pipe = make("vs_chunk", "fs_chunk", &chunk_bufs, true, false, Some(false), true);
        let ent_pipe = make("vs_ent", "fs_ent", &ent_bufs, true, false, Some(true), true);
        let sky_pipe = make("vs_sky", "fs_sky", &ent_bufs, true, false, None, true);
        let ui_pipe = make("vs_ui", "fs_ui", &ui_bufs, true, false, None, false);

        // upscale pass for the reduced-resolution scene
        let blit_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(BLIT_SHADER.into()),
        });
        let blit_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&blit_bgl)],
            immediate_size: 0,
        });
        let blit_pipe = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&blit_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // shared index buffer: every chunk mesh is a list of quads
        let mut idx: Vec<u32> = Vec::with_capacity(MAX_QUADS as usize * 6);
        for q in 0..MAX_QUADS {
            let b = q * 4;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let quad_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (idx.len() * 4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&quad_ibuf, 0, bytemuck::cast_slice(&idx));

        let depth = depth_view(&device, config.width, config.height);
        Gfx {
            scale: 1.0,
            scene: None,
            blit_pipe,
            blit_bgl,
            blit_sampler,
            inst_buf: vbuf(&device, (MAX_INSTANCES * 16) as u64),
            ent_buf: vbuf(&device, 1 << 20),
            item_buf: vbuf(&device, 1 << 16),
            detail_pipe,
            item_pipe,
            hand_pipe,
            sky_buf: vbuf(&device, 1 << 16),
            sky_pipe,
            ui_buf: vbuf(&device, 1 << 20),
            surface,
            device,
            queue,
            config,
            depth,
            chunk_pipe,
            trans_pipe,
            cut_pipe,
            ent_pipe,
            ui_pipe,
            globals,
            bind,
            quad_ibuf,
            inst_scratch: Vec::with_capacity(MAX_INSTANCES),
            font_tex,
            vsync,
            adapter_name,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.config.width = w;
        self.config.height = h;
        self.surface.configure(&self.device, &self.config);
        self.rebuild_targets();
    }

    /// Sets the share of the window resolution the world is rendered at (0.5..1).
    pub fn set_scale(&mut self, scale: f32) {
        let scale = scale.clamp(0.5, 1.0);
        if (scale - self.scale).abs() > 0.001 {
            self.scale = scale;
            self.rebuild_targets();
        }
    }

    fn rebuild_targets(&mut self) {
        let (w, h) = (self.config.width, self.config.height);
        if self.scale > 0.995 {
            self.scene = None;
            self.depth = depth_view(&self.device, w, h);
            return;
        }
        let (sw, sh) = (((w as f32 * self.scale).round() as u32).max(1), ((h as f32 * self.scale).round() as u32).max(1));
        self.depth = depth_view(&self.device, sw, sh);
        let view = self
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.blit_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.blit_sampler) },
            ],
        });
        self.scene = Some(Scene { view, bind });
    }

    pub fn set_vsync(&mut self, on: bool) {
        self.vsync = on;
        self.config.present_mode =
            if on { wgpu::PresentMode::AutoVsync } else { wgpu::PresentMode::AutoNoVsync };
        self.surface.configure(&self.device, &self.config);
    }

    /// Replaces the glyph atlas (FONT_ATLAS x FONT_ATLAS, one byte per pixel).
    pub fn upload_font(&self, pixels: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.font_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(FONT_ATLAS),
                rows_per_image: Some(FONT_ATLAS),
            },
            wgpu::Extent3d { width: FONT_ATLAS, height: FONT_ATLAS, depth_or_array_layers: 1 },
        );
    }

    pub fn upload_mesh(&self, m: &MeshData) -> Option<ChunkMesh> {
        if m.verts.is_empty() && m.detail.is_empty() {
            return None;
        }
        let vbuf = vbuf(&self.device, (m.verts.len().max(1) * 4) as u64);
        self.queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&m.verts));
        let detail = (!m.detail.is_empty()).then(|| {
            let buf = self::vbuf(&self.device, (m.detail.len() * 4) as u64);
            self.queue.write_buffer(&buf, 0, bytemuck::cast_slice(&m.detail));
            (buf, ((m.detail.len() / 8) as u32).min(MAX_QUADS))
        });
        Some(ChunkMesh {
            vbuf,
            detail,
            opaque_quads: m.opaque_quads.min(MAX_QUADS),
            deep_quads: if m.opaque_quads <= MAX_QUADS { m.deep_quads } else { 0 },
            cut_quads: m.cut_quads.min(MAX_QUADS),
            trans_quads: m.trans_quads.min(MAX_QUADS),
            top: m.top,
        })
    }

    fn fill(&self, buf: &mut wgpu::Buffer, bytes: &[u8]) {
        if bytes.len() as u64 > buf.size() {
            *buf = vbuf(&self.device, (bytes.len() as u64).next_power_of_two());
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(buf, 0, bytes);
        }
    }

    pub fn render(&mut self, f: &Frame) {
        use wgpu::CurrentSurfaceTexture as T;
        let frame = match self.surface.get_current_texture() {
            T::Success(t) | T::Suboptimal(t) => t,
            T::Outdated | T::Lost => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            _ => return,
        };
        let view = frame.texture.create_view(&Default::default());

        let g = Globals {
            view_proj: f.view_proj.to_cols_array_2d(),
            fog_color: [f.fog_color[0], f.fog_color[1], f.fog_color[2], 1.0],
            fog: [f.fog_start, f.fog_end, f.ambient, 0.0],
            misc: [f.daylight, f.lights.len().min(MAX_LIGHTS) as f32, f.ent_light, f.eye_y],
            lights: std::array::from_fn(|i| f.lights.get(i).copied().unwrap_or([0.0, 0.0, 0.0, 1.0])),
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));

        let chunks = &f.chunks[..f.chunks.len().min(MAX_INSTANCES)];
        self.inst_scratch.clear();
        self.inst_scratch.extend(chunks.iter().map(|(_, o, _, _)| [o[0], o[1], o[2], 0.0]));
        if !chunks.is_empty() {
            self.queue.write_buffer(&self.inst_buf, 0, bytemuck::cast_slice(&self.inst_scratch));
        }
        let mut ent_buf = std::mem::replace(&mut self.ent_buf, vbuf(&self.device, 4));
        self.fill(&mut ent_buf, bytemuck::cast_slice(f.ents));
        self.ent_buf = ent_buf;
        let mut item_buf = std::mem::replace(&mut self.item_buf, vbuf(&self.device, 4));
        let items = &f.items[..f.items.len().min(MAX_QUADS as usize * 4)];
        self.fill(&mut item_buf, bytemuck::cast_slice(items));
        self.item_buf = item_buf;
        let mut sky_buf = std::mem::replace(&mut self.sky_buf, vbuf(&self.device, 4));
        self.fill(&mut sky_buf, bytemuck::cast_slice(f.sky));
        self.sky_buf = sky_buf;
        let mut ui_buf = std::mem::replace(&mut self.ui_buf, vbuf(&self.device, 4));
        self.fill(&mut ui_buf, bytemuck::cast_slice(f.ui));
        self.ui_buf = ui_buf;

        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.scene.as_ref().map_or(&view, |s| &s.view),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f.fog_color[0] as f64,
                            g: f.fog_color[1] as f64,
                            b: f.fog_color[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_bind_group(0, &self.bind, &[]);

            if !f.sky.is_empty() {
                rp.set_pipeline(&self.sky_pipe);
                rp.set_vertex_buffer(0, self.sky_buf.slice(..));
                rp.draw(0..f.sky.len() as u32, 0..1);
            }
            // opaque terrain, front to back
            rp.set_pipeline(&self.chunk_pipe);
            rp.set_index_buffer(self.quad_ibuf.slice(..), wgpu::IndexFormat::Uint32);
            rp.set_vertex_buffer(1, self.inst_buf.slice(..));
            for (i, (m, _, _, deep)) in chunks.iter().enumerate() {
                let quads = m.opaque_quads - if *deep { 0 } else { m.deep_quads };
                if quads > 0 {
                    rp.set_vertex_buffer(0, m.vbuf.slice(..));
                    rp.draw_indexed(0..quads * 6, 0, i as u32..i as u32 + 1);
                }
            }
            // stairs, slabs, fences, doors and the like
            let mut first = true;
            for (i, (m, _, _, _)) in chunks.iter().enumerate() {
                if let Some((buf, quads)) = &m.detail {
                    if std::mem::take(&mut first) {
                        rp.set_pipeline(&self.detail_pipe);
                    }
                    rp.set_vertex_buffer(0, buf.slice(..));
                    rp.draw_indexed(0..quads * 6, 0, i as u32..i as u32 + 1);
                }
            }
            // items lying on the ground
            let lying = (items.len() - f.hand.min(items.len())) / 4 * 6;
            if lying > 0 {
                rp.set_pipeline(&self.item_pipe);
                rp.set_vertex_buffer(0, self.item_buf.slice(..));
                rp.draw_indexed(0..lying as u32, 0, 0..1);
            }
            // plants
            rp.set_pipeline(&self.cut_pipe);
            rp.set_vertex_buffer(1, self.inst_buf.slice(..));
            for (i, (m, _, plants, _)) in chunks.iter().enumerate() {
                if m.cut_quads > 0 && *plants {
                    rp.set_vertex_buffer(0, m.vbuf.slice(..));
                    rp.draw_indexed(0..m.cut_quads * 6, (m.opaque_quads * 4) as i32, i as u32..i as u32 + 1);
                }
            }
            // animals, block outline
            if !f.ents.is_empty() {
                rp.set_pipeline(&self.ent_pipe);
                rp.set_vertex_buffer(0, self.ent_buf.slice(..));
                rp.draw(0..f.ents.len() as u32, 0..1);
            }
            // water / glass, back to front
            rp.set_pipeline(&self.trans_pipe);
            rp.set_vertex_buffer(1, self.inst_buf.slice(..));
            for (i, (m, _, _, _)) in chunks.iter().enumerate().rev() {
                if m.trans_quads > 0 {
                    rp.set_vertex_buffer(0, m.vbuf.slice(..));
                    rp.draw_indexed(
                        0..m.trans_quads * 6,
                        ((m.opaque_quads + m.cut_quads) * 4) as i32,
                        i as u32..i as u32 + 1,
                    );
                }
            }
            // the item in the player's hand, last and over everything
            if items.len() / 4 * 6 > lying {
                rp.set_pipeline(&self.hand_pipe);
                rp.set_vertex_buffer(0, self.item_buf.slice(..));
                rp.draw_indexed(lying as u32..(items.len() / 4 * 6) as u32, 0, 0..1);
            }
        }
        {
            // second pass at window resolution: the upscaled scene, then the HUD on top
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if self.scene.is_some() { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(scene) = &self.scene {
                rp.set_pipeline(&self.blit_pipe);
                rp.set_bind_group(0, &scene.bind, &[]);
                rp.draw(0..3, 0..1);
            }
            if !f.ui.is_empty() {
                rp.set_pipeline(&self.ui_pipe);
                rp.set_bind_group(0, &self.bind, &[]);
                rp.set_vertex_buffer(0, self.ui_buf.slice(..));
                rp.draw(0..f.ui.len() as u32, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
        self.queue.present(frame);
    }
}
