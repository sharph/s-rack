use egui;
use itertools::Itertools;
use std::any::Any;
use std::f64::consts::PI;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use uuid;

#[derive(Clone)]
pub struct AudioConfig {
    pub sample_rate: u16,
    pub buffer_size: usize,
    pub channels: u8,
}

#[derive(Clone)]
pub struct AudioBuffer(Option<Arc<RwLock<Box<[ControlVoltage]>>>>);

impl AudioBuffer {
    fn new(size: Option<usize>) -> Self {
        AudioBuffer(size.map(|s| Arc::new(RwLock::new(vec![0.0; s].into_boxed_slice()))))
    }

    fn get(&self) -> Option<RwLockReadGuard<Box<[ControlVoltage]>>> {
        if let Some(arc) = &self.0 {
            return Some(arc.read().unwrap());
        }
        None
    }

    fn get_mut(&self) -> Option<RwLockWriteGuard<Box<[ControlVoltage]>>> {
        if let Some(arc) = &self.0 {
            return Some(arc.write().unwrap());
        }
        None
    }

    pub fn with_read<T, F: FnOnce(Option<&[ControlVoltage]>) -> T>(&self, f: F) -> T {
        if let Some(_) = &self.0 {
            return f(Some(self.get().unwrap().as_ref()));
        }
        f(None)
    }

    pub fn with_write<T, F: FnOnce(Option<&mut [ControlVoltage]>) -> T>(&self, f: F) -> T {
        if let Some(_) = &self.0 {
            let mut buf = self.get_mut();
            let deref = buf.as_deref_mut().unwrap();
            return f(Some(deref));
        }
        f(None)
    }

    pub fn with_read_many<T, F: FnOnce(Vec<Option<&[ControlVoltage]>>) -> T>(
        cv: Vec<AudioBuffer>,
        f: F,
    ) -> T {
        let unlocked: Vec<_> = cv.iter().map(|ab| ab.get()).collect();
        let derefed: Vec<_> = unlocked.iter().map(|ab| ab.as_ref()).collect();
        f(derefed.iter().map(|ab| ab.map(|ab| ab.as_ref())).collect())
    }

    pub fn with_write_many<T, F: FnOnce(Vec<Option<&mut [ControlVoltage]>>) -> T>(
        cv: Vec<AudioBuffer>,
        f: F,
    ) -> T {
        let mut unlocked: Vec<_> = cv.iter().map(|ab| ab.get_mut()).collect();
        f(unlocked
            .iter_mut()
            .map(|ab| ab.as_deref_mut().map(|ab| ab.deref_mut()))
            .collect())
    }
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
            // is there any module in our list we need to explore.
            for input in get_inputs(&*to_search.read().unwrap()) {
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

pub fn get_inputs(module: &dyn SynthModule) -> Vec<Option<(SharedSynthModule, u8)>> {
    (0..module.get_num_inputs())
        .map(|idx| module.get_input(idx).unwrap())
        .collect()
}

type ControlVoltage = f32;

pub trait SynthModule: Any {
    fn get_id(&self) -> String;
    fn get_name(&self) -> String;
    fn calc(&mut self);
    fn get_num_inputs(&self) -> u8;
    fn get_num_outputs(&self) -> u8;
    fn get_input(&self, input_idx: u8) -> Result<Option<(SharedSynthModule, u8)>, ()>;
    /// Return a string for the input, which may be used as a tooltip, for example.
    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()>;
    /// Return a string for the output, which may be used as a tooltip, for example.
    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()>;
    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()>;
    fn set_input(
        &mut self,
        input_idx: u8,
        src_module: SharedSynthModule,
        src_port: u8,
    ) -> Result<(), ()>;
    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()>;

    #[inline]
    fn resolve_input<'a>(&'a self, input_idx: u8) -> Result<AudioBuffer, ()> {
        match self.get_input(input_idx)? {
            Some((src_module, src_port)) => Ok(src_module.read().unwrap().get_output(src_port)?),
            None => Ok(AudioBuffer::new(None)),
        }
    }
    fn ui(&mut self, _ui: &mut egui::Ui) {}
    /// Return true when this module needs to be re-displayed
    fn ui_dirty(&self) -> bool {
        false
    }
    fn as_any(&self) -> &dyn Any;
}
impl PartialEq for dyn SynthModule {
    fn eq(&self, other: &Self) -> bool {
        self.get_id() == other.get_id()
    }
}

pub type SharedSynthModule = Arc<RwLock<dyn SynthModule + Send + Sync>>;

pub fn shared_are_eq(a: &SharedSynthModule, b: &SharedSynthModule) -> bool {
    Arc::ptr_eq(a, b)
}

pub struct OscillatorModule {
    id: String,
    pub val: ControlVoltage,
    input: Option<(SharedSynthModule, u8)>,
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
    cv_out: AudioBuffer,
    gate_out: AudioBuffer,
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
            cv_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            gate_out: AudioBuffer::new(Some(audio_config.buffer_size)),
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

const GRID_CELL_SIZE: f32 = 7.0;
const GRID_CELL_PADDING: f32 = 1.0;

impl SynthModule for GridSequencerModule {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Octaves: ");
                ui.scope(|ui| {
                    if self.octaves <= 1 {
                        ui.disable();
                    }
                    if ui.button("-").clicked() {
                        self.octaves -= 1;
                    }
                });
                ui.label(self.octaves.to_string());
                ui.scope(|ui| {
                    if self.octaves >= 4 {
                        ui.disable();
                    }
                    if ui.button("-").clicked() && self.octaves < 4 {
                        self.octaves += 1;
                    }
                });
                ui.label("Steps: ");
                ui.scope(|ui| {
                    if self.sequence.len() % 2 == 1 && self.sequence.len() <= 2 {
                        ui.disable()
                    }
                    if ui.button("/2").clicked() {
                        self.sequence.resize(self.sequence.len() / 2, None);
                    }
                });
                ui.scope(|ui| {
                    if self.sequence.len() <= 2 {
                        ui.disable()
                    }
                    if ui.button("-").clicked() {
                        self.sequence.resize(self.sequence.len() - 1, None);
                    }
                });
                ui.label(self.sequence.len().to_string());
                ui.scope(|ui| {
                    if self.sequence.len() >= 64 {
                        ui.disable()
                    }
                    if ui.button("+").clicked() {
                        self.sequence.resize(self.sequence.len() + 1, None);
                    }
                });
                ui.scope(|ui| {
                    if self.sequence.len() > 32 {
                        ui.disable()
                    }
                    if ui.button("x2").clicked() {
                        self.sequence.resize(self.sequence.len() * 2, None);
                    }
                });
            });
        });
        let num_rows = self.octaves as u16 * self.steps_per_octave;
        let (id, space_rect) = ui.allocate_space(
            [
                self.sequence.len() as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING),
                num_rows as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING),
            ]
            .into(),
        );
        let clicked = ui.interact(space_rect, id, egui::Sense::click()).clicked();
        for row in (0..num_rows).into_iter().rev() {
            for col in 0..self.sequence.len() {
                let top_left = egui::Pos2::new(
                    space_rect.min.x + (col as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING)),
                    space_rect.min.y
                        + ((num_rows - 1 - row) as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING)),
                );
                let rect = egui::Rect::from_two_pos(
                    top_left,
                    egui::Pos2::new(top_left.x + GRID_CELL_SIZE, top_left.y + GRID_CELL_SIZE),
                );
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
                if clicked && ui.rect_contains_pointer(rect) {
                    if self.sequence[usize::from(col)] == Some(row) {
                        self.sequence[usize::from(col)] = None;
                    } else {
                        self.sequence[usize::from(col)] = Some(row);
                    }
                }
            }
        }
        self.ui_dirty = false;
    }

    fn calc(&mut self) {
        self.resolve_input(0).unwrap().with_read(|step_in_buf| {
            AudioBuffer::with_write_many(
                vec![self.cv_out.clone(), self.gate_out.clone()],
                |bufs| {
                    let (cv_out, gate_out) = bufs
                        .into_iter()
                        .map(|b| b.unwrap())
                        .collect_tuple()
                        .unwrap();
                    for idx in 0..cv_out.len() {
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
                        (cv_out[idx], gate_out[idx]) = match self.sequence[current_step] {
                            Some(val) => (
                                val as ControlVoltage
                                    * (1.0 / self.steps_per_octave as ControlVoltage),
                                *step_in,
                            ),
                            None => (self.last, 0.0),
                        };
                        self.last = cv_out[idx];
                    }
                },
            );
        });
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

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Step".to_string())),
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

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.cv_out.clone()),
            1 => Ok(self.gate_out.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0 => Ok(Some("CV".to_string())),
            1 => Ok(Some("Gate".to_string())),
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

struct ADSRModule {
    id: String,
    a_sec: f32,
    d_sec: f32,
    s_val: ControlVoltage,
    r_sec: f32,
    // 0.0 < p < 1.0 attack
    // 1.0 < p < 2.0 decay
    // 2.0 < p < 3.0 release
    phase: f32,
    mode: ADSRMode,
    r_val: ControlVoltage,
    sample_rate: f32,
    gate_in: Option<(SharedSynthModule, u8)>,
    transition_detector: TransitionDetector,
    output_buffer: AudioBuffer,
    ui_dirty: bool,
}

enum ADSRMode {
    Attack,
    Decay,
    Sustain,
    Release,
    None,
}

impl ADSRModule {
    fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            a_sec: 0.0,
            d_sec: 0.5,
            s_val: 0.25,
            r_sec: 0.5,
            phase: 0.0,
            mode: ADSRMode::None,
            r_val: 0.0,
            sample_rate: audio_config.sample_rate as f32,
            gate_in: None,
            transition_detector: TransitionDetector::new(),
            output_buffer: AudioBuffer::new(Some(audio_config.buffer_size)),
            ui_dirty: false,
        }
    }

    fn get_name() -> String {
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
                    }
                }
            });
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.add(
                    egui::Slider::new(&mut self.a_sec, 0.0..=10.0)
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
                    egui::Slider::new(&mut self.d_sec, 0.0..=10.0)
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

struct VCAModule {
    id: String,
    audio_in: Option<(SharedSynthModule, u8)>,
    cv_in: Option<(SharedSynthModule, u8)>,
    buf: AudioBuffer,
    negative: bool,
}

impl VCAModule {
    fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            audio_in: None,
            cv_in: None,
            buf: AudioBuffer::new(Some(audio_config.buffer_size)),
            negative: false,
        }
    }

    fn get_name() -> String {
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
        (
            ADSRModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(ADSRModule::new(audio_config)))),
        ),
        (
            VCAModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(VCAModule::new(audio_config)))),
        ),
    ]
}
