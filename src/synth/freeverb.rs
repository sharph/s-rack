use super::{AudioBuffer, AudioConfig, SharedSynthModule, SynthModule};
use freeverb::Freeverb;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid;

#[derive(Serialize, Deserialize)]
pub struct FreeverbModule {
    id: String,
    #[serde(skip)]
    left_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    right_in: Option<(SharedSynthModule, u8)>,
    left_out: AudioBuffer,
    right_out: AudioBuffer,
    #[serde(skip)]
    freeverb: Option<Freeverb>,
    sample_rate: usize,
    dampening: f64,
    dampening_ctl: f64,
    freeze: bool,
    freeze_ctl: bool,
    wet: f64,
    wet_ctl: f64,
    width: f64,
    width_ctl: f64,
    room_size: f64,
    room_size_ctl: f64,
    dry: f64,
    dry_ctl: f64,
}

impl Clone for FreeverbModule {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            left_in: self.left_in.clone(),
            right_in: self.right_in.clone(),
            left_out: self.left_out.clone(),
            right_out: self.right_out.clone(),
            freeverb: None,
            sample_rate: self.sample_rate,
            dampening: self.dampening,
            dampening_ctl: self.dampening_ctl,
            freeze: self.freeze,
            freeze_ctl: self.freeze_ctl,
            wet: self.wet,
            wet_ctl: self.wet_ctl,
            width: self.width,
            width_ctl: self.width_ctl,
            room_size: self.room_size,
            room_size_ctl: self.room_size_ctl,
            dry: self.dry,
            dry_ctl: self.dry_ctl,
        }
    }
}

impl FreeverbModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            left_in: None,
            right_in: None,
            left_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            right_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            freeverb: None,
            sample_rate: audio_config.sample_rate as usize,
            dampening: 0.5,
            dampening_ctl: 0.5,
            freeze: false,
            freeze_ctl: false,
            wet: 1.0,
            wet_ctl: 1.0,
            width: 0.5,
            width_ctl: 0.5,
            room_size: 0.5,
            room_size_ctl: 0.5,
            dry: 0.0,
            dry_ctl: 0.0,
        }
    }

    pub fn get_name() -> String {
        "Freeverb".to_string()
    }

    fn set_freeverb(&mut self, all: bool) {
        let freeverb = self.freeverb.as_mut().expect("freeverb not initialized");
        if self.dampening_ctl != self.dampening || all {
            self.dampening = self.dampening_ctl;
            freeverb.set_dampening(self.dampening);
        }
        if self.freeze_ctl != self.freeze || all {
            self.freeze = self.freeze_ctl;
            freeverb.set_freeze(self.freeze);
        }
        if self.wet_ctl != self.wet || all {
            self.wet = self.wet_ctl;
            freeverb.set_wet(self.wet);
        }
        if self.width_ctl != self.width || all {
            self.width = self.width_ctl;
            freeverb.set_width(self.width);
        }
        if self.room_size_ctl != self.room_size || all {
            self.room_size = self.room_size_ctl;
            freeverb.set_room_size(self.room_size);
        }
        if self.dry_ctl != self.dry || all {
            self.dry = self.dry_ctl;
            freeverb.set_dry(self.dry);
        }
    }
}

impl SynthModule for FreeverbModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.left_out.resize(audio_config.buffer_size);
        self.right_out.resize(audio_config.buffer_size);
        if self.sample_rate != audio_config.sample_rate as usize {
            self.freeverb = None;
            self.sample_rate = audio_config.sample_rate as usize;
        }
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.left_in.clone()),
            1 => Ok(self.right_in.clone()),
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
                self.left_in = Some((src_module, src_port));
                Ok(())
            }
            1 => {
                self.right_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.left_in = None;
                Ok(())
            }
            1 => {
                self.right_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Left".to_string())),
            1 => Ok(Some("Right".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_outputs(&self) -> u8 {
        2
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.left_out.clone()),
            1 => Ok(self.right_out.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0 => Ok(Some("Left".to_string())),
            1 => Ok(Some("Right".to_string())),
            _ => Err(()),
        }
    }

    fn calc(&mut self) {
        if self.freeverb.is_none() {
            self.freeverb = Some(Freeverb::new(self.sample_rate));
            self.set_freeverb(true);
        } else {
            self.set_freeverb(false);
        }
        AudioBuffer::with_read_many(
            vec![
                self.resolve_input(0).unwrap(),
                self.resolve_input(1).unwrap(),
            ],
            |bufs| {
                let (left_in, right_in) = bufs.into_iter().collect_tuple().unwrap();
                AudioBuffer::with_write_many(
                    vec![self.left_out.clone(), self.right_out.clone()],
                    |bufs| {
                        let (left_out, right_out) = bufs
                            .into_iter()
                            .map(|b| b.unwrap())
                            .collect_tuple()
                            .unwrap();
                        let freeverb = self.freeverb.as_mut().unwrap();
                        match (left_in, right_in) {
                            (Some(l), Some(r)) => {
                                for (((li, ri), lo), ro) in l
                                    .iter()
                                    .zip(r.iter())
                                    .zip(left_out.iter_mut())
                                    .zip(right_out.iter_mut())
                                {
                                    let (l, r) = freeverb.tick((*li as f64, *ri as f64));
                                    (*lo, *ro) = (l as f32, r as f32);
                                }
                            }
                            (Some(l), None) => {
                                for ((li, lo), ro) in
                                    l.iter().zip(left_out.iter_mut()).zip(right_out.iter_mut())
                                {
                                    let (l, r) = freeverb.tick((*li as f64, 0.0));
                                    (*lo, *ro) = (l as f32, r as f32);
                                }
                            }
                            (None, Some(r)) => {
                                for ((ri, lo), ro) in
                                    r.iter().zip(left_out.iter_mut()).zip(right_out.iter_mut())
                                {
                                    let (l, r) = freeverb.tick((0.0, *ri as f64));
                                    (*lo, *ro) = (l as f32, r as f32);
                                }
                            }
                            (None, None) => {
                                for (lo, ro) in left_out.iter_mut().zip(right_out.iter_mut()) {
                                    let (l, r) = freeverb.tick((0.0, 0.0));
                                    (*lo, *ro) = (l as f32, r as f32);
                                }
                            }
                        }
                    },
                );
            },
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label("Dampening");
            ui.add(egui::Slider::new(&mut self.dampening_ctl, 0.0..=2.0));
            ui.label("Width");
            ui.add(egui::Slider::new(&mut self.width_ctl, 0.0..=1.0));
            ui.label("Room Size");
            ui.add(egui::Slider::new(&mut self.room_size_ctl, 0.0..=1.0));
            ui.label("Wet");
            ui.add(egui::Slider::new(&mut self.wet_ctl, 0.0..=1.0));
            ui.label("Dry");
            ui.add(egui::Slider::new(&mut self.dry_ctl, 0.0..=1.0));
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
