use bytemuck::{Pod, Zeroable};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StaticConfig {
    pub max_bodies: u32,
}

// TODO: Check alignment
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct DynamicConfig {
    pub num_bodies: u32,
    pub dt: f32,
    _pad: [u32; 2],
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            num_bodies: 0,
            dt: 3600.0,
            _pad: [0; 2],
        }
    }
}

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy, Zeroable, Pod)]
pub struct Body {
    pub position: [f32; 3],
    pub mass: f32,
    pub velocity: [f32; 3],
    pub mu: f32,
}
