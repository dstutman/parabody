use env_logger;
use pollster;
use wgpu::include_wgsl;

use crate::{
    pipeline::Pipeline,
    structures::{Body, Config},
};
mod pipeline;
mod structures;

async fn async_entry() {
    env_logger::init();
    println!("Starting parabody.");

    const NUM_BODIES: usize = 2_usize.pow(13);
    let t = 100;
    let dt = 0.001 as f32;
    let steps = (t as f32 / dt).ceil() as usize;

    let shader = include_wgsl!("../shaders/dynamics.wgsl");
    let mut pipeline = Pipeline::create(
        shader,
        "main",
        Config {
            num_bodies: NUM_BODIES as u32,
            dt,
        },
    )
    .await;
    let mut input: [Body; NUM_BODIES] = [Default::default(); NUM_BODIES];
    input[0].mu = 1.0;
    input[0].position = [10.0, 10.0, 10.0];
    input[1].mu = 2.0;
    pipeline.write_bodies(&input);
    pipeline.submit_and_block(steps);
    let output = pipeline.read_bodies();
    println!("{:?}", output.first());
    println!("{:?}", output.last());
}

fn main() {
    pollster::block_on(async_entry());
}
