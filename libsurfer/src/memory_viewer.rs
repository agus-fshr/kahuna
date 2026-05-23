use crate::{Message, system_state::SystemState};

impl SystemState {
    pub fn draw_memory_viewer_window(&mut self, ctx: &egui::Context, _msgs: &mut Vec<Message>) {
        if !self.memory_viewer.open {
            return;
        }

        egui::Window::new("Memory Viewer")
            .open(&mut self.memory_viewer.open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Memory Viewer");

                let Some(variable) = &self.memory_viewer.variable else {
                    ui.label("No variable selected");
                    return;
                };

                ui.label(format!("Variable: {}", variable.name));

                let Some(waves) = &self.user.waves else {
                    ui.label("No waveform loaded");
                    return;
                };

                let Some(cursor) = waves.cursor.as_ref().and_then(|cursor| cursor.to_biguint())
                else {
                    ui.label("Place the cursor to inspect values.");
                    return;
                };

                ui.label(format!("Time: {cursor}"));

                let Some(wave_container) = waves.inner.as_waves() else {
                    ui.label("No wave container available");
                    return;
                };

                ui.separator();

                egui::Grid::new("memory_viewer_table")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("Index");
                        ui.label("Value");
                        ui.end_row();

                        let mut variables = wave_container.variables_in_scope(&variable.path);
                        variables.sort_by_key(|v| v.index.unwrap_or(i64::MAX));

                        for var_ref in variables {
                            let index = var_ref
                                .index
                                .map_or_else(|| var_ref.name.clone(), |idx| idx.to_string());

                            let value = wave_container
                                .query_variable(&var_ref, &cursor)
                                .ok()
                                .flatten()
                                .and_then(|query_result| query_result.current)
                                .map(|(_, value)| value);

                            let Some(value) = value else {
                                continue;
                            };

                            ui.label(index);
                            ui.label(format!("{value:?}"));
                            ui.end_row();
                        }
                    });
            });
    }
}
