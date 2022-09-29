use core::sync::atomic::Ordering;
use std::sync::{atomic::AtomicBool, Arc};

use wgpu::{
    self,
    util::{BufferInitDescriptor, DeviceExt},
    Backends, BindGroupDescriptor, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
    BindingType, BufferBindingType, BufferSlice, BufferUsages, CommandEncoderDescriptor,
    ComputePassDescriptor, ComputePipelineDescriptor, DeviceDescriptor, Features, Instance, Limits,
    MaintainBase, MapMode, PipelineLayoutDescriptor, PowerPreference, RequestAdapterOptions,
    ShaderModuleDescriptor, ShaderStages,
};

use crate::structures::Body;
pub struct Pipeline {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bindgroup_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    buffers: [wgpu::Buffer; 2],
}

#[derive(Debug, Clone, Copy)]
pub enum SourceBuffer {
    A,
    B,
}

impl Pipeline {
    pub async fn create(shader: ShaderModuleDescriptor<'_>, entry_point: &'static str) -> Self {
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
        let bindgroup_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
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
            bind_group_layouts: &[&bindgroup_layout],
            ..Default::default()
        });
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("Compute pipeline"),
            module: &shader,
            entry_point: entry_point,
            layout: Some(&pipeline_layout),
        });
        let buffers = [
            device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Buffer A"),
                contents: bytemuck::cast_slice(
                    &[Body {
                        ..Default::default()
                    }; 100],
                ),
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
            }),
            device.create_buffer_init(&BufferInitDescriptor {
                label: Some("Buffer B"),
                contents: bytemuck::cast_slice(
                    &[Body {
                        ..Default::default()
                    }; 100],
                ),
                usage: BufferUsages::STORAGE | BufferUsages::MAP_READ | BufferUsages::MAP_WRITE,
            }),
        ];

        return Self {
            device,
            queue,
            bindgroup_layout,
            pipeline,
            buffers,
        };
    }

    pub fn submit_and_wait(&self, input: &[Body; 100], src: SourceBuffer) -> Vec<Body> {
        // Map the input buffer and write the data from the host to the GPU
        let mut input_slice = {
            let slice = match src {
                SourceBuffer::A => self.buffers[0].slice(..),
                SourceBuffer::B => self.buffers[1].slice(..),
            };
            self.map_slice_blocking(slice);
            slice.get_mapped_range_mut()
        };
        input_slice
            .as_mut()
            .copy_from_slice(bytemuck::cast_slice(input));
        drop(input_slice);
        match src {
            SourceBuffer::A => self.buffers[0].unmap(),
            SourceBuffer::B => self.buffers[1].unmap(),
        };
        // Fire off the job
        let bindgroup = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Compute bind group"),
            layout: &self.bindgroup_layout,
            entries: &match src {
                SourceBuffer::A => [
                    BindGroupEntry {
                        binding: 0,
                        resource: self.buffers[0].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: self.buffers[1].as_entire_binding(),
                    },
                ],
                SourceBuffer::B => [
                    BindGroupEntry {
                        binding: 1,
                        resource: self.buffers[0].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 0,
                        resource: self.buffers[1].as_entire_binding(),
                    },
                ],
            },
        });

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor { label: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindgroup, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }

        self.queue.submit(Some(encoder.finish()));

        // Wait for the output buffer to become mappable and read it out to the host
        let output_slice = match src {
            SourceBuffer::A => self.buffers[1].slice(..),
            SourceBuffer::B => self.buffers[0].slice(..),
        };
        self.map_slice_blocking(output_slice);
        return bytemuck::cast_slice(output_slice.get_mapped_range().as_ref()).to_owned();
    }

    pub fn map_slice_blocking(&self, slice: BufferSlice) {
        let signal = Arc::new(AtomicBool::new(false));
        let moved_signal = signal.clone();
        slice.map_async(MapMode::Read, move |result| {
            result.expect("Failed to map output buffer for reading");
            moved_signal.store(true, Ordering::SeqCst);
        });

        while !signal.load(Ordering::SeqCst) {
            self.device.poll(MaintainBase::Wait);
        }
    }
}
