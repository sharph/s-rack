use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use eframe;
use egui;
use std::sync::Arc;
use std::sync::RwLock;

mod synth;
mod ui;

fn main() -> eframe::Result {
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
    let lfo = Arc::new(RwLock::new(synth::OscillatorModule::new(
        buffer_size,
        sample_rate,
    )));
    lfo.write().unwrap().val = -9.0;
    let osc = Arc::new(RwLock::new(synth::OscillatorModule::new(
        buffer_size,
        sample_rate,
    )));
    let output = Arc::new(RwLock::new(synth::OutputModule::new(
        buffer_size,
        sample_rate,
        channels,
    )));
    println!(
        "Sample rate: {}, Buffer size: {}, channels: {}",
        sample_rate, buffer_size, channels
    );
    let mut workspace = ui::SynthModuleWorkspace::new();
    let mut src_buf_pos: usize = 0;
    let output_ref = workspace.output.clone();
    let plan_ref = workspace.plan.clone();
    let stream = device
        .build_output_stream(
            &cpal::StreamConfig {
                channels: channels.into(),
                sample_rate: cpal::SampleRate(sample_rate),
                buffer_size: cpal::BufferSize::Fixed(buffer_size.try_into().unwrap()),
            },
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let plan = plan_ref.lock().unwrap();
                if let Some(output) = output_ref.lock().unwrap().as_ref() {
                    let mut src_buf = None;
                    if let Some(output) = output
                        .read()
                        .unwrap()
                        .as_any()
                        .downcast_ref::<synth::OutputModule>()
                    {
                        src_buf = Some(output.bufs.clone());
                    }
                    for dst_buf_pos in 0..data.len() {
                        let channel = dst_buf_pos % <usize>::from(channels);
                        match &src_buf {
                            Some(buf) => data[dst_buf_pos] = buf[channel][src_buf_pos],
                            None => data[dst_buf_pos] = 0.0,
                        }
                        if dst_buf_pos % <usize>::from(channels) == <usize>::from(channels) - 1 {
                            src_buf_pos += 1;
                            if src_buf_pos >= buffer_size {
                                synth::execute(&plan);
                                src_buf_pos = 0;
                            }
                        }
                    }
                }
            },
            move |_err| {},
            None,
        )
        .unwrap();
    stream.play().unwrap();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };
    workspace.add_module(lfo.clone());
    workspace.add_module(osc.clone());
    workspace.add_module(output.clone());
    workspace.plan();
    eframe::run_simple_native("s-rack", options, move |ctx, _frame| {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::scroll_area::ScrollArea::both()
                .scroll([true, true])
                .show(ui, |ui| {
                    workspace.ui(ui);
                });
        });
    })
}
