use super::{
    AudioBuffer, AudioConfig, ControlVoltage, SharedSynthModule, SynthModule, TransitionDetector,
};
use egui::{self, output};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::any::Any;
use uuid;

const GRID_CELL_SIZE: f32 = 7.0;
const GRID_CELL_PADDING: f32 = 1.0;

#[derive(Serialize, Deserialize, Clone)]
pub struct GridSequencerModule {
    id: String,
    cv_out: AudioBuffer,
    gate_out: AudioBuffer,
    sync_out: AudioBuffer,
    sequence: Vec<Option<(u16, bool)>>,
    octaves: u8,
    steps_per_octave: u16,
    #[serde(skip)]
    step_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    sync_in: Option<(SharedSynthModule, u8)>,
    current_step: u16,
    transition_detector: TransitionDetector,
    sync_transition_detector: TransitionDetector,
    last: ControlVoltage,
    ui_dirty: bool,
}

impl GridSequencerModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        GridSequencerModule {
            id: uuid::Uuid::new_v4().into(),
            cv_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            gate_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            sync_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            octaves: 2,
            sequence: vec![None; 64],
            step_in: None,
            sync_in: None,
            current_step: 0,
            steps_per_octave: 12,
            transition_detector: TransitionDetector::new(),
            sync_transition_detector: TransitionDetector::new(),
            last: 0.0,
            ui_dirty: false,
        }
    }

    pub fn get_name() -> String {
        "Grid Sequencer".to_string()
    }
}

fn is_black_key(note: u16) -> bool {
    match note % 12 {
        1 => true,
        3 => true,
        6 => true,
        8 => true,
        10 => true,
        _ => false,
    }
}

impl SynthModule for GridSequencerModule {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        self.cv_out.resize(audio_config.buffer_size);
        self.gate_out.resize(audio_config.buffer_size);
        self.sync_out.resize(audio_config.buffer_size);
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
                    if ui.button("+").clicked() && self.octaves < 4 {
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
                if is_black_key(row) {
                    color = egui::Color32::DARK_GRAY;
                }
                if row % self.steps_per_octave == 0 {
                    color = egui::Color32::YELLOW;
                }
                if usize::from(self.current_step) == col {
                    color = egui::Color32::RED;
                }
                if self.sequence[usize::from(col)] == Some((row, true)) {
                    color = egui::Color32::BLACK;
                }
                if self.sequence[usize::from(col)] == Some((row, false)) {
                    color = egui::Color32::BLUE;
                }
                ui.painter().rect_filled(rect, 1.0, color);
                if clicked && ui.rect_contains_pointer(rect) {
                    if self.sequence[usize::from(col)] == Some((row, true)) {
                        self.sequence[usize::from(col)] = Some((row, false));
                    } else if self.sequence[usize::from(col)] == Some((row, false)) {
                        self.sequence[usize::from(col)] = None;
                    } else {
                        self.sequence[usize::from(col)] = Some((row, true));
                    }
                }
            }
        }
        self.ui_dirty = false;
    }

    fn calc(&mut self) {
        AudioBuffer::with_read_many(
            vec![
                self.resolve_input(0).unwrap(),
                self.resolve_input(1).unwrap(),
            ],
            |bufs| {
                let (step_in_buf, sync_in_buf) = bufs.into_iter().collect_tuple().unwrap();
                AudioBuffer::with_write_many(
                    vec![
                        self.cv_out.clone(),
                        self.gate_out.clone(),
                        self.sync_out.clone(),
                    ],
                    |bufs| {
                        let (cv_out, gate_out, sync_out) = bufs
                            .into_iter()
                            .map(|b| b.unwrap())
                            .collect_tuple()
                            .unwrap();
                        for idx in 0..cv_out.len() {
                            let step_in = match step_in_buf {
                                Some(v) => &v[idx],
                                None => &0.0,
                            };
                            let sync_in = match sync_in_buf {
                                Some(v) => &v[idx],
                                None => &0.0,
                            };
                            if self.transition_detector.is_transition(step_in) {
                                self.current_step += 1;
                                self.ui_dirty = true;
                            }
                            if self.sync_transition_detector.is_transition(sync_in) {
                                self.current_step = 0;
                            }
                            let mut current_step: usize = self.current_step.into();
                            if current_step >= self.sequence.len() {
                                self.current_step = 0;
                                current_step = 0;
                            }
                            (cv_out[idx], gate_out[idx]) = match self.sequence[current_step] {
                                Some((val, hold)) => (
                                    val as ControlVoltage
                                        * (1.0 / self.steps_per_octave as ControlVoltage),
                                    if hold { 1.0 } else { *step_in },
                                ),
                                None => (self.last, 0.0),
                            };
                            sync_out[idx] = if current_step == 0 { 1.0 } else { 0.0 };
                            self.last = cv_out[idx];
                        }
                    },
                );
            },
        );
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
            1 => Ok(self.sync_in.clone()),
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Step".to_string())),
            1 => Ok(Some("Sync".to_string())),
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
            1 => {
                self.sync_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        match output_idx {
            0 => Ok(self.cv_out.clone()),
            1 => Ok(self.gate_out.clone()),
            2 => Ok(self.sync_out.clone()),
            _ => Err(()),
        }
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        match output_idx {
            0 => Ok(Some("CV".to_string())),
            1 => Ok(Some("Gate".to_string())),
            2 => Ok(Some("Sync".to_string())),
            _ => Err(()),
        }
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_num_outputs(&self) -> u8 {
        3
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.step_in = None;
                Ok(())
            }
            1 => {
                self.sync_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn ui_dirty(&self) -> bool {
        self.ui_dirty
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PatternSequencerModule {
    id: String,
    gate_outs: Vec<AudioBuffer>,
    sync_out: AudioBuffer,
    sequence: Vec<Vec<Option<bool>>>,
    #[serde(skip)]
    step_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    sync_in: Option<(SharedSynthModule, u8)>,
    current_step: u16,
    transition_detector: TransitionDetector,
    sync_transition_detector: TransitionDetector,
    ui_dirty: bool,
}

impl PatternSequencerModule {
    pub fn new(audio_config: &AudioConfig) -> Self {
        Self {
            id: uuid::Uuid::new_v4().into(),
            gate_outs: (0..8)
                .map(|_| AudioBuffer::new(Some(audio_config.buffer_size)))
                .collect(),
            sync_out: AudioBuffer::new(Some(audio_config.buffer_size)),
            sequence: vec![vec![None; 64]; 8],
            step_in: None,
            sync_in: None,
            current_step: 0,
            transition_detector: TransitionDetector::new(),
            sync_transition_detector: TransitionDetector::new(),
            ui_dirty: false,
        }
    }

    pub fn get_name() -> String {
        "Pattern Sequencer".to_string()
    }
}

impl SynthModule for PatternSequencerModule {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn set_audio_config(&mut self, audio_config: &AudioConfig) {
        for out in self.gate_outs.iter_mut() {
            out.resize(audio_config.buffer_size)
        }
        self.sync_out.resize(audio_config.buffer_size);
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label("Steps: ");
                ui.scope(|ui| {
                    if self.sequence[0].len() % 2 == 1 && self.sequence[0].len() <= 2 {
                        ui.disable()
                    }
                    if ui.button("/2").clicked() {
                        for channel in self.sequence.iter_mut() {
                            channel.resize(channel.len() / 2, None);
                        }
                    }
                });
                ui.scope(|ui| {
                    if self.sequence[0].len() <= 2 {
                        ui.disable()
                    }
                    if ui.button("-").clicked() {
                        for channel in self.sequence.iter_mut() {
                            channel.resize(channel.len() - 1, None);
                        }
                    }
                });
                ui.label(self.sequence[0].len().to_string());
                ui.scope(|ui| {
                    if self.sequence[0].len() >= 64 {
                        ui.disable()
                    }
                    if ui.button("+").clicked() {
                        for channel in self.sequence.iter_mut() {
                            channel.resize(channel.len() + 1, None);
                        }
                    }
                });
                ui.scope(|ui| {
                    if self.sequence[0].len() > 32 {
                        ui.disable()
                    }
                    if ui.button("x2").clicked() {
                        for channel in self.sequence.iter_mut() {
                            channel.resize(channel.len() * 2, None);
                        }
                    }
                });
            });
        });
        let num_rows = self.sequence.len();
        let (id, space_rect) = ui.allocate_space(
            [
                self.sequence[0].len() as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING),
                num_rows as f32 * (GRID_CELL_SIZE + GRID_CELL_PADDING),
            ]
            .into(),
        );
        let clicked = ui.interact(space_rect, id, egui::Sense::click()).clicked();
        for (row, seq) in self.sequence.iter_mut().enumerate().rev() {
            for (col, val) in seq.iter_mut().enumerate() {
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
                if usize::from(self.current_step) == col {
                    color = egui::Color32::RED;
                }
                if *val == Some(true) {
                    color = egui::Color32::BLACK;
                }
                if *val == Some(false) {
                    color = egui::Color32::BLUE;
                }
                ui.painter().rect_filled(rect, 1.0, color);
                if clicked && ui.rect_contains_pointer(rect) {
                    if *val == Some(true) {
                        *val = Some(false);
                    } else if *val == Some(false) {
                        *val = None;
                    } else {
                        *val = Some(true);
                    }
                }
            }
        }
        self.ui_dirty = false;
    }

    fn calc(&mut self) {
        AudioBuffer::with_read_many(
            vec![
                self.resolve_input(0).unwrap(),
                self.resolve_input(1).unwrap(),
            ],
            |bufs| {
                let (step_in_buf, sync_in_buf) = bufs.into_iter().collect_tuple().unwrap();
                AudioBuffer::with_write_many(self.gate_outs.clone(), |outputs| {
                    self.sync_out.with_write(|sync_out| {
                        let sync_out = sync_out.unwrap();
                        let mut outputs: Vec<_> = outputs.into_iter().map(|o| o.unwrap()).collect();
                        for idx in 0..outputs[0].len() {
                            let step_in = match step_in_buf {
                                Some(v) => &v[idx],
                                None => &0.0,
                            };
                            let sync_in = match sync_in_buf {
                                Some(v) => &v[idx],
                                None => &0.0,
                            };
                            if self.transition_detector.is_transition(step_in) {
                                self.current_step += 1;
                                self.ui_dirty = true;
                            }
                            if self.sync_transition_detector.is_transition(sync_in) {
                                self.current_step = 0;
                            }
                            let mut current_step: usize = self.current_step.into();
                            if current_step >= self.sequence[0].len() {
                                self.current_step = 0;
                                current_step = 0;
                            }
                            for (seq, out) in self.sequence.iter().zip(outputs.iter_mut()) {
                                out[idx] = match seq[current_step] {
                                    Some(v) => {
                                        if v {
                                            1.0
                                        } else {
                                            *step_in
                                        }
                                    }
                                    None => 0.0,
                                };
                            }
                            sync_out[idx] = if current_step == 0 { 1.0 } else { 0.0 };
                        }
                    });
                });
            },
        );
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
            1 => Ok(self.sync_in.clone()),
            _ => Err(()),
        }
    }

    fn get_input_label(&self, input_idx: u8) -> Result<Option<String>, ()> {
        match input_idx {
            0 => Ok(Some("Step".to_string())),
            1 => Ok(Some("Sync".to_string())),
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
            1 => {
                self.sync_in = Some((src_module, src_port));
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn get_output(&self, output_idx: u8) -> Result<AudioBuffer, ()> {
        if output_idx == self.gate_outs.len() as u8 {
            return Ok(self.sync_out.clone());
        }
        if output_idx < self.gate_outs.len() as u8 {
            return Ok(self.gate_outs[output_idx as usize].clone());
        }
        Err(())
    }

    fn get_output_label(&self, output_idx: u8) -> Result<Option<String>, ()> {
        if output_idx == self.gate_outs.len() as u8 {
            return Ok(Some("Sync".to_string()));
        }
        if output_idx < self.gate_outs.len() as u8 {
            return Ok(Some(format!("{}", output_idx)));
        }
        Err(())
    }

    fn get_num_inputs(&self) -> u8 {
        2
    }

    fn get_num_outputs(&self) -> u8 {
        self.gate_outs.len() as u8 + 1
    }

    fn disconnect_input(&mut self, input_idx: u8) -> Result<(), ()> {
        match input_idx {
            0 => {
                self.step_in = None;
                Ok(())
            }
            1 => {
                self.sync_in = None;
                Ok(())
            }
            _ => Err(()),
        }
    }

    fn ui_dirty(&self) -> bool {
        self.ui_dirty
    }
}

// MIGRATIONS

#[derive(Serialize, Deserialize, Clone)]
pub struct GridSequencerModuleV0 {
    id: String,
    cv_out: AudioBuffer,
    gate_out: AudioBuffer,
    sync_out: AudioBuffer,
    sequence: Vec<Option<u16>>,
    octaves: u8,
    steps_per_octave: u16,
    #[serde(skip)]
    step_in: Option<(SharedSynthModule, u8)>,
    #[serde(skip)]
    sync_in: Option<(SharedSynthModule, u8)>,
    current_step: u16,
    transition_detector: TransitionDetector,
    sync_transition_detector: TransitionDetector,
    last: ControlVoltage,
    ui_dirty: bool,
}

impl From<GridSequencerModuleV0> for GridSequencerModule {
    fn from(item: GridSequencerModuleV0) -> Self {
        Self {
            id: item.id,
            cv_out: item.cv_out,
            gate_out: item.gate_out,
            sync_out: item.sync_out,
            sequence: item
                .sequence
                .into_iter()
                .map(|v| v.map(|v| (v, false)))
                .collect(),
            octaves: item.octaves,
            steps_per_octave: item.steps_per_octave,
            step_in: item.step_in,
            sync_in: item.sync_in,
            current_step: item.current_step,
            transition_detector: item.transition_detector,
            sync_transition_detector: item.sync_transition_detector,
            last: item.last,
            ui_dirty: item.ui_dirty,
        }
    }
}
