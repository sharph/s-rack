use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use eframe;
use egui;
use std::borrow::BorrowMut;
use std::fmt::write;
use std::sync::Arc;
use std::sync::RwLock;

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
                {
                    let src_buf = &out.write().unwrap().bufs.clone();
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

const SYNTH_HANDLE_SIZE: f32 = 10.0;
const SYNTH_HANDLE_PADDING: f32 = 2.0;

enum SynthModulePort {
    Input(synth::SharedSynthModule, u8),
    Output(synth::SharedSynthModule, u8),
}

struct SynthModuleHandle {}

impl SynthModuleHandle {
    fn new() -> Self {
        Self {}
    }

    fn layout_in_ui(&mut self, ui: &mut egui::Ui) -> (egui::Id, egui::Rect, egui::Response) {
        let (id, rect) = ui.allocate_space([SYNTH_HANDLE_SIZE, SYNTH_HANDLE_SIZE].into());
        (
            id,
            rect,
            ui.interact(rect, id, egui::Sense::click_and_drag()),
        )
    }
}

impl egui::Widget for &mut SynthModuleHandle {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (id, rect, response) = self.layout_in_ui(ui);
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, egui::Color32::RED);
        response
    }
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

        for module_ref in self.modules.iter() {
            let mut module = module_ref.write().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module", module.get_id()));
            let area = egui::Area::new(area_id)
                .constrain(false)
                .default_pos(egui::pos2(100.0, 100.0))
                .order(egui::Order::Middle)
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            for idx in 0..module.get_num_inputs() {
                                let response = ui.add(&mut SynthModuleHandle::new());
                                response.dnd_set_drag_payload(SynthModulePort::Input(
                                    module_ref.clone(),
                                    idx,
                                ));
                                if response.secondary_clicked() {
                                    module.disconnect_input(idx).unwrap();
                                }
                                if let Some(payload) =
                                    response.dnd_release_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Output(output_module, output_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        module
                                            .set_input(idx, output_module.clone(), *output_port)
                                            .unwrap();
                                    }
                                }
                            }
                        });
                        egui::Frame::default()
                            .rounding(egui::Rounding::same(2.0))
                            .inner_margin(egui::Margin::same(12.0))
                            .stroke(ui.ctx().style().visuals.window_stroke)
                            .fill(ui.style().visuals.panel_fill)
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::widgets::Label::new(module.get_name())
                                            .selectable(false),
                                    );
                                    module.ui(ui);
                                });
                            });
                        ui.vertical(|ui| {
                            for idx in 0..module.get_num_outputs() {
                                let response = ui.add(&mut SynthModuleHandle::new());
                                response.dnd_set_drag_payload(SynthModulePort::Output(
                                    module_ref.clone(),
                                    idx,
                                ));
                                if let Some(payload) =
                                    response.dnd_release_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Input(input_module, input_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        let mut sink_module = input_module.write().unwrap();
                                        sink_module
                                            .set_input(*input_port, module_ref.clone(), idx)
                                            .unwrap();
                                    }
                                }
                            }
                        });
                    });
                });
            // load pivot from memory

            let layer_id = area.response.layer_id;
            ui.ctx().set_transform_layer(layer_id, transform);
            ui.ctx().set_sublayer(window_layer, layer_id);
        }

        for module in self.modules.iter() {
            let module = module.read().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module-connection", module.get_id()));
            let module_area_id = id.with(("module", module.get_id()));
            let area = egui::Area::new(area_id)
                .fixed_pos((0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    if let Some(state) = egui::AreaState::load(ui.ctx(), module_area_id) {
                        use egui::epaint::*;
                        if let (Some(pivot_pos), Some(_size)) = (state.pivot_pos, state.size) {
                            // draw connections
                            for (input_idx, input_module) in module.get_inputs().iter().enumerate()
                            {
                                if let Some((input_module, port)) = input_module {
                                    let input_module = input_module.read().unwrap();
                                    let input_module_area_id =
                                        id.with(("module", input_module.get_id()));
                                    if let Some(input_module_area_state) =
                                        egui::AreaState::load(ui.ctx(), input_module_area_id)
                                    {
                                        if let (Some(src_pivot_pos), Some(src_pivot_size)) = (
                                            input_module_area_state.pivot_pos,
                                            input_module_area_state.size,
                                        ) {
                                            ui.painter().line_segment(
                                                [
                                                    [
                                                        pivot_pos.x + (SYNTH_HANDLE_SIZE / 2.0),
                                                        pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (input_idx as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                    [
                                                        src_pivot_pos.x + src_pivot_size.x
                                                            - (SYNTH_HANDLE_SIZE / 2.0),
                                                        src_pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (*port as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                ]
                                                .into(),
                                                Stroke::new(1.0, Color32::RED),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            ui.ctx()
                .set_transform_layer(area.response.layer_id, transform);
            ui.ctx().set_sublayer(window_layer, area.response.layer_id);
        }
    }
}
