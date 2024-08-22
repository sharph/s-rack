use crate::synth;
use egui;
use std::sync::{Arc, Mutex, RwLock};

const SYNTH_HANDLE_SIZE: f32 = 10.0;
const SYNTH_HANDLE_PADDING: f32 = 2.0;

enum SynthModulePort {
    Input(synth::SharedSynthModule, u8),
    Output(synth::SharedSynthModule, u8),
}

struct SynthModuleHandle {
    tooltip: Option<String>,
}

impl SynthModuleHandle {
    fn new(tooltip: Option<String>) -> Self {
        Self { tooltip }
    }

    fn layout_in_ui(&mut self, ui: &mut egui::Ui) -> (egui::Id, egui::Rect, egui::Response) {
        let (id, rect) = ui.allocate_space([SYNTH_HANDLE_SIZE, SYNTH_HANDLE_SIZE].into());
        (
            id,
            rect,
            ui.interact(rect, id, egui::Sense::click_and_drag()),
        )
    }
}

impl egui::Widget for &mut SynthModuleHandle {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (_id, rect, mut response) = self.layout_in_ui(ui);
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, egui::Color32::RED);
        if self.tooltip.is_some() {
            response = response.on_hover_text_at_pointer(self.tooltip.clone().unwrap());
        }
        response
    }
}

pub struct SynthModuleWorkspaceImpl {
    transform: egui::emath::TSTransform,
    modules: Vec<synth::SharedSynthModule>,
    pub plan: Arc<Mutex<Vec<synth::SharedSynthModule>>>,
    pub output: Arc<Mutex<Option<synth::SharedSynthModule>>>,
    pub audio_config: Option<synth::AudioConfig>,
}

impl SynthModuleWorkspaceImpl {
    pub fn plan(&mut self) -> () {
        if let Ok(output) = self.find_output() {
            let mut output_ref = self.output.lock().unwrap();
            synth::plan_execution(
                output.clone(),
                &self.modules,
                &mut self.plan.lock().unwrap(),
            );
            *output_ref = Some(output);
        } else {
            self.plan.lock().unwrap().clear();
            let mut output_ref = self.output.lock().unwrap();
            *output_ref = None;
        }
    }

    fn find_output(&self) -> Result<synth::SharedSynthModule, ()> {
        for module in self.modules.iter() {
            if let Some(_) = module
                .read()
                .unwrap()
                .as_any()
                .downcast_ref::<synth::OutputModule>()
            {
                return Ok(module.clone());
            }
        }
        Err(())
    }
}

#[derive(Clone)]
pub struct SynthModuleWorkspace(Arc<RwLock<SynthModuleWorkspaceImpl>>);

impl SynthModuleWorkspace {
    pub fn new() -> Self {
        SynthModuleWorkspace(Arc::new(RwLock::new(SynthModuleWorkspaceImpl {
            transform: egui::emath::TSTransform::new([0.0, 0.0].into(), 1.0),
            modules: vec![],
            plan: Arc::new(Mutex::new(vec![])),
            output: Arc::new(Mutex::new(None)),
            audio_config: None,
        })))
    }

    pub fn value(&self) -> Arc<RwLock<SynthModuleWorkspaceImpl>> {
        self.0.clone()
    }

    pub fn set_audio_config(&self, audio_config: synth::AudioConfig) {
        {
            let mut workspace = self.0.write().unwrap();
            workspace.audio_config = Some(audio_config)
        }
    }

    pub fn add_module(&self, module: synth::SharedSynthModule) -> () {
        let mut writable = self.0.write().unwrap();
        writable.modules.push(module);
    }

    pub fn delete_module(&self, module: synth::SharedSynthModule) -> () {
        let mut workspace = self.0.write().unwrap();
        // first, disconnect any inputs connected to this module
        for module_ref in workspace.modules.iter() {
            let mut other_module = module_ref.write().unwrap();
            for input_idx in 0..other_module.get_num_inputs() {
                let sink_input = other_module.get_input(input_idx).unwrap();
                if sink_input.is_some() && synth::shared_are_eq(&module, &sink_input.unwrap().0) {
                    other_module.disconnect_input(input_idx).unwrap();
                }
            }
        }
        // then, delete from our internal store
        if let Some(idx) = workspace
            .modules
            .iter()
            .enumerate()
            .filter(|(_idx, other_module)| synth::shared_are_eq(&module, other_module))
            .map(|(idx, _)| idx)
            .next()
        {
            workspace.modules.remove(idx);
        }
        // replan
        workspace.plan();
    }

    pub fn get_output(&self) -> Arc<Mutex<Option<synth::SharedSynthModule>>> {
        let workspace = self.0.read().unwrap();
        workspace.output.clone()
    }

    pub fn get_plan(&self) -> Arc<Mutex<Vec<synth::SharedSynthModule>>> {
        let workspace = self.0.read().unwrap();
        workspace.plan.clone()
    }

    pub fn plan(&mut self) -> () {
        let mut workspace = self.0.write().unwrap();
        workspace.plan();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut workspace = self.0.write().unwrap();
        let mut dirty = false;
        let mut to_delete: Option<synth::SharedSynthModule> = None;

        let (id, rect) = ui.allocate_space(ui.available_size());
        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
        // Allow dragging the background as well.
        if response.dragged() {
            workspace.transform.translation += response.drag_delta();
        }

        // Plot-like reset
        if response.double_clicked() {
            workspace.transform = egui::emath::TSTransform::new([0.0, 0.0].into(), 1.0);
        }

        let transform =
            egui::emath::TSTransform::from_translation(ui.min_rect().left_top().to_vec2())
                * workspace.transform;

        if let Some(pointer) = ui.ctx().input(|i| i.pointer.hover_pos()) {
            if response.hovered() {
                let pointer_in_layer = transform.inverse() * pointer;
                let zoom_delta = ui.ctx().input(|i| i.zoom_delta());
                let pan_delta = ui.ctx().input(|i| i.smooth_scroll_delta);

                workspace.transform = workspace.transform
                    * egui::emath::TSTransform::from_translation(pointer_in_layer.to_vec2())
                    * egui::emath::TSTransform::from_scaling(zoom_delta)
                    * egui::emath::TSTransform::from_translation(-pointer_in_layer.to_vec2());

                workspace.transform =
                    egui::emath::TSTransform::from_translation(pan_delta) * workspace.transform;
            }
        }

        for module_ref in workspace.modules.iter() {
            let mut module = module_ref.write().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module", module.get_id()));
            let area = egui::Area::new(area_id)
                .constrain(false)
                .default_pos(egui::pos2(100.0, 100.0))
                .order(egui::Order::Middle)
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            for idx in 0..module.get_num_inputs() {
                                let response = ui.add(&mut SynthModuleHandle::new(
                                    module.get_input_label(idx).unwrap(),
                                ));
                                response.dnd_set_drag_payload(SynthModulePort::Input(
                                    module_ref.clone(),
                                    idx,
                                ));
                                if response.secondary_clicked() {
                                    module.disconnect_input(idx).unwrap();
                                    dirty = true;
                                }
                                if let Some(payload) =
                                    response.dnd_release_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Output(output_module, output_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        module
                                            .set_input(idx, output_module.clone(), *output_port)
                                            .unwrap();
                                        dirty = true;
                                    }
                                }
                            }
                        });
                        egui::Frame::default()
                            .rounding(egui::Rounding::same(2.0))
                            .inner_margin(egui::Margin::same(12.0))
                            .stroke(ui.ctx().style().visuals.window_stroke)
                            .fill(ui.style().visuals.panel_fill)
                            .show(ui, |ui| {
                                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::widgets::Label::new(module.get_name())
                                                .selectable(false),
                                        );
                                        if module.get_name() != "Output" {
                                            ui.menu_button("...", |ui| {
                                                if ui.button("Delete module").clicked() {
                                                    to_delete = Some(module_ref.clone());
                                                }
                                            });
                                        }
                                    });
                                    module.ui(ui);
                                });
                            });
                        ui.vertical(|ui| {
                            for idx in 0..module.get_num_outputs() {
                                let response = ui.add(&mut SynthModuleHandle::new(
                                    module.get_output_label(idx).unwrap(),
                                ));
                                response.dnd_set_drag_payload(SynthModulePort::Output(
                                    module_ref.clone(),
                                    idx,
                                ));
                                if let Some(payload) =
                                    response.dnd_release_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Input(input_module, input_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        let mut sink_module = input_module.write().unwrap();
                                        sink_module
                                            .set_input(*input_port, module_ref.clone(), idx)
                                            .unwrap();
                                        dirty = true;
                                    }
                                }
                            }
                        });
                    });
                });
            // load pivot from memory

            let layer_id = area.response.layer_id;
            ui.ctx().set_transform_layer(layer_id, transform);
            ui.ctx().set_sublayer(window_layer, layer_id);
        }

        for module in workspace.modules.iter() {
            let module = module.read().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module-connection", module.get_id()));
            let module_area_id = id.with(("module", module.get_id()));
            let area = egui::Area::new(area_id)
                .fixed_pos((0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    if let Some(state) = egui::AreaState::load(ui.ctx(), module_area_id) {
                        use egui::epaint::*;
                        if let (Some(pivot_pos), Some(_size)) = (state.pivot_pos, state.size) {
                            // draw connections
                            for (input_idx, input_module) in
                                synth::get_inputs(module).iter().enumerate()
                            {
                                if let Some((input_module, port)) = input_module {
                                    let input_module = input_module.read().unwrap();
                                    let input_module_area_id =
                                        id.with(("module", input_module.get_id()));
                                    if let Some(input_module_area_state) =
                                        egui::AreaState::load(ui.ctx(), input_module_area_id)
                                    {
                                        if let (Some(src_pivot_pos), Some(src_pivot_size)) = (
                                            input_module_area_state.pivot_pos,
                                            input_module_area_state.size,
                                        ) {
                                            ui.painter().line_segment(
                                                [
                                                    [
                                                        pivot_pos.x + (SYNTH_HANDLE_SIZE / 2.0),
                                                        pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (input_idx as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                    [
                                                        src_pivot_pos.x + src_pivot_size.x
                                                            - (SYNTH_HANDLE_SIZE / 2.0),
                                                        src_pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (*port as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                ]
                                                .into(),
                                                Stroke::new(1.0, Color32::RED),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            ui.ctx()
                .set_transform_layer(area.response.layer_id, transform);
            ui.ctx().set_sublayer(window_layer, area.response.layer_id);
        }
        if dirty {
            workspace.plan()
        };
        drop(workspace);
        if to_delete.is_some() {
            self.delete_module(to_delete.unwrap());
        }
    }
}
