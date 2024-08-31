use super::{
    AudioBuffer, AudioConfig, ControlVoltage, SharedSynthModule, SynthModule, TransitionDetector,
};
use crate::ui::run_async;
use cpal::{Sample, I24};
use hound::{SampleFormat, WavReader};
use itertools::Itertools;
use rfd::AsyncFileDialog;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::error::Error;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use uuid;

#[derive(Default, Serialize, Deserialize)]
struct WaveBox {
    samples: Vec<ControlVoltage>,
    sample_rate: f32,
    new: bool,
}

#[derive(Debug)]
struct DecodeError {}
impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error decoding wav file")
    }
}
impl Error for DecodeError {}

impl WaveBox {
    fn load(&mut self, wav_data: Vec<u8>) -> Result<(), Box<dyn Error>> {
        let reader = WavReader::new(Cursor::new(wav_data))?;
        let spec = reader.spec();
        self.samples.clear();
        match spec.sample_format {
            SampleFormat::Float => {
                for sample in reader
                    .into_samples()
                    .map(|s| s.unwrap())
                    .enumerate()
                    .filter(|(idx, _)| idx % spec.channels as usize == 0)
                    .map(|(_, sample)| sample)
                {
                    self.samples.push(sample);
                }
            }
            SampleFormat::Int => {
                let convert = match spec.bits_per_sample {
                    8 => |x: i32| x as f32 / (i8::MAX as f32 + 1.0),
                    16 => |x: i32| x as f32 / (i16::MAX as f32 + 1.0),
                    24 => |x: i32| I24::new_unchecked(x).to_float_sample(),
                    _ => return Err(Box::new(DecodeError {})),
                };
                for sample in reader
                    .into_samples()
                    .map(|s| convert(s.unwrap()))
                    .enumerate()
                    .filter(|(idx, _)| idx % spec.channels as usize == 0)
                    .map(|(_, sample)| sample)
                {
                    self.samples.push(sample);
                }
            }
        }
        self.new = true;
        self.sample_rate = spec.sample_rate as f32;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SampleModule {
    id: String,
    #[serde(skip)]
    gate_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    cv_in: Option<(SharedSynthModule, u8)>,
    transition_detector: TransitionDetector,
    pos: f32,
    buf: AudioBuffer,
    wavebox: Arc<Mutex<WaveBox>>,
    playing: bool,
    sample_rate: f32,
}

impl SampleModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            gate_in: None,
            cv_in: None,
            transition_detector: TransitionDetector::new(),
            pos: 0.0,
            buf: AudioBuffer::new(Some(audio_config.buffer_size)),
            wavebox: Arc::new(Mutex::new(WaveBox::default())),
            playing: false,
            sample_rate: audio_config.sample_rate as f32,
        }
    }

    pub fn get_name() -> String {
        "Sample".to_string()
    }
}

impl SynthModule for SampleModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.sample_rate = audio_config.sample_rate as f32;
        self.buf.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.gate_in.clone()),
            1 => Ok(self.cv_in.clone()),
            _ => Err(()),
        }
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.gate_in = Some((src_module, src_port));
                Ok(())
            }
            1 => {
                self.cv_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.gate_in = None;
                Ok(())
            }
            1 => {
                self.cv_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Gate".to_string())),
            1 => Ok(Some("CV".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_outputs(&self) -> u8 {
        1
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.buf.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0 => Ok(None),
            _ => Err(()),
        }
    }

    fn calc(&mut self) {
        AudioBuffer::with_read_many(
            vec![
                self.resolve_input(0).unwrap(),
                self.resolve_input(1).unwrap(),
            ],
            |bufs| {
                let (gate_in, cv_in) = bufs.into_iter().collect_tuple().unwrap();
                self.buf.with_write(|output| {
                    let output = output.unwrap();
                    let wavebox = self.wavebox.try_lock();
                    if wavebox.is_err() {
                        self.transition_detector
                            .is_transition(gate_in.map(|i| &i[i.len() - 1]).unwrap_or(&0.0));
                        output.fill(0.0);
                        return;
                    }
                    let mut wavebox = wavebox.unwrap();
                    if wavebox.new {
                        self.pos = 0.0;
                        self.playing = false;
                        wavebox.new = false;
                    }
                    for (idx, out) in output.iter_mut().enumerate() {
                        let trigger = self
                            .transition_detector
                            .is_transition(gate_in.map(|i| &i[idx]).unwrap_or(&0.0));
                        if trigger {
                            self.pos = 0.0;
                            self.playing = true;
                        }
                        if self.pos as usize >= wavebox.samples.len() {
                            self.pos = 0.0;
                            self.playing = false;
                        }
                        if wavebox.samples.len() > 0 {
                            *out = wavebox.samples[self.pos as usize];
                        } else {
                            *out = 0.0;
                        }
                        if self.playing {
                            self.pos += wavebox.sample_rate / self.sample_rate
                                * 2.0_f32.powf(cv_in.map(|i| i[idx]).unwrap_or(0.0_f32));
                        }
                    }
                });
            },
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if ui.button("Load Sample...").clicked() {
            let wavebox = self.wavebox.clone();
            run_async(async move {
                let file = AsyncFileDialog::new()
                    .add_filter("audio", &["wav"])
                    .pick_file()
                    .await;
                match file {
                    Some(file) => {
                        let data = file.read().await;
                        let mut unlocked_wavebox = wavebox.lock().unwrap();
                        let _ = unlocked_wavebox.load(data);
                    }
                    None => (),
                }
            });
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
