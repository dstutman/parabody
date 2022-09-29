struct Body {
    position: vec3<f32>, // Size: 12, Align: 16, Upto: 12
    mass: f32, // Size: 4, Align: 4, Upto: 16
    velocity: vec3<f32>, // Size: 12, Align: 16, Upto: 32
    mu: f32, // Size: 4, Align: 4, Upto: 36
}

@group(0) @binding(0) var<storage> input : array<Body, 100>;
@group(0) @binding(1) var<storage, read_write> output : array<Body, 100>;

@compute @workgroup_size(100)
fn main(@builtin(local_invocation_index) lidx: u32, @builtin(global_invocation_id) gid: vec3<u32>) {
    output[0].position[0] = input[0].position[0] + 1.0;
    output[0].position[1] = input[0].position[1] + 2.0;
}