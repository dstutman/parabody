use bytemuck::{Pod, Zeroable};

// TODO: Check alignment
#[repr(C, align(4))]
#[derive(Debug, Default, Clone, Copy, Zeroable, Pod)]
pub struct Config {
    pub num_bodies: u32,
    pub dt: f32,
}

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy, Zeroable, Pod)]
pub struct Body {
    pub position: [f32; 3],
    pub mass: f32,
    pub velocity: [f32; 3],
    pub mu: f32,
}
