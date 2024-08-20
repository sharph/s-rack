use egui;
use std::any::Any;
use std::f64::consts::PI;
use std::iter::zip;
use std::sync::Arc;
use std::sync::RwLock;
use uuid;

#[derive(Clone)]
pub struct AudioConfig {
    pub sample_rate: u16,
    pub buffer_size: usize,
    pub channels: u8,
}

pub fn execute(plan: &Vec<SharedSynthModule>) {
    for ssm in plan {
        ssm.write().unwrap().calc();
    }
}

pub fn plan_execution(
    output: SharedSynthModule,
    all_modules: &Vec<SharedSynthModule>,
    plan: &mut Vec<SharedSynthModule>,
) -> () {
    let mut execution_list: Vec<(SharedSynthModule, bool)> = vec![(output, false)];
    let mut all_modules = all_modules.clone();
    let mut to_concat: Vec<(SharedSynthModule, bool)> = Vec::new();
    loop {
        if let Some((idx, (to_search, searched))) = execution_list
            .iter()
            .enumerate()
            .filter(|(_idx, (_to_search, searched))| !searched)
            .next()
        {
            // is there any module in our list we need to explore?
            for input in to_search.read().unwrap().get_inputs() {
                // add all inputs to list if not already in list
                if let Some((input, _)) = input {
                    if !execution_list
                        .iter()
                        .any(|(to_compare, _)| shared_are_eq(&input, to_compare))
                        && !to_concat
                            .iter()
                            .any(|(to_compare, _)| shared_are_eq(&input, to_compare))
                    {
                        to_concat.push((input, false));
                    }
                }
            }
            execution_list[idx].1 = true;
        } else {
            // start processing modules not connected to output via graph
            if let Some(possibly_unconnected) = all_modules.pop() {
                if !execution_list
                    .iter()
                    .any(|(to_compare, _)| shared_are_eq(&possibly_unconnected, to_compare))
                {
                    to_concat.push((possibly_unconnected, false))
                }
            } else {
                // we are at end
                break;
            }
        }
        execution_list.append(&mut to_concat);
    }
    plan.clear();
    plan.append(
        &mut execution_list
            .iter()
            .rev()
            .map(|(sm, _searched)| {
                println!(
                    "{} {}",
                    sm.read().unwrap().get_name(),
                    sm.read().unwrap().get_id()
                );
                sm.clone()
            })
            .collect(),
    );
}

pub fn connect(
    src_module: SharedSynthModule,
    src_port: u8,
    sink_module: SharedSynthModule,
    sink_port: u8,
) -> Result<(), ()> {
    let mut sink_module_write = sink_module.write().unwrap();
    sink_module_write.set_input(sink_port, src_module.clone(), src_port)?;
    Ok(())
}

type ControlVoltage = f32;

pub trait SynthModule: Any {
    fn get_name(&self) -> String;
    fn get_id(&self) -> String;
    fn calc(&mut self);
    fn get_num_inputs(&self) -> u8;
    fn get_num_outputs(&self) -> u8;
    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()>;
    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>>;
    fn get_output(&self, output_idx: u8) -> Result<&[ControlVoltage], ()>;
    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()>;
    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()>;
    fn ui(&mut self, ui: &mut egui::Ui);
    fn as_any(&self) -> &dyn Any;
}
impl PartialEq for dyn SynthModule {
    fn eq(&self, other: &Self) -> bool {
        self.get_id() == other.get_id()
    }
}
impl Eq for dyn SynthModule {}
pub type SharedSynthModule = Arc<RwLock<dyn SynthModule + Send + Sync>>;

pub fn shared_are_eq(a: &SharedSynthModule, b: &SharedSynthModule) -> bool {
    let a = { a.read().unwrap().get_id() };
    let b = { b.read().unwrap().get_id() };
    a == b
}

pub struct OscillatorModule {
    id: String,
    pub val: ControlVoltage,
    input: Option<(SharedSynthModule, u8)>,
    sample_rate: u16,
    sine: Box<[ControlVoltage]>,
    square: Box<[ControlVoltage]>,
    saw: Box<[ControlVoltage]>,
    pos: f64,
}

impl OscillatorModule {
    pub fn new(audio_config: &AudioConfig) -> OscillatorModule {
        OscillatorModule {
            id: uuid::Uuid::new_v4().into(),
            input: None,
            val: 0.0,
            sample_rate: audio_config.sample_rate,
            sine: (0..audio_config.buffer_size).map(|_| 0.0).collect(),
            square: (0..audio_config.buffer_size).map(|_| 0.0).collect(),
            saw: (0..audio_config.buffer_size).map(|_| 0.0).collect(),
            pos: 0.0,
        }
    }

    fn get_freq_in_hz(&self, buf: Option<&[ControlVoltage]>, i: usize) -> f64 {
        match buf {
            Some(buf) => 440.0 * (2.0_f64.powf(<f64>::from(buf[i]) + <f64>::from(self.val))),
            None => 440.0 * (2.0_f64.powf(<f64>::from(self.val))),
        }
    }
}

impl SynthModule for OscillatorModule {
    fn get_name(&self) -> String {
        "Oscillator".to_string()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_output(&self, output_idx: u8) -> Result<&[ControlVoltage], ()> {
        match output_idx {
            0 => Ok(&self.sine),
            1 => Ok(&self.square),
            2 => Ok(&self.saw),
            _ => Err(()),
        }
    }

    fn calc(&mut self) {
        let mut input_buf: Option<&[ControlVoltage]> = None;
        let input_module;
        if let Some((input, port)) = &self.input {
            input_module = input.read().unwrap();
            input_buf = Some(input_module.get_output(*port).unwrap());
        }
        for i in 0..self.sine.len() {
            self.sine[i] = (self.pos * PI * 2.0).sin() as ControlVoltage;
            self.square[i] = if self.pos < 0.5 { -1.0 } else { 1.0 };
            self.saw[i] = self.pos as ControlVoltage * 2.0 - 1.0;
            self.pos = self.pos + (self.get_freq_in_hz(input_buf, i) / (self.sample_rate as f64));
            self.pos = self.pos % 1.0;
        }
    }

    fn get_num_outputs(&self) -> u8 {
        3
    }

    fn get_input(&self, output_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        if output_idx == 0 {
            return Ok(self.input.clone());
        }
        Err(())
    }

    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>> {
        vec![self.input.clone()]
    }

    fn get_num_inputs(&self) -> u8 {
        1
    }

    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()> {
        if input_idx == 0 {
            self.input = Some((src_module, src_port));
            return Ok(());
        }
        Err(())
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        if input_idx != 0 {
            return Err(());
        }
        self.input = None;
        Ok(())
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("-").clicked() {
                self.val -= 1.0;
            }
            ui.label(self.val.to_string());
            if ui.button("+").clicked() {
                self.val += 1.0;
            }
        });
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod dco_tests {
    use super::*;

    #[test]
    fn produces_440() {
        let mut module = OscillatorModule::new(17, 440 * 4); // notice odd sized buffer
        module.calc();
        {
            let buf = module.get_output(0).unwrap();
            assert_eq!(buf[0], 0.0);
            assert!((buf[1] - 1.0).abs() < 0.00001);
            assert!(buf[2].abs() < 0.00001);
            assert!((buf[3] + 1.0).abs() < 0.00001);
            assert!(buf[4].abs() < 0.00001);
        }
        module.calc();
        let buf = module.get_output(0).unwrap();
        assert!((buf[0] - 1.0).abs() < 0.00001); // should continue smoothly into next buffer
    }
}

pub struct OutputModule {
    id: String,
    pub bufs: Box<[Box<[ControlVoltage]>]>,
    inputs: Box<[Option<(SharedSynthModule, u8)>]>,
}

impl OutputModule {
    pub fn new(audio_config: &AudioConfig) -> OutputModule {
        OutputModule {
            id: uuid::Uuid::new_v4().into(),
            bufs: (0..audio_config.channels)
                .map(|_| (0..audio_config.buffer_size).map(|_| 0.0).collect())
                .collect(),
            inputs: (0..audio_config.channels).map(|_| None).collect(),
        }
    }
}

impl SynthModule for OutputModule {
    fn get_name(&self) -> String {
        "Output".to_string()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn calc(&mut self) {
        for (input, buf) in zip(self.inputs.iter_mut(), self.bufs.iter_mut()) {
            match input {
                Some((module, port)) => {
                    let input_module = module.read().unwrap();
                    buf.copy_from_slice(input_module.get_output(*port).unwrap());
                }
                None => buf.fill(0.0),
            }
        }
    }

    fn get_num_outputs(&self) -> u8 {
        0
    }

    fn get_output(&self, _: u8) -> Result<&[ControlVoltage], ()> {
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

    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>> {
        (0..self.get_num_inputs())
            .map(|n| self.get_input(n).unwrap())
            .collect()
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
        self.inputs[<usize>::from(input_idx)] = Some((src_module.clone(), src_port));
        Ok(())
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        if input_idx >= self.get_num_inputs() {
            return Err(());
        }
        self.inputs[input_idx as usize] = None;
        Ok(())
    }

    fn ui(&mut self, ui: &mut egui::Ui) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}
