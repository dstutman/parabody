use env_logger;
use pollster;
use wgpu::include_wgsl;

use crate::{
    pipeline::{Pipeline, SourceBuffer},
    structures::Body,
};
mod pipeline;
mod structures;

async fn async_entry() {
    env_logger::init();
    println!("Starting parabody.");

    let shader = include_wgsl!("../shaders/test.wgsl");
    let pipeline = Pipeline::create(shader, "main").await;

    let input: [Body; 100] = [Default::default(); 100];
    let output = pipeline.submit_and_wait(&input, SourceBuffer::A);
    println!("{:?}", output[0]);
}

fn main() {
    pollster::block_on(async_entry());
}
