use super::{
    AudioBuffer, AudioConfig, ControlVoltage, SharedSynthModule, SynthModule, TransitionDetector,
};
use serde::{Deserialize, Serialize};
use std::any::Any;

#[derive(Serialize, Deserialize, Clone)]
pub struct ADSRModule {
    id: String,
    a_sec: f32,
    d_sec: f32,
    s_val: ControlVoltage,
    r_sec: f32,
    phase: f32,
    mode: ADSRMode,
    r_val: ControlVoltage,
    from_a_val: ControlVoltage,
    sample_rate: f32,
    #[serde(skip)]
    gate_in: Option<(SharedSynthModule, u8)>,
    transition_detector: TransitionDetector,
    output_buffer: AudioBuffer,
    ui_dirty: bool,
}

#[derive(Serialize, Deserialize, Clone)]
enum ADSRMode {
    Attack,
    Decay,
    Sustain,
    Release,
    None,
}

impl ADSRModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            a_sec: 0.0,
            d_sec: 0.5,
            s_val: 0.25,
            r_sec: 0.5,
            phase: 0.0,
            mode: ADSRMode::None,
            r_val: 0.0,
            from_a_val: 0.0,
            sample_rate: audio_config.sample_rate as f32,
            gate_in: None,
            transition_detector: TransitionDetector::new(),
            output_buffer: AudioBuffer::new(Some(audio_config.buffer_size)),
            ui_dirty: false,
        }
    }

    pub fn get_name() -> String {
        "ADSR".to_string()
    }
}

impl SynthModule for ADSRModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.output_buffer.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        1
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.gate_in.clone()),
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
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.gate_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Gate".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_outputs(&self) -> u8 {
        1
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.output_buffer.clone()),
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
        self.resolve_input(0).unwrap().with_read(|gate_in_buf| {
            self.output_buffer.with_write(|output_buffer| {
                let output_buffer = output_buffer.unwrap();
                for idx in 0..output_buffer.len() {
                    let is_transition = self.transition_detector.is_transition(match gate_in_buf {
                        Some(buf) => &buf[idx],
                        None => &0.0,
                    });
                    match self.mode {
                        ADSRMode::None => {
                            if gate_in_buf.is_some() && gate_in_buf.unwrap()[idx] > 0.0 {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Attack;
                                self.ui_dirty = true;
                            }
                        }
                        ADSRMode::Attack => {
                            self.phase += 1.0 / (self.sample_rate * self.a_sec);
                            if self.phase >= 1.0 {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Decay;
                                self.ui_dirty = true;
                            } else if is_transition {
                                self.phase = 0.0;
                                self.r_val = self.from_a_val;
                            }
                        }
                        ADSRMode::Decay => {
                            self.phase += 1.0 / (self.sample_rate * self.d_sec);
                            if self.phase >= 1.0 {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Sustain;
                                self.ui_dirty = true;
                            }
                            if is_transition {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Attack;
                                self.ui_dirty = true;
                            }
                        }
                        ADSRMode::Sustain => {
                            if gate_in_buf.is_none() || gate_in_buf.unwrap()[idx] <= 0.0 {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Release;
                                self.ui_dirty = true;
                            }
                            if is_transition {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Attack;
                                self.ui_dirty = true;
                            }
                        }
                        ADSRMode::Release => {
                            if gate_in_buf.is_some() && gate_in_buf.unwrap()[idx] > 0.0 {
                                self.phase = 0.0;
                                self.mode = ADSRMode::Attack;
                                self.ui_dirty = true;
                            }
                            self.phase += 1.0 / (self.sample_rate * self.r_sec);
                            if self.phase >= 1.0 {
                                self.phase = 0.0;
                                self.r_val = 0.0;
                                self.mode = ADSRMode::None;
                                self.ui_dirty = true;
                            }
                        }
                    }
                    output_buffer[idx] = match self.mode {
                        ADSRMode::None => 0.0,
                        ADSRMode::Attack => self.r_val + (1.0 - self.r_val) * self.phase,
                        ADSRMode::Decay => self.s_val + (1.0 - self.s_val) * (1.0 - self.phase),
                        ADSRMode::Sustain => self.s_val,
                        ADSRMode::Release => self.s_val * (1.0 - self.phase),
                    };
                    if !matches!(self.mode, ADSRMode::Attack) {
                        self.r_val = output_buffer[idx];
                    } else {
                        self.from_a_val = output_buffer[idx];
                    }
                }
            });
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.a_sec, 0.0..=1.0)
                        .orientation(egui::SliderOrientation::Vertical),
                );
                if matches!(self.mode, ADSRMode::Attack) {
                    ui.colored_label(egui::Color32::RED, "A");
                } else {
                    ui.label("A");
                }
            });
            ui.vertical(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.d_sec, 0.0..=1.0)
                        .orientation(egui::SliderOrientation::Vertical),
                );
                if matches!(self.mode, ADSRMode::Decay) {
                    ui.colored_label(egui::Color32::RED, "D");
                } else {
                    ui.label("D");
                }
            });
            ui.vertical(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.s_val, 0.0..=1.0)
                        .orientation(egui::SliderOrientation::Vertical),
                );
                if matches!(self.mode, ADSRMode::Sustain) {
                    ui.colored_label(egui::Color32::RED, "S");
                } else {
                    ui.label("S");
                }
            });
            ui.vertical(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.r_sec, 0.0..=1.0)
                        .orientation(egui::SliderOrientation::Vertical),
                );
                if matches!(self.mode, ADSRMode::Release) {
                    ui.colored_label(egui::Color32::RED, "R");
                } else {
                    ui.label("R");
                }
            });
        });
        self.ui_dirty = false;
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ui_dirty(&self) -> bool {
        self.ui_dirty
    }
}
