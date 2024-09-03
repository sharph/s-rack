use crate::synth::{self, SharedSynthModule};
use by_address::ByAddress;
use egui::{self, pos2};
use rfd::AsyncFileDialog;
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::future::Future;
use std::io::Cursor;
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
    modules_pos: HashMap<String, (f32, f32)>,
    loads: u16,
    pub plan: Arc<Mutex<Vec<synth::SharedSynthModule>>>,
    pub output: Arc<Mutex<Option<synth::SharedSynthModule>>>,
    pub audio_config: Option<synth::AudioConfig>,
}

impl SynthModuleWorkspaceImpl {
    pub fn plan(&mut self) -> () {
        println!("start plan");
        if let Ok(output) = self.find_output() {
            synth::plan_execution(
                output.clone(),
                &self.modules,
                &mut self.plan.lock().unwrap(),
            );
            let mut output_ref = self.output.lock().unwrap();
            *output_ref = Some(output);
        } else {
            self.plan.lock().unwrap().clear();
            let mut output_ref = self.output.lock().unwrap();
            *output_ref = None;
        }
        println!("end plan");
    }

    fn find_output(&self) -> Result<synth::SharedSynthModule, ()> {
        for module in self.modules.iter() {
            if let Some(_) = module
                .read()
                .unwrap()
                .as_any()
                .downcast_ref::<synth::output::OutputModule>()
            {
                return Ok(module.clone());
            }
        }
        Err(())
    }

    fn serialize(&self, ctx: egui::Context, id: &egui::Id) -> Vec<u8> {
        let mut container = FileFormat::default();
        let mut buf: Vec<u8> = Vec::new();
        container.capture_modules(&self.modules);
        container.capture_connections(&self.modules);
        container.capture_pos(&self.modules, |module_id| {
            let module_id = id.with(("module", module_id, self.loads));
            if let Some(state) = egui::AreaState::load(&ctx.clone(), module_id) {
                if let Some(pos) = state.pivot_pos {
                    return Ok((pos.x, pos.y));
                }
            }
            Err(())
        });
        container.serialize(&mut Serializer::new(&mut buf)).unwrap();
        buf
    }

    fn deserialize(&mut self, buf: &Vec<u8>) -> Result<(), Box<dyn Error>> {
        // first clear state
        for module in self.modules.iter() {
            let mut locked = module.write().unwrap();
            locked.disconnect_inputs();
        }
        self.loads += 1;
        self.modules.clear();
        let reader = Cursor::new(buf);
        let mut container = FileFormat::deserialize(&mut Deserializer::new(reader))?;
        self.modules_pos.clear();
        container.unpack_pos(|module_id, (x, y)| {
            self.modules_pos.insert(module_id, (x, y));
        });
        container.unpack_modules(&mut self.modules, &self.audio_config.as_ref().unwrap());
        container.unpack_connections(&mut self.modules)?;
        self.plan();
        Ok(())
    }
}

#[derive(Clone)]
pub struct SynthModuleWorkspace(Arc<RwLock<SynthModuleWorkspaceImpl>>, pub Option<egui::Id>);

impl SynthModuleWorkspace {
    pub fn new() -> Self {
        SynthModuleWorkspace(
            Arc::new(RwLock::new(SynthModuleWorkspaceImpl {
                transform: egui::emath::TSTransform::new([0.0, 0.0].into(), 1.0),
                modules_pos: HashMap::new(),
                modules: vec![],
                plan: Arc::new(Mutex::new(vec![])),
                output: Arc::new(Mutex::new(None)),
                audio_config: None,
                loads: 0,
            })),
            None,
        )
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

    pub fn open(&mut self) {
        let inner_workspace = self.0.clone();
        run_async(async move {
            let file_dialog = AsyncFileDialog::new()
                .add_filter("s-rack", &["srk"])
                .pick_file()
                .await;
            if let Some(file) = file_dialog {
                let data = file.read().await;
                let mut unlocked = inner_workspace.write().unwrap();
                let _ = unlocked.deserialize(&data);
            }
        });
    }

    pub fn save(&mut self, ctx: egui::Context, id: &egui::Id) {
        let inner_workspace = self.0.clone();
        let id = id.clone();
        run_async(async move {
            let file_dialog = AsyncFileDialog::new()
                .add_filter("s-rack", &["srk"])
                .set_file_name("Patch.srk")
                .save_file()
                .await;
            match file_dialog {
                Some(file) => {
                    let buf;
                    {
                        let locked = inner_workspace.read().unwrap();
                        buf = locked.serialize(ctx, &id);
                    }
                    let _ = file.write(&buf).await;
                }
                None => (),
            }
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let mut workspace = self.0.write().unwrap();
        let mut dirty = false;
        let mut to_delete: Option<synth::SharedSynthModule> = None;
        let mut output_to_disconnect: Option<(synth::SharedSynthModule, u8)> = None;

        let (id, rect) = ui.allocate_space(ui.available_size());
        self.1 = Some(id);
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

        let mut hover_wire: Option<(SharedSynthModule, u8, SharedSynthModule, u8)> = None;

        for module_ref in workspace.modules.iter() {
            let mut module = module_ref.write().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module", module.get_id(), workspace.loads));
            let area = egui::Area::new(area_id)
                .constrain(false)
                .default_pos(
                    workspace
                        .modules_pos
                        .get(&module.get_id())
                        .map(|(x, y)| pos2(*x, *y))
                        .unwrap_or(pos2(100.0, 100.0)),
                )
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
                                    response.dnd_hover_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Output(output_module, output_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        hover_wire = Some((
                                            output_module.clone(),
                                            *output_port,
                                            module_ref.clone(),
                                            idx,
                                        ));
                                    }
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
                                if response.secondary_clicked() {
                                    output_to_disconnect = Some((module_ref.clone(), idx));
                                }
                                if let Some(payload) =
                                    response.dnd_hover_payload::<SynthModulePort>()
                                {
                                    if let SynthModulePort::Input(input_module, input_port) =
                                        Arc::as_ref(&payload)
                                    {
                                        hover_wire = Some((
                                            module_ref.clone(),
                                            idx,
                                            input_module.clone(),
                                            *input_port,
                                        ));
                                    }
                                }
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

        for module_ref in workspace.modules.iter() {
            let module = module_ref.read().unwrap();
            let window_layer = ui.layer_id();
            // create area and draw module
            let area_id = id.with(("module-connection", module.get_id()));
            let module_area_id = id.with(("module", module.get_id(), workspace.loads));
            let area = egui::Area::new(area_id)
                .fixed_pos((0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.set_clip_rect(transform.inverse() * rect);
                    if let Some(state) = egui::AreaState::load(ui.ctx(), module_area_id) {
                        use egui::epaint::*;
                        if let (Some(pivot_pos), Some(size)) = (state.pivot_pos, state.size) {
                            // draw drag
                            if let (Some(payload), Some(pointer)) = (
                                response.dnd_hover_payload::<SynthModulePort>(),
                                ui.ctx().pointer_interact_pos(),
                            ) {
                                match payload.as_ref() {
                                    SynthModulePort::Input(payload_module, port_num) => {
                                        if ByAddress(payload_module.clone())
                                            == ByAddress(module_ref.clone())
                                        {
                                            ui.painter().line_segment(
                                                [
                                                    [
                                                        pivot_pos.x + (SYNTH_HANDLE_SIZE / 2.0),
                                                        pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (*port_num as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                    transform.inverse().mul_pos(pointer),
                                                ],
                                                Stroke::new(1.0, Color32::DARK_RED),
                                            );
                                        }
                                    }
                                    SynthModulePort::Output(payload_module, port_num) => {
                                        if ByAddress(payload_module.clone())
                                            == ByAddress(module_ref.clone())
                                        {
                                            ui.painter().line_segment(
                                                [
                                                    [
                                                        pivot_pos.x + size.x
                                                            - (SYNTH_HANDLE_SIZE / 2.0),
                                                        pivot_pos.y
                                                            + (SYNTH_HANDLE_SIZE / 2.0)
                                                            + (*port_num as f32
                                                                * (SYNTH_HANDLE_SIZE
                                                                    + SYNTH_HANDLE_PADDING)),
                                                    ]
                                                    .into(),
                                                    transform.inverse().mul_pos(pointer),
                                                ],
                                                Stroke::new(1.0, Color32::DARK_RED),
                                            );
                                        }
                                    }
                                }
                            }

                            // draw connections
                            for (input_idx, input_module) in
                                synth::get_inputs(&*module).iter().enumerate()
                            {
                                let mut dragging = false;
                                let mut dragged_port: Option<(SharedSynthModule, u8)> = None;
                                if hover_wire.as_ref().is_some_and(|(_, _, m, i)| {
                                    *i == input_idx as u8
                                        && ByAddress(m.clone()) == ByAddress(module_ref.clone())
                                }) {
                                    dragged_port = Some((
                                        hover_wire.as_ref().unwrap().0.clone(),
                                        hover_wire.as_ref().unwrap().1,
                                    ));
                                    dragging = true;
                                }
                                if let Some((input_module, port)) =
                                    input_module.as_ref().or_else(|| dragged_port.as_ref())
                                {
                                    let input_module = input_module.read().unwrap();
                                    let input_module_area_id =
                                        id.with(("module", input_module.get_id(), workspace.loads));
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
                                                Stroke::new(
                                                    if dragging { 2.0 } else { 1.0 },
                                                    Color32::RED,
                                                ),
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
        if let Some((src_module, src_port)) = output_to_disconnect {
            for module in workspace.modules.iter() {
                let mut unlocked = module.write().unwrap();
                for input_idx in 0..unlocked.get_num_inputs() {
                    if let Some((input_module, input_port)) = unlocked.get_input(input_idx).unwrap()
                    {
                        if ByAddress(input_module) == ByAddress(src_module.clone())
                            && input_port == src_port
                        {
                            unlocked.disconnect_input(input_idx).unwrap();
                            dirty = true;
                        }
                    }
                }
            }
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

#[derive(Serialize, Deserialize, Default)]
struct FileFormat {
    modules: Vec<synth::SynthModuleType>,
    /// List of connections in (src_id, src_port, sink_id, sink_port) format
    connections: Vec<(String, u8, String, u8)>,

    /// List of workspace positions for modules by id
    positions: Vec<(String, (f32, f32))>,
}

impl FileFormat {
    fn capture_pos<F>(&mut self, modules: &Vec<SharedSynthModule>, get_pos: F)
    where
        F: Fn(String) -> Result<(f32, f32), ()>,
    {
        for module in modules {
            let unlocked = module.read().unwrap();
            let id = unlocked.get_id();
            drop(unlocked);
            if let Ok(pos) = get_pos(id.clone()) {
                self.positions.push((id, pos));
            }
        }
    }

    fn unpack_pos<F>(&mut self, mut set_pos: F)
    where
        F: FnMut(String, (f32, f32)),
    {
        while let Some((id, pos)) = self.positions.pop() {
            set_pos(id, pos);
        }
    }

    fn capture_connections(&mut self, modules: &Vec<SharedSynthModule>) {
        let mut id_map: HashMap<ByAddress<SharedSynthModule>, String> = HashMap::new();
        for module in modules {
            let unlocked = module.read().unwrap();
            id_map.insert(ByAddress(module.clone()), unlocked.get_id());
        }
        for module in modules {
            let unlocked = module.read().unwrap();
            for (sink_port, (source_module, source_port)) in synth::get_inputs(&*unlocked)
                .into_iter()
                .enumerate()
                .filter(|(_, i)| i.is_some())
                .map(|(idx, i)| (idx, i.unwrap()))
            {
                self.connections.push((
                    id_map
                        .get(&ByAddress(source_module.clone()))
                        .unwrap()
                        .to_string(),
                    source_port,
                    unlocked.get_id(),
                    sink_port as u8,
                ))
            }
        }
    }

    fn capture_modules(&mut self, modules: &Vec<SharedSynthModule>) {
        for module in modules {
            let unlocked = module.read().unwrap();
            self.modules.push(
                synth::any_module_to_enum(Box::new(&*unlocked))
                    .expect("Unable to prepare module for serialization"),
            );
        }
    }

    fn unpack_modules(
        &mut self,
        modules: &mut Vec<SharedSynthModule>,
        audio_config: &synth::AudioConfig,
    ) {
        while let Some(module) = self.modules.pop() {
            let module = synth::enum_to_sharedsynthmodule(module);
            let mut locked = module.write().unwrap();
            locked.set_audio_config(audio_config);
            drop(locked);
            modules.push(module);
        }
    }

    fn unpack_connections(
        &mut self,
        modules: &Vec<SharedSynthModule>,
    ) -> Result<(), Box<dyn Error>> {
        let mut id_map: HashMap<String, ByAddress<SharedSynthModule>> = HashMap::new();
        for module in modules {
            let unlocked = module.read().unwrap();
            id_map.insert(unlocked.get_id(), ByAddress(module.clone()));
        }
        while let Some((src_id, src_port, sink_id, sink_port)) = self.connections.pop() {
            if let (Some(sink_module), Some(src_module)) =
                (id_map.get(&sink_id), id_map.get(&src_id))
            {
                let mut unlocked = sink_module.write().unwrap();
                let _ = unlocked.set_input(sink_port, (**src_module).clone(), src_port);
            }
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_async<F: Future<Output = ()> + Send + 'static>(f: F) {
    std::thread::spawn(move || futures::executor::block_on(f));
}

#[cfg(target_arch = "wasm32")]
pub fn run_async<F: Future<Output = ()> + 'static>(f: F) {
    wasm_bindgen_futures::spawn_local(f);
}
