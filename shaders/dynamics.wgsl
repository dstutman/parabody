struct Config {
    num_bodies: u32,
    dt: f32,
}

struct Body {
    position: vec3<f32>, // Size: 12, Align: 16, Upto: 12
    mass: f32, // Currently unused, is free because of alignment
    velocity: vec3<f32>, // Size: 12, Align: 16, Upto: 32
    mu: f32, // Size: 4, Align: 4, Upto: 36
}

@group(0) @binding(0) var<storage> config: Config;
@group(1) @binding(0) var<storage, read> input : array<Body, 65536>;
@group(1) @binding(1) var<storage, read_write> output : array<Body, 65536>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid[0];
    if !(idx < config.num_bodies) { return; }
    // Create mutable copy of previous state
    output[idx] = input[idx];
    // Propagate dynamics
    output[idx].position += input[idx].velocity * config.dt;
    var acceleration: vec3<f32> = vec3<f32>(0.0, 0.0, 0.0);
    for(var other_idx: u32 = u32(0); other_idx < config.num_bodies; other_idx++) {
        // TODO: Ensure there isn't a faster way to do this
        if (idx == other_idx) { return; }
        let separation = input[other_idx].position - input[idx].position;
        let distance = length(separation);
        if (distance < 1.0) { return; }
        acceleration += input[other_idx].mu / pow(distance, 3.0) * separation;
    }
    output[idx].velocity += acceleration * config.dt;
    output[idx].mass += 1.0;
}