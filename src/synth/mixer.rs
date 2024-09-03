use super::{AudioBuffer, AudioConfig, SharedSynthModule, SynthModule};
use egui;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct MonoMixerModule {
    id: String,
    #[serde(skip)]
    audio_in: Vec<Option<(SharedSynthModule, u8)>>,
    gain: Vec<f32>,
    buf: AudioBuffer,
}

impl MonoMixerModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            audio_in: vec![None; 4],
            gain: vec![1.0; 4],
            buf: AudioBuffer::new(Some(audio_config.buffer_size)),
        }
    }

    pub fn get_name() -> String {
        "Mono Mixer".to_string()
    }
}

impl SynthModule for MonoMixerModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.audio_in.resize(self.gain.len(), None);
        self.buf.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        self.audio_in.len() as u8
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        if input_idx < self.audio_in.len() as u8 {
            return Ok(self.audio_in[input_idx as usize].clone());
        }
        Err(())
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        if input_idx < self.audio_in.len() as u8 {
            self.audio_in[input_idx as usize] = Some((src_module, src_port));
            return Ok(());
        }
        Err(())
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        if input_idx < self.audio_in.len() as u8 {
            self.audio_in[input_idx as usize] = None;
            return Ok(());
        }
        Err(())
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        if input_idx < self.audio_in.len() as u8 {
            return Ok(None);
        }
        Err(())
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
            (0..self.get_num_inputs())
                .into_iter()
                .map(|n| self.resolve_input(n).unwrap())
                .collect_vec(),
            |bufs| {
                self.buf.with_write(|output| {
                    let output = output.unwrap();
                    output.fill(0.0);
                    for (buf, gain) in bufs.into_iter().zip(self.gain.iter()) {
                        if buf.is_none() {
                            continue;
                        }
                        let buf = buf.unwrap();
                        for (src, dst) in buf.iter().zip(output.iter_mut()) {
                            *dst += src * gain;
                        }
                    }
                });
            },
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for gain in self.gain.iter_mut() {
                ui.add(
                    egui::Slider::new(gain, 0.0..=2.0)
                        .orientation(egui::SliderOrientation::Vertical),
                );
            }
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
