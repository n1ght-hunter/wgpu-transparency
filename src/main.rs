use std::sync::Arc;

use glam::Vec4;
use wgpu::{include_wgsl, util::DeviceExt};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

const SHADER: wgpu::ShaderModuleDescriptor = include_wgsl!("main.wgsl");

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VertexInput {
    pub position: Vec4,
    pub color: Vec4,
}

impl VertexInput {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
    ];

    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

struct State {
    window: Arc<dyn Window>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,

    pub surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,

    pub pipeline: wgpu::RenderPipeline,
    pub queue: wgpu::Queue,
    pub bind_group: wgpu::BindGroup,

    // buffers
    pub data_buffer: wgpu::Buffer,

    // depth texture
    pub depth_texture: Option<wgpu::Texture>,
    pub depth_texture_format: wgpu::TextureFormat,

    // config
    size: winit::dpi::PhysicalSize<u32>,
}

impl State {
    async fn new(window: Arc<dyn Window>) -> State {
        let size = window.surface_size();

        let mut backend_options = wgpu::BackendOptions::default();
        backend_options.dx12.presentation_system = wgpu::wgt::Dx12SwapchainKind::DxgiFromVisual;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            backend_options: backend_options,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        // handle to graphics card
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();

        // get device and queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Device"),
                required_features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
                experimental_features: wgpu::ExperimentalFeatures::default(),
            })
            .await
            .unwrap();

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Bind Group Layout"),
            entries: &[],
        });

        // create bind group for view parameters
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Bind Group"),
            layout: &bind_group_layout,
            entries: &[],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        // Load the shaders from disk
        let shader = device.create_shader_module(SHADER);

        let depth_texture_format = wgpu::TextureFormat::Depth24PlusStencil8;

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex_main"),
                buffers: &[VertexInput::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment_main"),
                compilation_options: Default::default(),
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_texture_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            // depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let data_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&[
                VertexInput {
                    position: Vec4::new(1.0, -1.0, 0.0, 1.0),
                    color: Vec4::new(1.0, 0.0, 0.0, 1.0),
                },
                VertexInput {
                    position: Vec4::new(-1.0, -1.0, 0.0, 1.0),
                    color: Vec4::new(0.0, 1.0, 0.0, 1.0),
                },
                VertexInput {
                    position: Vec4::new(0.0, 1.0, 0.0, 1.0),
                    color: Vec4::new(0.0, 0.0, 1.0, 1.0),
                },
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let depth_texture_format = wgpu::TextureFormat::Depth24PlusStencil8;

        let mut state = Self {
            window,
            adapter,
            device,
            queue,
            surface,
            surface_format: swapchain_format,
            pipeline: render_pipeline,
            bind_group: bind_group,
            data_buffer: data_buf,
            depth_texture: None,
            depth_texture_format,
            size,
        };

        state.configure_surface();

        state
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;

            // reconfigure the surface
            self.configure_surface();
        }
    }

    fn configure_surface(&mut self) {
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.surface_format,
                // Request compatibility with the sRGB-format texture view we‘re going to create later.
                view_formats: vec![self.surface_format.add_srgb_suffix()],
                alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                width: self.size.width,
                height: self.size.height,
                desired_maximum_frame_latency: 2,
                present_mode: wgpu::PresentMode::AutoVsync,
            },
        );

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Texture"),
            size: wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_texture = Some(texture);
    }

    pub fn render(&self) {
        // Create texture view
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                // Without add_srgb_suffix() the image we will be working with
                // might not be "gamma correct".
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        let depth_texture = self
            .depth_texture
            .as_ref()
            .unwrap()
            .create_view(&Default::default());

        // Renders a GREEN screen
        let mut encoder = self.device.create_command_encoder(&Default::default());

        let render_pass_descriptor = wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &texture_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_texture,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            }),
            // timestamp_writes: None,
            // occlusion_query_set: None,
            ..Default::default()
        };

        // encoder.copy_buffer_to_buffer(source, source_offset, destination, destination_offset, copy_size);
        {
            // Create the renderpass which will clear the screen.
            let mut renderpass = encoder.begin_render_pass(&render_pass_descriptor);

            renderpass.set_pipeline(&self.pipeline);
            renderpass.set_bind_group(0, &self.bind_group, &[]);
            renderpass.set_vertex_buffer(0, self.data_buffer.slice(..));
            renderpass.draw(0..3, 0..1);
        }

        // Submit the command in the queue to execute
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
    }
}

struct App {
    state: Option<State>,
    last_render_time: std::time::Instant,
}

impl Default for App {
    fn default() -> Self {
        Self {
            state: None,
            last_render_time: std::time::Instant::now(),
        }
    }
}

enum Event {}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Create window object
        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_transparent(true)
                        .with_title("WGPU Window"),
                )
                .unwrap(),
        );
        let state = pollster::block_on(State::new(window.clone()));
        self.state = Some(state);

        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let dt = now - self.last_render_time;
                self.last_render_time = now;
                state.render();
                // Emits a new redraw requested event.
                state.window.request_redraw();
            }
            WindowEvent::SurfaceResized(size) => {
                println!("Window resized to {:?}", size);
                // Reconfigures the size of the surface. We do not re-render
                // here as this event is always followed up by redraw request.
                state.resize(size);
            }
            _ => (),
        }
    }
}

fn main() {
    let event_loop = winit::event_loop::EventLoop::new().unwrap();

    // When the current loop iteration finishes, suspend the thread until
    // another event arrives. Helps keeping CPU utilization low if nothing
    // is happening, which is preferred if the application might be idling in
    // the background.
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
