use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use eframe;
use egui;
use egui::output;
use std::sync::Arc;
use std::sync::RwLock;
use synth::SharedSynthModule;

mod synth;

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
    let output_ui = output.clone();
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };
    eframe::run_simple_native("s-rack", options, move |ctx, _frame| {
        let lfo_ui = lfo.clone();
        let osc_ui = osc.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::scroll_area::ScrollArea::both()
                .scroll([true, true])
                .show(ui, |ui| {
                    synth_module_container(ui, lfo_ui.clone());
                    synth_module_container(ui, osc_ui.clone());
                    synth_module_container(ui, output_ui.clone());
                });
        });
    })
}
fn synth_module_container(ui: &egui::Ui, synth_module: synth::SharedSynthModule) {
    let mut synth_module = synth_module.write().unwrap();
    let mut id = "synth_module:".to_string();
    id.push_str(&synth_module.get_id());
    let ctx = ui.ctx();
    let area = egui::Area::new(egui::Id::new(id))
        .default_pos(egui::pos2(100.0, 100.0))
        .show(ctx, |ui| {
            egui::Frame::none()
                .stroke(egui::Stroke {
                    width: 2.0,
                    color: egui::Color32::RED,
                })
                .inner_margin(10.0)
                .show(ui, |ui| {
                    synth_module.ui(ui);
                });
        });
}
