use super::{AudioBuffer, AudioConfig, SharedSynthModule, SynthModule};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid;

#[derive(Serialize, Deserialize, Clone)]
pub struct VCAModule {
    id: String,
    #[serde(skip)]
    audio_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    cv_in: Option<(SharedSynthModule, u8)>,
    buf: AudioBuffer,
    negative: bool,
}

impl VCAModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            audio_in: None,
            cv_in: None,
            buf: AudioBuffer::new(Some(audio_config.buffer_size)),
            negative: false,
        }
    }

    pub fn get_name() -> String {
        "VCA".to_string()
    }
}

impl SynthModule for VCAModule {
    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.buf.resize(audio_config.buffer_size);
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.audio_in.clone()),
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
                self.audio_in = Some((src_module, src_port));
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
                self.audio_in = None;
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
            0 => Ok(Some("Audio".to_string())),
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
                let (audio_in, cv_in) = bufs.into_iter().collect_tuple().unwrap();
                self.buf.with_write(|output| {
                    let output = output.unwrap();
                    if let (Some(audio_buf), Some(cv_buf)) = (audio_in, cv_in) {
                        for (idx, val) in audio_buf
                            .iter()
                            .zip(cv_buf)
                            .map(|(audio, cv)| {
                                if self.negative || *cv > 0.0 {
                                    audio * cv
                                } else {
                                    0.0
                                }
                            })
                            .enumerate()
                        {
                            output[idx] = val;
                        }
                    } else {
                        output.fill(0.0);
                    }
                });
            },
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
