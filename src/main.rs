use cpal::traits::DeviceTrait;
use cpal::traits::HostTrait;
use cpal::traits::StreamTrait;
use cpal::SupportedBufferSize;
use eframe;
use egui;
use log;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;

mod synth;
mod ui;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    use eframe::App;
    let audio_config = synth::AudioConfig {
        sample_rate: 48000,
        buffer_size: 1024,
        channels: 2,
    };
    println!("Hello, world!");
    let workspace = ui::SynthModuleWorkspace::new();
    workspace.set_audio_config(audio_config.clone());
    workspace.add_module(Arc::new(RwLock::new(synth::OutputModule::new(
        &audio_config,
    ))));
    let mut app = SRackApp::new(workspace.clone());
    let mut audio_engine: Option<AudioEngine> = None;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };
    eframe::run_simple_native("s-rack", options, move |ctx, frame| {
        if audio_engine.is_none() {
            audio_engine = Some(AudioEngine::new(
                &audio_config,
                workspace.get_plan(),
                workspace.get_output(),
                Some(ctx.clone()),
            ));
        }
        app.update(ctx, frame)
    })
}

struct AudioEngine {
    stream: cpal::Stream,
    audio_config: synth::AudioConfig,
}

impl AudioEngine {
    fn default() {}

    fn new(
        audio_config: &synth::AudioConfig,
        plan: Arc<Mutex<Vec<synth::SharedSynthModule>>>,
        output: Arc<Mutex<Option<synth::SharedSynthModule>>>,
        mut ctx: Option<egui::Context>,
    ) -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("No output device available!");
        println!(
            "Sample rate: {}, Buffer size: {}, channels: {}",
            audio_config.sample_rate, audio_config.buffer_size, audio_config.channels
        );
        let mut src_buf_idx = 0;
        let mut src_buf: Box<[Box<[f32]>]> = (0..audio_config.channels)
            .map(|_| (0..audio_config.buffer_size).map(|_| 0.0).collect())
            .collect();
        let channels = usize::from(audio_config.channels);
        let buffer_size = usize::from(audio_config.buffer_size);
        let stream = device
            .build_output_stream(
                &cpal::StreamConfig {
                    channels: audio_config.channels as u16,
                    sample_rate: cpal::SampleRate(audio_config.sample_rate as u32),
                    buffer_size: cpal::BufferSize::Fixed(
                        audio_config.buffer_size.try_into().unwrap(),
                    ),
                },
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let plan = plan.lock().unwrap();
                    for out_idx in 0..data.len() {
                        if src_buf_idx == 0 && out_idx % channels == 0 {
                            synth::execute(&plan);
                            let output_mutex = output.lock().unwrap();
                            if let Some(output_mutex_value) = output_mutex.as_ref() {
                                let module = output_mutex_value.read().unwrap();
                                let output_module = module
                                    .as_any()
                                    .downcast_ref::<synth::OutputModule>()
                                    .unwrap();
                                for c in 0..channels {
                                    src_buf[c].copy_from_slice(&output_module.bufs[c]);
                                }
                            }
                        }
                        data[out_idx] = src_buf[out_idx % channels][src_buf_idx];
                        if out_idx % channels == channels - 1 {
                            src_buf_idx += 1;
                            if src_buf_idx >= buffer_size {
                                src_buf_idx = 0;
                            }
                        }
                    }

                    if ctx.is_some() && synth::ui_dirty(&plan) {
                        ctx.as_mut().unwrap().request_repaint();
                    }
                },
                move |_err| {},
                None,
            )
            .unwrap();
        stream.play().unwrap();
        Self {
            stream,
            audio_config: audio_config.clone(),
        }
    }
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
                        let audio_config = {
                            let workspace_arc = self.workspace.value();
                            let workspace = workspace_arc.read().unwrap();
                            workspace.audio_config.clone()
                        };
                        if audio_config.is_some() {
                            self.workspace.add_module(constuct(&audio_config.unwrap()));
                        }
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
