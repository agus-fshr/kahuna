use crate::{Message, system_state::SystemState, wave_container::ScopeRefExt};
use egui_extras::{Column, TableBuilder};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemoryViewerFormat {
    Decimal,
    Hexadecimal,
    Binary,
}

impl MemoryViewerFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Decimal => "Decimal",
            Self::Hexadecimal => "Hexadecimal",
            Self::Binary => "Binary",
        }
    }
}

fn format_index(index: i64, format: MemoryViewerFormat, max_index: i64) -> String {
    match format {
        MemoryViewerFormat::Decimal => {
            let width = max_index.to_string().len();
            format!("{index:0width$}")
        }
        MemoryViewerFormat::Hexadecimal => {
            let width = format!("{max_index:x}").len();
            format!("{index:0width$x}")
        }
        MemoryViewerFormat::Binary => {
            let width = format!("{max_index:b}").len();
            format!("{index:0width$b}")
        }
    }
}

fn format_value(value: &str, format: MemoryViewerFormat) -> String {
    let Ok(parsed) = value.parse::<u64>() else {
        return value.to_string();
    };

    match format {
        MemoryViewerFormat::Decimal => parsed.to_string(),
        MemoryViewerFormat::Hexadecimal => format!("{:x}", parsed),
        MemoryViewerFormat::Binary => format!("{:b}", parsed),
    }
}

fn parse_index_from_name(name: &str) -> Option<i64> {
    name.trim()
        .trim_start_matches('[')
        .split(']')
        .next()
        .and_then(|index| index.parse::<i64>().ok())
}

impl SystemState {
    pub fn draw_memory_viewer_window(&mut self, ctx: &egui::Context, _msgs: &mut Vec<Message>) {
        if !self.memory_viewer.open {
            return;
        }

        let mut open = self.memory_viewer.open;

        egui::Window::new("Memory Viewer")
            .open(&mut open)
            .resizable(true)
            .default_size([520.0, 500.0])
            .show(ctx, |ui| {
                ui.heading("Memory Viewer");

                let Some(scope) = self.memory_viewer.scope.clone() else {
                    ui.label("No variable selected");
                    return;
                };

                let display_name = self
                    .memory_viewer
                    .name
                    .clone()
                    .unwrap_or_else(|| scope.name());

                ui.label(format!("Variable: {display_name}"));

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

                let mut rows = Vec::new();

                let mut variables = wave_container.variables_in_scope(&scope);
                variables.sort_by_key(|v| v.index.unwrap_or(i64::MAX));

                for var_ref in variables {
                    let Some(index) = var_ref
                        .index
                        .or_else(|| parse_index_from_name(&var_ref.name))
                    else {
                        continue;
                    };

                    let value = wave_container
                        .query_variable(&var_ref, &cursor)
                        .ok()
                        .flatten()
                        .and_then(|query_result| query_result.current)
                        .map(|(_, value)| value);

                    let Some(value) = value else {
                        continue;
                    };

                    rows.push((index, value.to_string()));
                }

                ui.label(format!("Structured entries: {}", rows.len()));

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Index format:");
                    egui::ComboBox::from_id_salt("memory_viewer_index_format")
                        .selected_text(self.memory_viewer.index_format.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.memory_viewer.index_format,
                                MemoryViewerFormat::Decimal,
                                "Decimal",
                            );
                            ui.selectable_value(
                                &mut self.memory_viewer.index_format,
                                MemoryViewerFormat::Hexadecimal,
                                "Hexadecimal",
                            );
                            ui.selectable_value(
                                &mut self.memory_viewer.index_format,
                                MemoryViewerFormat::Binary,
                                "Binary",
                            );
                        });

                    ui.label("Value format:");
                    egui::ComboBox::from_id_salt("memory_viewer_value_format")
                        .selected_text(self.memory_viewer.value_format.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.memory_viewer.value_format,
                                MemoryViewerFormat::Decimal,
                                "Decimal",
                            );
                            ui.selectable_value(
                                &mut self.memory_viewer.value_format,
                                MemoryViewerFormat::Hexadecimal,
                                "Hexadecimal",
                            );
                            ui.selectable_value(
                                &mut self.memory_viewer.value_format,
                                MemoryViewerFormat::Binary,
                                "Binary",
                            );
                        });
                });

                let mut jump_requested = false;

                ui.horizontal(|ui| {
                    ui.label("Jump to index:");
                    ui.add_sized(
                        [60.0, 20.0],
                        egui::TextEdit::singleline(&mut self.memory_viewer.jump_to_index),
                    );

                    if ui.button("Jump").clicked() {
                        jump_requested = true;
                    }

                    ui.label("Search value:");
                    ui.add_sized(
                        [90.0, 20.0],
                        egui::TextEdit::singleline(&mut self.memory_viewer.search_value),
                    );

                    ui.separator();
                });

                ui.separator();

                let search_value = self.memory_viewer.search_value.trim().to_lowercase();

                let visible_rows: Vec<_> = rows
                    .iter()
                    .filter(|(_, value)| {
                        if search_value.is_empty() {
                            return true;
                        }

                        let display_value =
                            format_value(value, self.memory_viewer.value_format).to_lowercase();

                        display_value == search_value || display_value.contains(&search_value)
                    })
                    .collect();

                if jump_requested
                    && let Ok(target_index) = self.memory_viewer.jump_to_index.trim().parse::<i64>()
                {
                    self.memory_viewer.scroll_to_row = visible_rows
                        .iter()
                        .position(|(index, _)| *index == target_index);
                }

                let max_index = rows.iter().map(|(index, _)| *index).max().unwrap_or(0);

                let text_height = egui::TextStyle::Body
                    .resolve(ui.style())
                    .size
                    .max(ui.spacing().interact_size.y);

                let available_height = ui.available_height().max(250.0);

                let mut table = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(Column::auto())
                    .column(Column::remainder())
                    .min_scrolled_height(150.0)
                    .max_scroll_height(available_height);

                if let Some(row_index) = self.memory_viewer.scroll_to_row.take() {
                    table = table.scroll_to_row(row_index, Some(egui::Align::Min));
                }

                table
                    .header(24.0, |mut header| {
                        header.col(|ui| {
                            ui.monospace("Index");
                        });
                        header.col(|ui| {
                            ui.monospace("Value");
                        });
                    })
                    .body(|body| {
                        body.rows(text_height, visible_rows.len(), |mut row| {
                            let row_index = row.index();
                            let (index, value) = visible_rows[row_index];

                            row.col(|ui| {
                                ui.monospace(format_index(
                                    *index,
                                    self.memory_viewer.index_format,
                                    max_index,
                                ));
                            });

                            row.col(|ui| {
                                ui.monospace(format_value(value, self.memory_viewer.value_format));
                            });
                        });
                    });
            });

        self.memory_viewer.open = open;
    }
}
