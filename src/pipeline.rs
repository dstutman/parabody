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
    PowerPreference, RequestAdapterOptions, ShaderModuleDescriptor, ShaderStages,
};

use crate::structures::{Body, Config};
pub struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config_bindgroup_layout: wgpu::BindGroupLayout,
    body_bindgroup_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    config_buffer: wgpu::Buffer,
    body_buffers: [wgpu::Buffer; 2],
    active_source: SourceBuffer,
    config: Config,
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
        shader: ShaderModuleDescriptor<'_>,
        entry_point: &'static str,
        config: Config,
    ) -> Self {
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
                    ty: BufferBindingType::Storage { read_only: true },
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
            size: size_of::<Config>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
            mapped_at_creation: false,
        });
        let body_buffers = [
            device.create_buffer(&BufferDescriptor {
                label: Some("Buffer A"),
                size: (config.num_bodies as usize * size_of::<Body>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
                mapped_at_creation: false,
            }),
            device.create_buffer(&BufferDescriptor {
                label: Some("Buffer B"),
                size: (config.num_bodies as usize * size_of::<Body>()) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
                mapped_at_creation: false,
            }),
        ];

        let pipeline = Self {
            device,
            queue,
            config_bindgroup_layout,
            body_bindgroup_layout,
            pipeline,
            config_buffer,
            body_buffers,
            config,
            active_source: SourceBuffer::A,
        };
        pipeline.write_config();

        pipeline
    }

    fn write_config(&self) {
        // Map the config buffer and write the data from the host to the GPU
        let mut config_slice = {
            let slice = self.config_buffer.slice(..);
            self.map_slice_blocking(slice);
            slice.get_mapped_range_mut()
        };

        let config_bytes: [u8; size_of::<Config>()] = bytemuck::cast(self.config);
        config_slice.as_mut().copy_from_slice(&config_bytes);
        drop(config_slice);
        self.config_buffer.unmap();
    }

    pub fn write_bodies(&self, input: &[Body]) {
        assert!(input.len() == self.config.num_bodies as usize);
        // Map the input buffer and write the data from the host to the GPU
        let mut input_slice = {
            let slice = match self.active_source {
                SourceBuffer::A => self.body_buffers[0].slice(..),
                SourceBuffer::B => self.body_buffers[1].slice(..),
            };
            self.map_slice_blocking(slice);
            slice.get_mapped_range_mut()
        };
        input_slice
            .as_mut()
            .copy_from_slice(bytemuck::cast_slice(input));
        drop(input_slice);
        match self.active_source {
            SourceBuffer::A => self.body_buffers[0].unmap(),
            SourceBuffer::B => self.body_buffers[1].unmap(),
        };
    }

    pub fn read_bodies(&self) -> Vec<Body> {
        // Wait for the output buffer to become mappable and read it out to the host
        let output_slice = match self.active_source {
            SourceBuffer::A => self.body_buffers[1].slice(..),
            SourceBuffer::B => self.body_buffers[0].slice(..),
        };
        self.map_slice_blocking(output_slice);
        let output = bytemuck::cast_slice(output_slice.get_mapped_range().as_ref()).to_owned();
        drop(output_slice);
        match self.active_source {
            SourceBuffer::A => self.body_buffers[1].unmap(),
            SourceBuffer::B => self.body_buffers[0].unmap(),
        };
        return output;
    }

    pub fn submit_and_block(&mut self, num_passes: usize) {
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
            pass.dispatch_workgroups(self.config.num_bodies / 64, 1, 1);
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

    pub fn map_slice_blocking(&self, slice: BufferSlice) {
        let signal = Arc::new(AtomicBool::new(false));
        let moved_signal = signal.clone();
        slice.map_async(MapMode::Read, move |result| {
            result.expect("Failed to map output buffer for reading");
            moved_signal.store(true, Ordering::SeqCst);
        });

        // TODO: Relax the ordering
        while !signal.load(Ordering::SeqCst) {
            self.device.poll(Maintain::Poll);
        }
    }
}
