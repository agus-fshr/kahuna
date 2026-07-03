use crate::{
    Message,
    system_state::{MemoryViewerState, SystemState},
    translation::{TranslationResultExt, ValueKindExt},
    wave_container::ScopeRefExt,
};
use egui_extras::{Column, TableBuilder};
use std::rc::Rc;
use surfer_translation_types::{TranslationPreference, Translator, ValueKind};
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
impl Default for MemoryViewerState {
    fn default() -> Self {
        Self {
            open: false,
            scope: None,
            name: None,
            jump_to_index: String::new(),
            search_value: String::new(),
            index_format: MemoryViewerFormat::Decimal,
            value_format: "Hexadecimal".to_string(),
            scroll_to_row: None,
            color_values: false,
            change_display_modes: ChangeModes::AllValues,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChangeModes {
    AllValues,
    ChangedAtCursor,
    ChangedBtwCursorAndMarker(u8),
}
#[derive(Clone)]
pub(crate) struct MemoryRow {
    index: i64,
    value: String,
    kind: ValueKind,
    changed_values: bool,
    change_b_selected_times: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MemoryViewerCacheKey {
    pub scope: crate::wave_container::ScopeRef,
    pub cursor: num::BigUint,
    pub value_format: String,
    pub change_display_modes: ChangeModes,
}

#[derive(Clone)]
pub(crate) struct MemoryViewerCache {
    pub key: MemoryViewerCacheKey,
    pub rows: std::rc::Rc<Vec<MemoryRow>>,
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

fn parse_index_from_name(name: &str) -> Option<i64> {
    name.trim()
        .trim_start_matches('[')
        .split(']')
        .next()
        .and_then(|index| index.parse::<i64>().ok())
}

fn parse_jump_to_index(input: &str) -> Option<i64> {
    let input = input.trim();

    if let Some(hex) = input.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(binary) = input.strip_prefix("0b") {
        i64::from_str_radix(binary, 2).ok()
    } else {
        input.parse::<i64>().ok()
    }
}

fn closest_row_index(rows: &[&MemoryRow], target_index: i64) -> Option<usize> {
    rows.iter()
        .enumerate()
        .min_by_key(|(_position, row)| (row.index - target_index).abs())
        .map(|(row_index, _)| row_index)
}
fn marker_combo(ui: &mut egui::Ui, id: &'static str, selected: &mut ChangeModes) {
    let selected_marker = match selected {
        ChangeModes::ChangedBtwCursorAndMarker(index) => *index,
        _ => 1,
    };

    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("Marker {selected_marker}"))
        .show_ui(ui, |ui| {
            for marker_index in 0..=254 {
                ui.selectable_value(
                    selected,
                    ChangeModes::ChangedBtwCursorAndMarker(marker_index),
                    format!("Marker {marker_index}"),
                );
            }
        });
}
impl SystemState {
    pub fn draw_memory_viewer_window(&mut self, ctx: &egui::Context, _msgs: &mut Vec<Message>) {
        if !self.memory_viewer.open {
            return;
        }

        let mut open = self.memory_viewer.open;
        let translator_name = self.memory_viewer.value_format.clone();
        let translator = self.translators.get_translator(&translator_name);
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
                let cache_key = MemoryViewerCacheKey {
                    scope: scope.clone(),
                    cursor: cursor.clone(),
                    value_format: translator_name.clone(),
                    change_display_modes: self.memory_viewer.change_display_modes,
                };
                let cache_hit = self
                    .memory_viewer_cache
                    .as_ref()
                    .filter(|cache| cache.key == cache_key)
                    .map(|cache| Rc::clone(&cache.rows));

                if let Some(cached_rows) = cache_hit {
                    rows = cached_rows.as_ref().clone();
                }

                let mut variables = wave_container.variables_in_scope(&scope);
                variables.sort_by_key(|v| v.index.unwrap_or(i64::MAX));

                let change_range = match self.memory_viewer.change_display_modes {
                    ChangeModes::ChangedBtwCursorAndMarker(marker_index) => {
                        waves.markers.get(&marker_index).and_then(|marker| {
                            marker.to_biguint().map(|marker_time| {
                                if cursor <= marker_time {
                                    (cursor.clone(), marker_time)
                                } else {
                                    (marker_time, cursor.clone())
                                }
                            })
                        })
                    }
                    _ => None,
                };
                if rows.is_empty() {
                    for var_ref in &variables {
                        let Some(index) = var_ref
                            .index
                            .or_else(|| parse_index_from_name(&var_ref.name))
                        else {
                            continue;
                        };

                        let Some((change_time, raw_value)) = wave_container
                            .query_variable(var_ref, &cursor)
                            .ok()
                            .flatten()
                            .and_then(|query_result| query_result.current)
                        else {
                            continue;
                        };

                        let changed = change_time == cursor;
                        let change_b_selected_times = change_range
                            .as_ref()
                            .and_then(|(start_time, end_time)| {
                                wave_container
                                    .query_variable(var_ref, start_time)
                                    .ok()
                                    .flatten()
                                    .and_then(|query_result| query_result.next)
                                    .map(|next_change| {
                                        next_change > *start_time && next_change < *end_time
                                    })
                            })
                            .unwrap_or(false);

                        let display_value = wave_container
                            .variable_meta(var_ref)
                            .ok()
                            .and_then(|meta| {
                                translator
                                    .translate(&meta, &raw_value)
                                    .ok()
                                    .and_then(|result| {
                                        result
                                            .format_flat(
                                                &Some(translator_name.clone()),
                                                &[],
                                                &self.translators,
                                            )
                                            .into_iter()
                                            .next()
                                            .and_then(|formatted| {
                                                formatted
                                                    .value
                                                    .map(|value| (value.value, value.kind))
                                            })
                                    })
                            })
                            .unwrap_or_else(|| (raw_value.to_string(), ValueKind::Normal));

                        let (value, kind) = display_value;
                        rows.push(MemoryRow {
                            index,
                            value,
                            kind,
                            changed_values: changed,
                            change_b_selected_times,
                        });
                    }
                    self.memory_viewer_cache = Some(MemoryViewerCache {
                        key: cache_key,
                        rows: Rc::new(rows.clone()),
                    });
                }

                ui.label(format!("Structured entries: {}", rows.len()));

                ui.separator();

                egui::CollapsingHeader::new("View Options")
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.checkbox(&mut self.memory_viewer.color_values, "Color values");

                        ui.radio_value(
                            &mut self.memory_viewer.change_display_modes,
                            ChangeModes::AllValues,
                            "Show all values",
                        );
                        ui.radio_value(
                            &mut self.memory_viewer.change_display_modes,
                            ChangeModes::ChangedAtCursor,
                            "Show changed values at current cursor",
                        );
                        ui.horizontal(|ui| {
                            let marker_index = match self.memory_viewer.change_display_modes {
                                ChangeModes::ChangedBtwCursorAndMarker(index) => index,
                                _ => 1,
                            };

                            ui.radio_value(
                                &mut self.memory_viewer.change_display_modes,
                                ChangeModes::ChangedBtwCursorAndMarker(marker_index),
                                "Show changed rows between cursor and",
                            );
                            marker_combo(
                                ui,
                                "memory_viewer_marker",
                                &mut self.memory_viewer.change_display_modes,
                            );
                        });
                    });

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

                    let (mut preferred_translators, mut bad_translators): (Vec<_>, Vec<_>) =
                        variables
                            .first()
                            .and_then(|first_var_ref| {
                                wave_container.variable_meta(first_var_ref).ok()
                            })
                            .map(|meta| {
                                self.translators
                                    .all_translator_names()
                                    .into_iter()
                                    .partition(|translator_name| {
                                        let translator =
                                            self.translators.get_translator(translator_name);

                                        match translator.translates(&meta) {
                                            Ok(TranslationPreference::Yes) => true,
                                            Ok(TranslationPreference::Prefer) => true,
                                            Ok(TranslationPreference::No) => false,
                                            Err(_) => false,
                                        }
                                    })
                            })
                            .unwrap_or_else(|| (vec![], self.translators.all_translator_names()));

                    preferred_translators.sort_by(|a, b| numeric_sort::cmp(a, b));
                    bad_translators.sort_by(|a, b| numeric_sort::cmp(a, b));

                    egui::ComboBox::from_id_salt("memory_viewer_value_format")
                        .selected_text(self.memory_viewer.value_format.clone())
                        .show_ui(ui, |ui| {
                            for name in preferred_translators {
                                ui.selectable_value(
                                    &mut self.memory_viewer.value_format,
                                    name.to_string(),
                                    name,
                                );
                            }

                            if !bad_translators.is_empty() {
                                ui.separator();
                                ui.label("Not recommended");

                                for name in bad_translators {
                                    ui.selectable_value(
                                        &mut self.memory_viewer.value_format,
                                        name.to_string(),
                                        name,
                                    );
                                }
                            }
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

                    ui.label("Filter:");
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
                    .filter(|row| {
                        match self.memory_viewer.change_display_modes {
                            ChangeModes::AllValues => {}
                            ChangeModes::ChangedAtCursor => {
                                if !row.changed_values {
                                    return false;
                                }
                            }
                            ChangeModes::ChangedBtwCursorAndMarker(_marker) => {
                                if !row.change_b_selected_times {
                                    return false;
                                }
                            }
                        }

                        if search_value.is_empty() {
                            return true;
                        }

                        let value = row.value.to_lowercase();
                        value == search_value || value.contains(&search_value)
                    })
                    .collect();

                if jump_requested
                    && let Some(target_index) =
                        parse_jump_to_index(&self.memory_viewer.jump_to_index)
                {
                    self.memory_viewer.scroll_to_row =
                        closest_row_index(&visible_rows, target_index);
                }

                let max_index = rows.iter().map(|row| row.index).max().unwrap_or(0);

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
                            let row_data = visible_rows[row_index];

                            row.col(|ui| {
                                ui.monospace(format_index(
                                    row_data.index,
                                    self.memory_viewer.index_format,
                                    max_index,
                                ));
                            });

                            row.col(|ui| {
                                if self.memory_viewer.color_values {
                                    let color = row_data.kind.color(
                                        self.user.config.theme.variable_default,
                                        &self.user.config.theme,
                                    );
                                    ui.horizontal(|ui| {
                                        ui.colored_label(color, "■");
                                        ui.monospace(&row_data.value);
                                    });
                                } else {
                                    ui.monospace(&row_data.value);
                                }
                            });
                        });
                    });
            });

        self.memory_viewer.open = open;
    }
}
