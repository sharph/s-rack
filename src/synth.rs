mod adsr;
mod filter;
mod mixer;
mod oscillator;
pub mod output;
mod sample;
mod sequencer;
mod vca;

use by_address::ByAddress;
use egui;
use serde::{Deserialize, Serialize};
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

#[derive(Clone, Serialize, Deserialize)]
pub struct AudioBuffer(Option<Arc<RwLock<Box<[ControlVoltage]>>>>);

impl AudioBuffer {
    fn new(size: Option<usize>) -> Self {
        AudioBuffer(size.map(|s| Arc::new(RwLock::new(vec![0.0; s].into_boxed_slice()))))
    }

    fn resize(&mut self, size: usize) {
        if self.0.is_some() {
            let locked = self.0.as_ref().unwrap().read().unwrap();
            if locked.len() == size {
                return;
            }
            drop(locked);
            self.0 = Some(Arc::new(RwLock::new(vec![0.0; size].into_boxed_slice())));
        }
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

    pub fn deep_clone(&self) -> Self {
        match self.get() {
            Some(inner) => Self(Some(Arc::new(RwLock::new(inner.clone())))),
            None => Self(None),
        }
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

fn is_loop(
    module: &SharedSynthModule,
    edges: &HashMap<ByAddress<SharedSynthModule>, Vec<ByAddress<SharedSynthModule>>>,
) -> Option<ByAddress<SharedSynthModule>> {
    let mut to_search: Vec<ByAddress<SharedSynthModule>> = vec![ByAddress(module.clone())];
    let mut to_add: Vec<ByAddress<SharedSynthModule>> = vec![];
    let mut visited: HashSet<ByAddress<SharedSynthModule>> = HashSet::new();
    while let Some(current_module) = to_search.iter().filter(|m| visited.get(m).is_none()).next() {
        visited.insert(current_module.clone());
        for dependency in edges.get(&current_module.clone()).unwrap() {
            if dependency.clone() == ByAddress(module.clone()) {
                println!("cycle detected");
                return Some(current_module.clone());
            }
            to_add.push(dependency.clone());
        }
        to_search.append(&mut to_add);
    }
    None
}

pub fn plan_execution(
    output: SharedSynthModule,
    all_modules: &Vec<SharedSynthModule>,
    plan: &mut Vec<SharedSynthModule>,
) -> () {
    // topological sort of a graph with cycles -- first we need to break cycles
    let mut edges: HashMap<ByAddress<SharedSynthModule>, Vec<ByAddress<SharedSynthModule>>> =
        HashMap::new(); // K: sink, V: sources
    let mut visited: HashSet<ByAddress<SharedSynthModule>> = HashSet::new();
    let mut to_search = all_modules.clone();
    to_search.push(output.clone());
    loop {
        // create all edges
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
            // store edges
            ByAddress(module.clone()),
            get_inputs(&*unlocked)
                .into_iter()
                .filter(|i| i.is_some())
                .map(|i| i.unwrap().0)
                .map(|m| {
                    to_search.push(m.clone());
                    ByAddress(m)
                })
                .collect(),
        );
    }
    let mut to_search = all_modules.clone();
    plan.clear();
    visited.clear();
    to_search.push(output.clone());
    // remove cycles
    while let Some(module) = to_search.pop() {
        if !visited.insert(ByAddress(module.clone())) {
            continue;
        }
        for dependency in edges.get(&ByAddress(module.clone())).unwrap().iter() {
            to_search.push((**dependency).clone());
        }
        while let Some(from) = is_loop(&module, &edges) {
            let unlocked = module.read().unwrap();
            println!("{}", unlocked.get_name());
            while let Some(idx) = edges
                .get(&from)
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, m)| ByAddress(module.clone()) == **m)
                .map(|(idx, _)| idx)
                .next()
            {
                let dependencies = edges.get_mut(&from).unwrap();
                dependencies.remove(idx);
            }
        }
    }
    visited.clear();
    let to_search = all_modules.clone();
    // find leaves first, then search for nodes for which children have already been visited
    // find next node with no dependencies that haven't been visited
    while let Some(node) = to_search
        .iter()
        .map(|m| m.clone())
        .filter(|m| !visited.contains(&ByAddress(m.clone())))
        .filter(|m| {
            !edges
                .get(&ByAddress(m.clone()))
                .unwrap()
                .iter()
                .any(|d| !visited.contains(d)) // any will return true when there are unvisited
                                               // dependencies
        })
        .next()
    {
        visited.insert(ByAddress(node.clone()));
        plan.push(node);
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

    fn disconnect_inputs(&mut self) {
        for idx in 0..self.get_num_inputs() {
            self.disconnect_input(idx);
        }
    }

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
    /// Change the audio configuration. Used after deserialize.
    fn set_audio_config(&mut self, audio_config: &AudioConfig);
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

#[derive(Serialize, Deserialize, Clone)]
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

#[derive(Serialize, Deserialize)]
pub enum SynthModuleType {
    OutputModule(output::OutputModule),
    OscillatorModule(oscillator::OscillatorModule),
    GridSequencerModule(sequencer::GridSequencerModule),
    ADSRModule(adsr::ADSRModule),
    VCAModule(vca::VCAModule),
    MoogFilterModule(filter::MoogFilterModule),
    MonoMixerModule(mixer::MonoMixerModule),
    SampleModule(sample::SampleModule),
}

fn prep_for_serialization<T: SynthModule + Clone>(module: &T) -> T {
    let mut module = module.clone();
    module.disconnect_inputs();
    module
}

/// Unpack a module from an enum which came from deserialization
pub fn enum_to_sharedsynthmodule(synthmoduleenum: SynthModuleType) -> SharedSynthModule {
    match synthmoduleenum {
        SynthModuleType::OutputModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::OscillatorModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::GridSequencerModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::ADSRModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::VCAModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::MoogFilterModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::MonoMixerModule(m) => Arc::new(RwLock::new(m)),
        SynthModuleType::SampleModule(m) => Arc::new(RwLock::new(m)),
    }
}

/// Prepare a module for serialization by encapsulating it in an enum
pub fn any_module_to_enum(module: Box<&dyn SynthModule>) -> Result<SynthModuleType, ()> {
    let module = module.as_any();
    if let Some(module) = module.downcast_ref::<output::OutputModule>() {
        return Ok(SynthModuleType::OutputModule(prep_for_serialization(
            &module,
        )));
    }
    if let Some(module) = module.downcast_ref::<oscillator::OscillatorModule>() {
        return Ok(SynthModuleType::OscillatorModule(prep_for_serialization(
            &module,
        )));
    }
    if let Some(module) = module.downcast_ref::<sequencer::GridSequencerModule>() {
        return Ok(SynthModuleType::GridSequencerModule(
            prep_for_serialization(&module),
        ));
    }
    if let Some(module) = module.downcast_ref::<adsr::ADSRModule>() {
        return Ok(SynthModuleType::ADSRModule(prep_for_serialization(&module)));
    }
    if let Some(module) = module.downcast_ref::<vca::VCAModule>() {
        return Ok(SynthModuleType::VCAModule(prep_for_serialization(&module)));
    }
    if let Some(module) = module.downcast_ref::<filter::MoogFilterModule>() {
        return Ok(SynthModuleType::MoogFilterModule(prep_for_serialization(
            &module,
        )));
    }
    if let Some(module) = module.downcast_ref::<mixer::MonoMixerModule>() {
        return Ok(SynthModuleType::MonoMixerModule(prep_for_serialization(
            &module,
        )));
    }
    if let Some(module) = module.downcast_ref::<sample::SampleModule>() {
        return Ok(SynthModuleType::SampleModule(prep_for_serialization(
            &module,
        )));
    }
    Err(())
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
            filter::MoogFilterModule::get_name(),
            Box::new(|audio_config| {
                Arc::new(RwLock::new(filter::MoogFilterModule::new(audio_config)))
            }),
        ),
        (
            mixer::MonoMixerModule::get_name(),
            Box::new(|audio_config| {
                Arc::new(RwLock::new(mixer::MonoMixerModule::new(audio_config)))
            }),
        ),
        (
            sample::SampleModule::get_name(),
            Box::new(|audio_config| Arc::new(RwLock::new(sample::SampleModule::new(audio_config)))),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::seq::SliceRandom;
    use std::collections::HashMap;

    fn connect(src: SharedSynthModule, sink: SharedSynthModule) {
        let mut unlocked_sink = sink.write().unwrap();
        let unconnected_idx = get_inputs(&*unlocked_sink)
            .iter()
            .enumerate()
            .filter(|(_idx, input)| input.is_none())
            .map(|(idx, _)| idx)
            .next()
            .unwrap();
        unlocked_sink
            .set_input(unconnected_idx as u8, src, 0)
            .unwrap();
    }

    #[test]
    fn topological_sort() {
        //     0 -> 1 -> 2 -> 3 -> o
        //      \----> 4 -----^
        //        5<->6^
        let ac = AudioConfig {
            buffer_size: 64,
            sample_rate: 44100,
            channels: 2,
        };
        let mut rng = rand::thread_rng();
        let create_mod = || Arc::new(RwLock::new(mixer::MonoMixerModule::new(&ac)));
        let out = Arc::new(RwLock::new(output::OutputModule::new(&ac)));
        let modules: Vec<SharedSynthModule> =
            (0..7).map(|_| create_mod() as SharedSynthModule).collect();
        connect(modules[0].clone(), modules[1].clone());
        connect(modules[1].clone(), modules[2].clone());
        connect(modules[2].clone(), modules[3].clone());
        connect(modules[3].clone(), out.clone());
        connect(modules[0].clone(), modules[4].clone());
        connect(modules[4].clone(), modules[3].clone());
        connect(modules[6].clone(), modules[4].clone());
        connect(modules[5].clone(), modules[6].clone());
        connect(modules[6].clone(), modules[5].clone());
        for _ in 0..1000 {
            let mut indexes: HashMap<ByAddress<SharedSynthModule>, usize> = HashMap::new();
            let mut list: Vec<SharedSynthModule> = Vec::new();
            let mut plan: Vec<SharedSynthModule> = Vec::new();
            list.append(&mut modules.clone());
            list.push(out.clone());
            list.shuffle(&mut rng);
            plan_execution(out.clone(), &list, &mut plan);
            println!("---");
            for (idx, module) in plan.iter().enumerate() {
                indexes.insert(ByAddress(module.clone()), idx);
            }
            for (idx, mapping) in modules
                .iter()
                .map(|m| indexes.get(&ByAddress(m.clone())).unwrap())
                .enumerate()
            {
                println!("{} -> {}", idx, mapping);
            }
            println!("o -> {}", indexes.get(&ByAddress(out.clone())).unwrap());
            assert!(
                indexes.get(&ByAddress(modules[0].clone()))
                    < indexes.get(&ByAddress(modules[1].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[1].clone()))
                    < indexes.get(&ByAddress(modules[2].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[2].clone()))
                    < indexes.get(&ByAddress(modules[3].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[3].clone())) < indexes.get(&ByAddress(out.clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[0].clone()))
                    < indexes.get(&ByAddress(modules[4].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[4].clone()))
                    < indexes.get(&ByAddress(modules[3].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[6].clone()))
                    < indexes.get(&ByAddress(modules[4].clone()))
            );
            assert!(
                indexes.get(&ByAddress(modules[5].clone()))
                    < indexes.get(&ByAddress(modules[6].clone()))
            );
        }
    }
}
