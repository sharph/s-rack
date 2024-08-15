use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::sleep;
use std::time;

mod synth;

fn main() {
    println!("Hello, world!");
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .expect("No output device available!");
    let mut supported_configs_range = device
        .supported_output_configs()
        .expect("error while querying configs");
    let supported_config = supported_configs_range
        .next()
        .expect("no supported config")
        .with_sample_rate(cpal::SampleRate(48000));
    let buffer_size = match supported_config.buffer_size() {
        SupportedBufferSize::Range { min: _, max: _ } => 2048,
        SupportedBufferSize::Unknown => 4096,
    };
    let sample_rate = supported_config.sample_rate().0;
    let mut channels = <u8>::try_from(supported_config.channels()).unwrap();
    channels = 2;
    let lfo = Arc::new(RwLock::new(synth::DCOModule::new(buffer_size, sample_rate)));
    lfo.write().unwrap().val = -9.0;
    let osc = Arc::new(RwLock::new(synth::DCOModule::new(buffer_size, sample_rate)));
    let output = Arc::new(RwLock::new(synth::OutputModule::new(
        buffer_size,
        sample_rate,
        channels,
    )));
    println!(
        "Sample rate: {}, Buffer size: {}, channels: {}",
        sample_rate, buffer_size, channels
    );
    synth::connect(lfo.clone(), 0, osc.clone(), 0).unwrap();
    synth::connect(osc.clone(), 0, output.clone(), 0).unwrap();
    synth::connect(osc.clone(), 0, output.clone(), 1).unwrap();
    let mut src_buf_pos: usize = 0;
    let plan = synth::plan_execution(output.clone());
    let stream = device
        .build_output_stream(
            &cpal::StreamConfig {
                channels: channels.into(),
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Fixed(buffer_size.try_into().unwrap()),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let out = output.clone();
                let src_buf = &out.read().unwrap().bufs.clone();
                for dst_buf_pos in 0..data.len() {
                    let channel = dst_buf_pos % <usize>::from(channels);
                    data[dst_buf_pos] = src_buf[channel][src_buf_pos];
                    if dst_buf_pos % <usize>::from(channels) == <usize>::from(channels) - 1 {
                        src_buf_pos += 1;
                        if src_buf_pos >= buffer_size {
                            synth::execute(&plan);
                            src_buf_pos = 0;
                        }
                    }
                }
            },
            move |_err| {},
            None,
        )
        .unwrap();
    stream.play().unwrap();
    sleep(time::Duration::from_secs(10));
}
