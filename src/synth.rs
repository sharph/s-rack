mod adsr;
mod mixer;
mod oscillator;
pub mod output;
mod sequencer;
mod vca;

use egui;
use std::any::Any;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

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

pub fn get_catalog() -> Vec<(String, Box<dyn Fn(&AudioConfig) -> SharedSynthModule>)> {
    vec![
        (
            oscillator::OscillatorModule::get_name(),
            Box::new(|audio_config| {
                Arc::new(RwLock::new(oscillator::OscillatorModule::new(audio_config)))
            }),
        ),
        (
            sequencer::GridSequencerModule::get_name(),
            Box::new(|audio_config| {
                Arc::new(RwLock::new(sequencer::GridSequencerModule::new(
                    audio_config,
                )))
            }),
        ),
        (
            adsr::ADSRModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(adsr::ADSRModule::new(audio_config)))),
        ),
        (
            vca::VCAModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(vca::VCAModule::new(audio_config)))),
        ),
        (
            mixer::MonoMixerModule::get_name(),
            Box::new(|audio_config| {
                Arc::new(RwLock::new(mixer::MonoMixerModule::new(audio_config)))
            }),
        ),
    ]
}
