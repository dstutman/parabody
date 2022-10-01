use core::sync::atomic::Ordering;
use std::{
    mem::size_of,
    sync::{atomic::AtomicBool, Arc},
};

use wgpu::{
    self, Backends, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingType, BufferBindingType, BufferDescriptor, BufferSlice,
    BufferUsages, CommandEncoderDescriptor, ComputePassDescriptor, ComputePipelineDescriptor,
    DeviceDescriptor, Features, Instance, Limits, Maintain, MapMode, PipelineLayoutDescriptor,
    PowerPreference, RequestAdapterOptions, ShaderStages,
};

use crate::structures::{Body, DynamicConfig, StaticConfig};
pub struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config_bindgroup_layout: wgpu::BindGroupLayout,
    body_bindgroup_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    config_buffer: wgpu::Buffer,
    body_buffers: [wgpu::Buffer; 2],
    active_source: SourceBuffer,
    static_config: StaticConfig,
    dynamic_config: DynamicConfig,
}

#[derive(Debug, Clone, Copy)]
pub enum SourceBuffer {
    A,
    B,
}

impl SourceBuffer {
    pub fn other(self) -> Self {
        match self {
            SourceBuffer::A => SourceBuffer::B,
            SourceBuffer::B => SourceBuffer::A,
        }
    }
}

impl Pipeline {
    pub async fn create(
        shader_src: &'static str,
        entry_point: &'static str,
        static_config: StaticConfig,
    ) -> Self {
        // Render the shader with its static configuration
        let mut tera = tera::Tera::default();
        tera.add_raw_template("shader", shader_src).unwrap();
        let mut context = tera::Context::new();
        context.insert("static_config", &static_config);
        let shader = wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                tera.render("shader", &context)
                    .expect("Failed to render shader from template")
                    .into(),
            ),
        };

        // Create default config
        let dynamic_config = DynamicConfig::default();

        // Construct the pipeline
        let instance = Instance::new(Backends::all());
        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .expect("Could not get adapter");

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("Compute device"),
                    features: Features::empty(),
                    limits: Limits::downlevel_defaults(),
                },
                None,
            )
            .await
            .expect("Could not acquire WebGPU device");
        let shader = device.create_shader_module(shader);
        let config_bindgroup_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let body_bindgroup_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Compute pipeline layout"),
            bind_group_layouts: &[&config_bindgroup_layout, &body_bindgroup_layout],
            ..Default::default()
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("Compute pipeline"),
            module: &shader,
            entry_point: entry_point,
            layout: Some(&pipeline_layout),
        });
        let config_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Config"),
            size: size_of::<DynamicConfig>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
        let body_buffers = [
            device.create_buffer(&BufferDescriptor {
                label: Some("Buffer A"),
                size: (static_config.max_bodies as usize * size_of::<Body>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
                mapped_at_creation: false,
            }),
            device.create_buffer(&BufferDescriptor {
                label: Some("Buffer B"),
                size: (static_config.max_bodies as usize * size_of::<Body>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
                mapped_at_creation: false,
            }),
        ];

        let mut pipeline = Self {
            device,
            queue,
            config_bindgroup_layout,
            body_bindgroup_layout,
            pipeline,
            config_buffer,
            body_buffers,
            static_config,
            dynamic_config,
            active_source: SourceBuffer::A,
        };
        pipeline.synchronize_dynamic_config();

        pipeline
    }

    pub fn set_dt(&mut self, dt: f32) {
        self.dynamic_config.dt = dt;
    }

    fn synchronize_dynamic_config(&mut self) {
        // Map the config buffer and write the data from the host to the GPU
        let slice = self.config_buffer.slice(..);
        self.map_slice_blocking(MapMode::Write, slice);
        {
            let mut config = slice.get_mapped_range_mut();
            let config_bytes: [u8; size_of::<DynamicConfig>()] =
                bytemuck::cast(self.dynamic_config);
            config.as_mut().copy_from_slice(&config_bytes);
        }
        self.config_buffer.unmap();
    }

    pub fn write_bodies(&mut self, input: &[Body]) {
        assert!(input.len() <= self.static_config.max_bodies as usize);
        self.dynamic_config.num_bodies = input.len() as u32;
        // Map the input buffer and write the data from the host to the GPU
        let upper_bound = (self.dynamic_config.num_bodies * size_of::<Body>() as u32) as u64;
        let slice = match self.active_source {
            SourceBuffer::A => self.body_buffers[0].slice(..upper_bound),
            SourceBuffer::B => self.body_buffers[1].slice(..upper_bound),
        };
        self.map_slice_blocking(MapMode::Write, slice);
        {
            let mut source = slice.get_mapped_range_mut();
            source.as_mut().copy_from_slice(bytemuck::cast_slice(input));
        }
        match self.active_source {
            SourceBuffer::A => self.body_buffers[0].unmap(),
            SourceBuffer::B => self.body_buffers[1].unmap(),
        };
    }

    pub fn read_bodies(&self) -> Vec<Body> {
        // Wait for the output buffer to become mappable and read it out to the host
        let upper_bound = (self.dynamic_config.num_bodies * size_of::<Body>() as u32) as u64;
        let slice = match self.active_source {
            SourceBuffer::A => self.body_buffers[1].slice(..upper_bound),
            SourceBuffer::B => self.body_buffers[0].slice(..upper_bound),
        };
        self.map_slice_blocking(MapMode::Read, slice);
        let output = bytemuck::cast_slice(slice.get_mapped_range().as_ref()).to_owned();
        match self.active_source {
            SourceBuffer::A => self.body_buffers[1].unmap(),
            SourceBuffer::B => self.body_buffers[0].unmap(),
        };
        return output;
    }

    pub fn submit_and_block(&mut self, num_passes: usize) {
        // Synchronize configurations
        self.synchronize_dynamic_config();
        // Fire off the job
        let config_bindgroup = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Config bind group"),
            layout: &self.config_bindgroup_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: self.config_buffer.as_entire_binding(),
            }],
        });
        let active_a_bindgroup = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Active-A bind group"),
            layout: &self.body_bindgroup_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.body_buffers[0].as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: self.body_buffers[1].as_entire_binding(),
                },
            ],
        });
        let active_b_bindgroup = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Active-B bind group"),
            layout: &self.body_bindgroup_layout,
            entries: &[
                BindGroupEntry {
                    binding: 1,
                    resource: self.body_buffers[0].as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 0,
                    resource: self.body_buffers[1].as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });

        for _ in 0..num_passes {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { label: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &config_bindgroup, &[]);
            match self.active_source {
                SourceBuffer::A => pass.set_bind_group(1, &active_a_bindgroup, &[]),
                SourceBuffer::B => pass.set_bind_group(1, &active_b_bindgroup, &[]),
            };
            pass.dispatch_workgroups(
                (self.dynamic_config.num_bodies as f32 / 64.0).ceil() as u32,
                1,
                1,
            );
            self.active_source = self.active_source.other();
        }

        println!("Submitting");
        self.queue.submit(Some(encoder.finish()));

        let signal = Arc::new(AtomicBool::new(false));
        let moved_signal = signal.clone();
        self.queue.on_submitted_work_done(move || {
            moved_signal.store(true, Ordering::SeqCst);
        });

        // TODO: Relax the ordering
        while !signal.load(Ordering::SeqCst) {
            self.device.poll(Maintain::Poll);
        }
        println!("Done");
    }

    pub fn map_slice_blocking(&self, mode: MapMode, slice: BufferSlice) {
        let signal = Arc::new(AtomicBool::new(false));
        let moved_signal = signal.clone();
        slice.map_async(mode, move |result| {
            result.expect("Failed to map output buffer for reading");
            moved_signal.store(true, Ordering::SeqCst);
        });

        // TODO: Relax the ordering
        while !signal.load(Ordering::SeqCst) {
            self.device.poll(Maintain::Poll);
        }
    }
}
