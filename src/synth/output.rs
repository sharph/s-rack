use super::{AudioBuffer, AudioConfig, SharedSynthModule, SynthModule};
use std::any::Any;
use uuid;

pub struct OutputModule {
    id: String,
    pub bufs: Box<[AudioBuffer]>,
    inputs: Box<[Option<(SharedSynthModule, u8)>]>,
}

impl OutputModule {
    pub fn new(audio_config: &AudioConfig) -> OutputModule {
        OutputModule {
            id: uuid::Uuid::new_v4().into(),
            bufs: (0..audio_config.channels)
                .map(|_| AudioBuffer::new(Some(audio_config.buffer_size)))
                .collect(),
            inputs: (0..audio_config.channels).map(|_| None).collect(),
        }
    }

    pub fn get_name() -> String {
        "Output".to_string()
    }
}

impl SynthModule for OutputModule {
    fn get_name(&self) -> String {
        OutputModule::get_name()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn calc(&mut self) {
        for input_idx in 0..self.get_num_inputs() {
            self.resolve_input(input_idx)
                .unwrap()
                .with_read(|input_buf| {
                    self.bufs[input_idx as usize].with_write(|output_buf| {
                        let output_buf = output_buf.unwrap();
                        match input_buf {
                            Some(buf) => output_buf.clone_from_slice(buf),
                            None => output_buf.fill(0.0),
                        }
                    });
                });
        }
    }

    fn get_num_outputs(&self) -> u8 {
        0
    }

    fn get_output(&self, _: u8) -> Result<AudioBuffer, ()> {
        Err(())
    }

    fn get_output_label(&self, _output_idx: u8) -> Result<Option<String>, ()> {
        Err(())
    }

    fn get_num_inputs(&self) -> u8 {
        self.inputs.len().try_into().unwrap()
    }

    fn get_input(&self, idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        if <usize>::from(idx) > self.inputs.len() {
            return Err(());
        }
        Ok(self.inputs[<usize>::from(idx)].clone())
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        if input_idx >= self.get_num_inputs() {
            return Err(());
        }
        Ok(None)
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        if input_idx >= self.get_num_inputs() {
            return Err(());
        }
        self.inputs[<usize>::from(input_idx)] = Some((src_module, src_port));
        Ok(())
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        if input_idx >= self.get_num_inputs() {
            return Err(());
        }
        self.inputs[input_idx as usize] = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
