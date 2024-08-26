mod adsr;
mod mixer;
mod oscillator;
pub mod output;
mod sequencer;
mod vca;

use by_address::ByAddress;
use egui;
use itertools::Itertools;
use std::any::Any;
use std::collections::{HashMap, HashSet};
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
    // topological sort of a graph with cycles -- first we need to break cycles
    let mut edges: HashMap<ByAddress<SharedSynthModule>, Vec<ByAddress<SharedSynthModule>>> =
        HashMap::new();
    let mut visited: HashSet<ByAddress<SharedSynthModule>> = HashSet::new();
    let mut to_search = all_modules.clone();
    loop {
        // depth first search to break cycles
        let module = to_search.pop();
        if module.is_none() {
            break;
        }
        let module = module.unwrap();
        if !visited.insert(ByAddress(module.clone())) {
            continue;
        }
        let unlocked = module.read().unwrap();
        edges.insert(
            // store edges, but only to nodes which have not been visited
            ByAddress(module.clone()),
            get_inputs(&*unlocked)
                .into_iter()
                .filter(|i| i.is_some())
                .map(|i| i.unwrap().0)
                .filter(|m| visited.get(&ByAddress(m.clone())).is_none()) // don't create edges to
                // nodes visited
                .map(|m| {
                    to_search.push(m.clone());
                    ByAddress(m)
                })
                .collect(),
        );
    }
    let to_search = all_modules.clone();
    plan.clear();
    visited.clear();
    loop {
        // find leaves first, then search for nodes for which children have already been visited
        if let Some(node) = to_search
            .iter()
            .map(|m| m.clone())
            .filter(|m| {
                edges
                    .get(&ByAddress(m.clone()))
                    .unwrap()
                    .into_iter()
                    .filter(|d| !visited.contains(d))
                    .collect::<Vec<_>>()
                    .len()
                    == 0
            })
            .filter(|m| !visited.contains(&ByAddress(m.clone())))
            .next()
        {
            visited.insert(ByAddress(node.clone()));
            plan.push(node);
        } else {
            break;
        }
    }
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
