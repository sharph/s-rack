use super::{AudioBuffer, AudioConfig, ControlVoltage, SharedSynthModule, SynthModule};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{any::Any, ops::Add};
use uuid;

#[derive(Serialize, Deserialize, Clone)]
pub enum MathOperation {
    Add,
    Subtract,
    Multiply,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MathModule {
    id: String,
    #[serde(skip)]
    in1: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    in2: Option<(SharedSynthModule, u8)>,
    buf: AudioBuffer,
    constant: ControlVoltage,
    operation: MathOperation,
}

impl MathModule {
    pub fn new(audio_config: &AudioConfig, operation: MathOperation) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            in1: None,
            in2: None,
            buf: AudioBuffer::new(Some(audio_config.buffer_size)),
            constant: 0.0,
            operation,
        }
    }

    pub fn get_name(operation: &MathOperation) -> String {
        match operation {
            MathOperation::Add => "Add".to_string(),
            MathOperation::Subtract => "Subtract".to_string(),
            MathOperation::Multiply => "Multiply".to_string(),
        }
    }

    #[inline]
    fn operation(&self, a: ControlVoltage, b: ControlVoltage) -> ControlVoltage {
        match self.operation {
            MathOperation::Add => a + b,
            MathOperation::Subtract => a - b,
            MathOperation::Multiply => a * b,
        }
    }
}

impl SynthModule for MathModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name(&self.operation)
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.buf.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.in1.clone()),
            1 => Ok(self.in2.clone()),
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
                self.in1 = Some((src_module, src_port));
                Ok(())
            }
            1 => {
                self.in2 = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.in1 = None;
                Ok(())
            }
            1 => {
                self.in2 = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("In1".to_string())),
            1 => Ok(Some("In2".to_string())),
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
                let (i1, i2) = bufs.into_iter().collect_tuple().unwrap();
                self.buf.with_write(|output| {
                    let output = output.unwrap();
                    for idx in 0..output.len() {
                        output[idx] = match (i1, i2) {
                            (Some(i1), Some(i2)) => self.operation(i1[idx], i2[idx]),
                            (Some(i1), None) => self.operation(i1[idx], self.constant),
                            (None, Some(i2)) => self.operation(0.0, i2[idx]),
                            (None, None) => self.operation(0.0, self.constant),
                        }
                    }
                });
            },
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        if self.in2.is_none() {
            ui.add(
                egui::Slider::new(&mut self.constant, -2.0..=2.0)
                    .orientation(egui::SliderOrientation::Vertical),
            );
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
