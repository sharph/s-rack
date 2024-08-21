use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use eframe;
use egui;
use log;
use std::sync::Arc;
use std::sync::RwLock;

mod synth;
mod ui;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    use eframe::App;

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
    let channels = <u8>::try_from(supported_config.channels()).unwrap();
    let audio_config = synth::AudioConfig {
        channels: 2,
        sample_rate: sample_rate as u16,
        buffer_size,
    };
    let output = Arc::new(RwLock::new(synth::OutputModule::new(&audio_config)));
    println!(
        "Sample rate: {}, Buffer size: {}, channels: {}",
        sample_rate, buffer_size, channels
    );
    let mut workspace = ui::SynthModuleWorkspace::new(audio_config.clone());
    workspace.add_module(output.clone());
    workspace.plan();
    let mut src_buf_pos: usize = 0;
    let output_ref = workspace.output.clone();
    let plan_ref = workspace.plan.clone();
    let mut app = SRackApp::new(workspace);
    let rc_ctx = app.ctx.clone();
    let stream = device
        .build_output_stream(
            &cpal::StreamConfig {
                channels: audio_config.channels as u16,
                sample_rate: cpal::SampleRate(audio_config.sample_rate as u32),
                buffer_size: cpal::BufferSize::Fixed(audio_config.buffer_size.try_into().unwrap()),
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
                        let channel = dst_buf_pos % <usize>::from(audio_config.channels);
                        match &src_buf {
                            Some(buf) => data[dst_buf_pos] = buf[channel][src_buf_pos],
                            None => data[dst_buf_pos] = 0.0,
                        }
                        if dst_buf_pos % <usize>::from(audio_config.channels)
                            == <usize>::from(audio_config.channels) - 1
                        {
                            src_buf_pos += 1;
                            if src_buf_pos >= buffer_size {
                                synth::execute(&plan);
                                src_buf_pos = 0;
                            }
                        }
                    }
                }
                if synth::ui_dirty(&plan) {
                    let mut ctx_ref = rc_ctx.write().unwrap();
                    if let Some(ctx) = ctx_ref.as_mut() {
                        ctx.request_repaint();
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
    eframe::run_simple_native("s-rack", options, move |ctx, frame| app.update(ctx, frame))
}

struct SRackApp {
    workspace: ui::SynthModuleWorkspace,
    ctx_set: bool,
    pub ctx: Arc<RwLock<Option<egui::Context>>>,
}

impl SRackApp {
    fn new(workspace: ui::SynthModuleWorkspace) -> Self {
        Self {
            workspace,
            ctx_set: false,
            ctx: Arc::new(RwLock::new(None)),
        }
    }
}

impl eframe::App for SRackApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.ctx_set {
            let mut shared_ctx_ref = self.ctx.write().unwrap();
            *shared_ctx_ref = Some(ctx.clone());
            self.ctx_set = true;
        }
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.menu_button("Modules", |ui| {
                for (name, constuct) in synth::get_catalog() {
                    if ui.button(name).clicked() {
                        self.workspace
                            .add_module(constuct(&self.workspace.audio_config));
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.workspace.ui(ui);
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    let audio_config = synth::AudioConfig {
        sample_rate: 48000,
        channels: 2,
        buffer_size: 1024,
    };
    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    let workspace = ui::SynthModuleWorkspace::new(audio_config.clone());

    wasm_bindgen_futures::spawn_local(async {
        let start_result = eframe::WebRunner::new()
            .start(
                "synth",
                web_options,
                Box::new(|cc| Ok(Box::new(SRackApp::new(workspace)))),
            )
            .await;

        // Remove the loading text and spinner:
        let loading_text = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("loading_text"));
        if let Some(loading_text) = loading_text {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
