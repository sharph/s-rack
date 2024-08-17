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
    let mut workspace = SynthModuleWorkspace::new();
    workspace.modules.push(lfo.clone());
    workspace.modules.push(osc.clone());
    workspace.modules.push(output_ui.clone());
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

struct SynthModuleWorkspace {
    transform: egui::emath::TSTransform,
    modules: Vec<synth::SharedSynthModule>,
}

impl SynthModuleWorkspace {
    fn new() -> Self {
        Self {
            transform: egui::emath::TSTransform::new([0.0, 0.0].into(), 1.0),
            modules: vec![],
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let (id, rect) = ui.allocate_space(ui.available_size());
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        // Allow dragging the background as well.
        if response.dragged() {
            self.transform.translation += response.drag_delta();
        }

        // Plot-like reset
        if response.double_clicked() {
            self.transform = egui::emath::TSTransform::new([0.0, 0.0].into(), 1.0);
        }

        let transform =
            egui::emath::TSTransform::from_translation(ui.min_rect().left_top().to_vec2())
                * self.transform;

        if let Some(pointer) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            if response.hovered() {
                let pointer_in_layer = transform.inverse() * pointer;
                let zoom_delta = ui.ctx().input(|i| i.zoom_delta());
                let pan_delta = ui.ctx().input(|i| i.smooth_scroll_delta);

                self.transform = self.transform
                    * egui::emath::TSTransform::from_translation(pointer_in_layer.to_vec2())
                    * egui::emath::TSTransform::from_scaling(zoom_delta)
                    * egui::emath::TSTransform::from_translation(-pointer_in_layer.to_vec2());

                self.transform =
                    egui::emath::TSTransform::from_translation(pan_delta) * self.transform;
            }
        }

        for module in self.modules.iter() {
            let mut module = module.write().unwrap();
            let window_layer = ui.layer_id();
            let id = egui::Area::new(id.with(("module", module.get_id())))
                .constrain(false)
                .default_pos(egui::pos2(100.0, 100.0))
                .order(egui::Order::Middle)
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    egui::Frame::default()
                        .rounding(egui::Rounding::same(4.0))
                        .inner_margin(egui::Margin::same(8.0))
                        .stroke(ui.ctx().style().visuals.window_stroke)
                        .fill(ui.style().visuals.panel_fill)
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            ui.vertical(|ui| {
                                ui.add(
                                    egui::widgets::Label::new(module.get_name()).selectable(false),
                                );
                                module.ui(ui);
                            });
                        });
                })
                .response
                .layer_id;
            ui.ctx().set_transform_layer(id, transform);
            ui.ctx().set_sublayer(window_layer, id);
        }
    }
}
