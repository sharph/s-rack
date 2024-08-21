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

pub fn ui_dirty(plan: &Vec<SharedSynthModule>) -> bool {
    plan.iter().any(|module| module.read().unwrap().ui_dirty())
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
    /// Return true when this module needs to be re-displayed
    fn ui_dirty(&self) -> bool;
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
    antialiasing: bool,
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
            antialiasing: true,
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

    fn get_name() -> String {
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
            let delta = self.get_freq_in_hz(input_buf, i) / (self.sample_rate as f64);
            self.sine[i] = (self.pos * PI * 2.0).sin() as ControlVoltage;

            self.square[i] = if self.pos < 0.5 { -1.0 } else { 1.0 }
                - if self.antialiasing {
                    (Self::poly_blep(self.pos, delta)
                        - Self::poly_blep((self.pos + 0.5) % 1.0, delta)) as f32
                } else {
                    0.0
                };

            self.saw[i] = (self.pos as ControlVoltage * 2.0 - 1.0)
                - if self.antialiasing {
                    Self::poly_blep(self.pos, delta) as f32
                } else {
                    0.0
                };

            self.pos = self.pos + delta;
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
        ui.checkbox(&mut self.antialiasing, "Antialiasing");
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

    fn get_name() -> String {
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

    fn ui_dirty(&self) -> bool {
        false
    }
}

struct TransitionDetector {
    last: bool,
}

impl TransitionDetector {
    fn new() -> Self {
        Self { last: true }
    }

    fn is_above_threshold(val: &ControlVoltage) -> bool {
        *val > 0.0
    }

    /// Returns true if current val is above 0.0 but last
    /// val was 0.0 or below.
    fn is_transition(&mut self, val: &ControlVoltage) -> bool {
        let above_threshold = Self::is_above_threshold(val);
        let is_transition = above_threshold && !self.last;
        self.last = above_threshold;
        is_transition
    }
}

struct GridSequencerModule {
    id: String,
    cv_out: Box<[ControlVoltage]>,
    gate_out: Box<[ControlVoltage]>,
    sequence: Vec<Option<u16>>,
    octaves: u8,
    steps_per_octave: u16,
    step_in: Option<(SharedSynthModule, u8)>,
    current_step: u16,
    transition_detector: TransitionDetector,
    last: ControlVoltage,
    ui_dirty: bool,
}

impl GridSequencerModule {
    fn new(audio_config: &AudioConfig) -> Self {
        GridSequencerModule {
            id: uuid::Uuid::new_v4().into(),
            cv_out: (0..audio_config.buffer_size).map(|_| 0.0).collect(),
            gate_out: (0..audio_config.buffer_size).map(|_| 0.0).collect(),
            octaves: 2,
            sequence: vec![None; 64],
            step_in: None,
            current_step: 0,
            steps_per_octave: 12,
            transition_detector: TransitionDetector::new(),
            last: 0.0,
            ui_dirty: false,
        }
    }

    fn get_name() -> String {
        "Grid Sequencer".to_string()
    }
}

impl SynthModule for GridSequencerModule {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let num_rows = self.octaves as u16 * self.steps_per_octave;
        egui::Grid::new("grid::".to_string() + &self.get_id())
            .min_col_width(0.0)
            .max_col_width(7.0)
            .min_row_height(0.0)
            .spacing([1.0, 1.0])
            .show(ui, |ui| {
                for row in (0..num_rows).into_iter().rev() {
                    for col in 0..self.sequence.len() {
                        let (id, rect) = ui.allocate_space([7.0, 7.0].into());
                        let mut color = egui::Color32::LIGHT_GRAY;
                        if col % 4 == 0 {
                            color = egui::Color32::GRAY;
                        }
                        if row % self.steps_per_octave == 0 {
                            color = egui::Color32::YELLOW;
                        }
                        if usize::from(self.current_step) == col {
                            color = egui::Color32::RED;
                        }
                        if self.sequence[usize::from(col)] == Some(row) {
                            color = egui::Color32::BLACK;
                        }
                        ui.painter().rect_filled(rect, 1.0, color);
                        if ui.interact(rect, id, egui::Sense::click()).clicked() {
                            if self.sequence[usize::from(col)] == Some(row) {
                                self.sequence[usize::from(col)] = None;
                            } else {
                                self.sequence[usize::from(col)] = Some(row);
                            }
                        }
                    }
                    ui.end_row();
                }
            });
        self.ui_dirty = false;
    }

    fn calc(&mut self) {
        let mut step_in_buf: Option<&[ControlVoltage]> = None;
        let step_in_module;
        if let Some((input, port)) = &self.step_in {
            step_in_module = input.read().unwrap();
            step_in_buf = Some(step_in_module.get_output(*port).unwrap());
        }
        for idx in 0..self.cv_out.len() {
            let step_in = match step_in_buf {
                Some(v) => &v[idx],
                None => &0.0,
            };
            if self.transition_detector.is_transition(step_in) {
                self.current_step += 1;
                self.ui_dirty = true;
            }
            let mut current_step: usize = self.current_step.into();
            if current_step >= self.sequence.len() {
                self.current_step = 0;
                current_step = 0;
            }
            (self.cv_out[idx], self.gate_out[idx]) = match self.sequence[current_step] {
                Some(val) => (
                    val as ControlVoltage * (1.0 / self.steps_per_octave as ControlVoltage),
                    0.0,
                ),
                None => (self.last, 0.0),
            };
            self.last = self.cv_out[idx];
        }
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn get_name(&self) -> String {
        Self::get_name()
    }

    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()> {
        match input_idx {
            0 => Ok(self.step_in.clone()),
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
                self.step_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_inputs(&self) -> Vec<Option<(SharedSynthModule, u8)>> {
        (0..self.get_num_inputs())
            .map(|idx| self.get_input(idx).unwrap())
            .collect()
    }

    fn get_output(&self, output_idx: u8) -> Result<&[ControlVoltage], ()> {
        match output_idx {
            0 => Ok(&self.cv_out),
            1 => Ok(&self.gate_out),
            _ => Err(()),
        }
    }

    fn get_num_inputs(&self) -> u8 {
        1
    }

    fn get_num_outputs(&self) -> u8 {
        2
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.step_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn ui_dirty(&self) -> bool {
        self.ui_dirty
    }
}

pub fn get_catalog() -> Vec<(String, Box<dyn Fn(&AudioConfig) -> SharedSynthModule>)> {
    vec![
        (
            OscillatorModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(OscillatorModule::new(audio_config)))),
        ),
        (
            GridSequencerModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(GridSequencerModule::new(audio_config)))),
        ),
    ]
}
