use super::{
    AudioBuffer, AudioConfig, ControlVoltage, SharedSynthModule, SynthModule, TransitionDetector,
};
use egui;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::f64::consts::PI;
use uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct OscillatorModule {
    id: String,
    pub val: ControlVoltage,
    #[serde(skip)]
    input: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    sync_input: Option<(SharedSynthModule, u8)>,
    sample_rate: u16,
    sine: AudioBuffer,
    square: AudioBuffer,
    saw: AudioBuffer,
    pos: f64,
    antialiasing: bool,
    sync_detector: TransitionDetector,
}

impl OscillatorModule {
    pub fn new(audio_config: &AudioConfig) -> OscillatorModule {
        OscillatorModule {
            id: uuid::Uuid::new_v4().into(),
            input: None,
            sync_input: None,
            val: 0.0,
            sample_rate: audio_config.sample_rate,
            sine: AudioBuffer::new(Some(audio_config.buffer_size)),
            square: AudioBuffer::new(Some(audio_config.buffer_size)),
            saw: AudioBuffer::new(Some(audio_config.buffer_size)),
            pos: 0.0,
            antialiasing: true,
            sync_detector: TransitionDetector::new(),
        }
    }

    fn get_freq_in_hz(&self, buf: Option<&[ControlVoltage]>, i: usize) -> f64 {
        match buf {
            Some(buf) => 440.0 * (2.0_f64.powf(<f64>::from(buf[i]) + <f64>::from(self.val))),
            None => 440.0 * (2.0_f64.powf(<f64>::from(self.val))),
        }
    }

    fn poly_blep(t: f64, dt: f64) -> f64 {
        // adopted from https://www.martin-finke.de/articles/audio-plugins-018-polyblep-oscillator/
        if dt == 0.0 {
            return 0.0;
        }
        // 0 <= t < 1
        let mut t = t;
        if t < dt {
            t /= dt;
            return t + t - t * t - 1.0;
        }
        // -1 < t < 0
        else if t > 1.0 - dt {
            t = (t - 1.0) / dt;
            return t * t + t + t + 1.0;
        }
        0.0
    }

    pub fn get_name() -> String {
        "Oscillator".to_string()
    }
}

impl SynthModule for OscillatorModule {
    fn get_name(&self) -> String {
        OscillatorModule::get_name()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.sample_rate = audio_config.sample_rate;
        self.sine.resize(audio_config.buffer_size);
        self.square.resize(audio_config.buffer_size);
        self.saw.resize(audio_config.buffer_size);
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.sine.clone()),
            1 => Ok(self.square.clone()),
            2 => Ok(self.saw.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0 => Ok(Some("Sine".to_string())),
            1 => Ok(Some("Square".to_string())),
            2 => Ok(Some("Sawtooth".to_string())),
            _ => Err(()),
        }
    }

    fn calc(&mut self) {
        AudioBuffer::with_write_many(
            vec![self.sine.clone(), self.square.clone(), self.saw.clone()],
            |out_bufs| {
                AudioBuffer::with_read_many(
                    vec![
                        self.resolve_input(0).unwrap(),
                        self.resolve_input(1).unwrap(),
                    ],
                    |in_bufs| {
                        let (cv, sync_in) = in_bufs.into_iter().collect_tuple().unwrap();
                        let (sine, square, saw) = out_bufs
                            .into_iter()
                            .map(|b| b.unwrap())
                            .collect_tuple()
                            .unwrap();
                        for i in 0..sine.len() {
                            let sync_val = match sync_in {
                                Some(v) => v[i],
                                None => 0.0,
                            };
                            if self.sync_detector.is_transition(&sync_val) {
                                self.pos = 0.0;
                            }
                            let delta = self.get_freq_in_hz(cv, i) / (self.sample_rate as f64);
                            sine[i] = (self.pos * PI * 2.0).sin() as ControlVoltage;

                            square[i] = if self.pos < 0.5 { -1.0 } else { 1.0 }
                                - if self.antialiasing {
                                    (Self::poly_blep(self.pos, delta)
                                        - Self::poly_blep((self.pos + 0.5) % 1.0, delta))
                                        as f32
                                } else {
                                    0.0
                                };

                            saw[i] = (self.pos as ControlVoltage * 2.0 - 1.0)
                                - if self.antialiasing {
                                    Self::poly_blep(self.pos, delta) as f32
                                } else {
                                    0.0
                                };

                            self.pos = self.pos + delta;
                            self.pos = self.pos % 1.0;
                        }
                    },
                );
            },
        );
    }

    fn get_num_outputs(&self) -> u8 {
        3
    }

    fn get_input(&self, idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match idx {
            0 => Ok(self.input.clone()),
            1 => Ok(self.sync_input.clone()),
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("CV".to_string())),
            1 => Ok(Some("Sync".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.input = Some((src_module, src_port));
                Ok(())
            }
            1 => {
                self.sync_input = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.input = None;
                Ok(())
            }
            1 => {
                self.sync_input = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("osc").show(ui, |ui| {
            ui.label("Coarse");
            ui.add(
                egui::Slider::new(&mut self.val, -9.0..=6.0)
                    .step_by(1.0 / 12.0)
                    .show_value(false),
            );
            ui.scope(|ui| {
                if ui.button("-").clicked() {
                    self.val -= 1.0;
                }
                if ui.button("+").clicked() {
                    self.val += 1.0;
                }
            });
            ui.end_row();
            ui.label("Note");
            let floor = self.val.floor();
            ui.add(
                egui::Slider::new(&mut self.val, floor..=floor + 11.0 / 12.0)
                    .step_by(1.0 / 12.0)
                    .show_value(false),
            );
            ui.scope(|ui| {
                if ui.button("-").clicked() {
                    self.val -= 1.0 / 12.0;
                }
                if ui.button("+").clicked() {
                    self.val += 1.0 / 12.0;
                }
            });
            let note = ((self.val + (1.0 / 24.0)) * 12.0).floor() / 12.0;
            ui.end_row();
            ui.label("Fine");
            ui.add(
                egui::Slider::new(
                    &mut self.val,
                    note - 1.0 / 24.0 + 0.00001..=note + 1.0 / 24.0 - (1.0 / 12.0 / 100.0),
                )
                .step_by(1.0 / 12.0 / 100.0)
                .show_value(false),
            );
            ui.scope(|ui| {
                if ui.button("-").clicked() {
                    self.val -= 1.0 / 12.0 / 100.0;
                }
                if ui.button("+").clicked() {
                    self.val += 1.0 / 12.0 / 100.0;
                }
            });
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ui_dirty(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod dco_tests {
    use super::*;

    #[test]
    fn produces_440() {
        let mut module = OscillatorModule::new(&AudioConfig {
            sample_rate: 440 * 4,
            buffer_size: 17,
            channels: 2,
        }); // notice odd sized buffer
        module.calc();
        {
            let output = module.get_output(0).unwrap();
            let buf = output.get().unwrap();
            assert_eq!(buf[0], 0.0);
            assert!((buf[1] - 1.0).abs() < 0.00001);
            assert!(buf[2].abs() < 0.00001);
            assert!((buf[3] + 1.0).abs() < 0.00001);
            assert!(buf[4].abs() < 0.00001);
        }
        module.calc();
        let output = module.get_output(0).unwrap();
        let buf = output.get().unwrap();
        assert!((buf[0] - 1.0).abs() < 0.00001); // should continue smoothly into next buffer
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NoiseModule {
    id: String,
    out: AudioBuffer,
}

impl NoiseModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().into(),
            out: AudioBuffer::new(Some(audio_config.buffer_size)),
        }
    }

    pub fn get_name() -> String {
        "Noise".to_string()
    }
}

impl SynthModule for NoiseModule {
    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.out.resize(audio_config.buffer_size);
    }

    fn get_input(&self, _input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        Err(())
    }

    fn get_num_inputs(&self) -> u8 {
        0
    }

    fn get_input_label(&self, _input_idx: u8) -> Result<Option<String>, ()> {
        Err(())
    }

    fn set_input(
        &mut self,
        _input_idx: u8,
        _src_module: SharedSynthModule,
        _src_port: u8,
    ) -> Result<(), ()> {
        Err(())
    }

    fn get_num_outputs(&self) -> u8 {
        1
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        if output_idx == 0 {
            Ok(self.out.clone())
        } else {
            Err(())
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        if output_idx == 0 { Ok(None) } else { Err(()) }
    }

    fn disconnect_input(&mut self, _input_idx: u8) -> Result<(), ()> {
        Err(())
    }

    fn calc(&mut self) {
        self.out.with_write(|out| {
            let out = out.unwrap();
            for sample in out.iter_mut() {
                *sample = (rand::random::<f32>() - 0.5) * 2.0;
            }
        });
    }

    fn as_any(&self) -> &dyn Any {
        return self;
    }
}
