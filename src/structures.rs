use bytemuck::{Pod, Zeroable};

#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy, Zeroable, Pod)]
pub struct Body {
    pub position: [f32; 3],
    pub mass: f32,
    pub velocity: [f32; 3],
    pub mu: f32,
}
