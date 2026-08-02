//! Settings dialog for a protocol decoder.
//!
//! Edits a working copy and only emits a message when something actually
//! changed, so that dragging a slider does not fill the undo stack with one
//! entry per frame.

use egui::{Align, ComboBox, Layout, RichText, Window};

use super::{BitOrder, DecoderSettings, Protocol, RoleBindings, WordFormat, spi::SpiSettings};
use crate::displayed_item::{DecoderInstance, DisplayedDecoder, DisplayedItem};
use crate::message::Message;
use crate::wave_container::{VariableRef, VariableRefExt};
use crate::wave_data::WaveData;

/// Draw the dialog for `instance`. Does nothing if the decoder has since been
/// removed, which can happen if its rows are deleted while the dialog is open.
pub fn draw(
    ctx: &egui::Context,
    instance: DecoderInstance,
    waves: &WaveData,
    msgs: &mut Vec<Message>,
) {
    let Some(decoder) = waves.displayed_items.values().find_map(|item| match item {
        DisplayedItem::Decoder(d) if d.instance == instance => Some(d),
        _ => None,
    }) else {
        msgs.push(Message::DecoderOpenDialog(None));
        return;
    };

    // Signals the user can bind, taken from what the waveform actually has
    // loaded rather than only from the rows on screen.
    let candidates = candidate_signals(waves);

    let mut settings = decoder.settings.clone();
    let mut bindings = decoder.bindings.clone();
    let mut close = false;

    Window::new(format!("{} decoder", decoder.protocol()))
        .auto_sized()
        .collapsible(false)
        .show(ctx, |ui| {
            draw_role_bindings(ui, decoder, &mut bindings, &candidates);
            ui.separator();
            match &mut settings {
                DecoderSettings::Spi(spi) => draw_spi_settings(ui, spi),
            }

            let missing = bindings.missing(decoder.protocol());
            if !missing.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("Not decoding: {} unset", missing.join(", ")))
                        .color(ui.visuals().warn_fg_color),
                );
            }

            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });

    if settings != decoder.settings || bindings != decoder.bindings {
        msgs.push(Message::DecoderConfigure {
            instance,
            settings: Box::new(settings),
            bindings: Box::new(bindings),
        });
    }
    if close {
        msgs.push(Message::DecoderOpenDialog(None));
    }
}

/// Every variable currently displayed, deduplicated, as binding candidates.
fn candidate_signals(waves: &WaveData) -> Vec<VariableRef> {
    let mut out: Vec<VariableRef> = waves
        .items_tree
        .iter()
        .filter_map(|node| match waves.displayed_items.get(&node.item_ref) {
            Some(DisplayedItem::Variable(v)) => Some(v.variable_ref.clone()),
            _ => None,
        })
        .collect();
    out.dedup_by(|a, b| a == b);
    out
}

fn draw_role_bindings(
    ui: &mut egui::Ui,
    decoder: &DisplayedDecoder,
    bindings: &mut RoleBindings,
    candidates: &[VariableRef],
) {
    let roles = decoder.protocol().roles();
    bindings.fit_to(decoder.protocol());

    egui::Grid::new("decoder_roles")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for (idx, role) in roles.iter().enumerate() {
                let label = if role.required {
                    format!("{} *", role.name)
                } else {
                    role.name.to_string()
                };
                ui.label(label);

                let current = bindings
                    .get(idx)
                    .map_or_else(|| "-".to_string(), |v| v.full_path_string());

                ComboBox::from_id_salt(("decoder_role", idx))
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        // "-" unbinds the role, which is how an optional
                        // signal like MISO is turned off.
                        if ui
                            .selectable_label(bindings.get(idx).is_none(), "-")
                            .clicked()
                        {
                            bindings.signals[idx] = None;
                        }
                        for cand in candidates {
                            let selected = bindings.get(idx) == Some(cand);
                            if ui
                                .selectable_label(selected, cand.full_path_string())
                                .clicked()
                            {
                                bindings.signals[idx] = Some(cand.clone());
                            }
                        }
                    });
                ui.end_row();
            }
        });
}

fn draw_spi_settings(ui: &mut egui::Ui, spi: &mut SpiSettings) {
    egui::Grid::new("spi_settings")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Mode");
            let mut mode = spi.mode();
            ComboBox::from_id_salt("spi_mode")
                .selected_text(format!(
                    "{mode} (CPOL={}, CPHA={})",
                    u8::from(spi.cpol),
                    u8::from(spi.cpha)
                ))
                .show_ui(ui, |ui| {
                    for m in 0..4u8 {
                        let label = format!("{m} (CPOL={}, CPHA={})", (m >> 1) & 1, m & 1);
                        if ui.selectable_label(mode == m, label).clicked() {
                            mode = m;
                        }
                    }
                });
            if mode != spi.mode() {
                spi.set_mode(mode);
            }
            ui.end_row();

            ui.label("Bit order");
            ComboBox::from_id_salt("spi_bit_order")
                .selected_text(spi.bit_order.to_string())
                .show_ui(ui, |ui| {
                    for o in BitOrder::ALL {
                        if ui
                            .selectable_label(spi.bit_order == o, o.to_string())
                            .clicked()
                        {
                            spi.bit_order = o;
                        }
                    }
                });
            ui.end_row();

            ui.label("Word size");
            ui.add(egui::DragValue::new(&mut spi.word_size).range(1..=64));
            ui.end_row();

            ui.label("Format");
            ComboBox::from_id_salt("spi_format")
                .selected_text(spi.format.to_string())
                .show_ui(ui, |ui| {
                    for f in WordFormat::ALL {
                        if ui
                            .selectable_label(spi.format == f, f.to_string())
                            .clicked()
                        {
                            spi.format = f;
                        }
                    }
                });
            ui.end_row();

            ui.label("Chip select");
            ComboBox::from_id_salt("spi_cs_polarity")
                .selected_text(if spi.cs_active_low {
                    "Active low"
                } else {
                    "Active high"
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(spi.cs_active_low, "Active low")
                        .clicked()
                    {
                        spi.cs_active_low = true;
                    }
                    if ui
                        .selectable_label(!spi.cs_active_low, "Active high")
                        .clicked()
                    {
                        spi.cs_active_low = false;
                    }
                });
            ui.end_row();
        });
}

/// Menu entries for creating a decoder, shared by the item context menu and the
/// top-level menu bar.
pub fn add_menu(ui: &mut egui::Ui, msgs: &mut Vec<Message>) {
    for protocol in Protocol::ALL {
        if ui.button(protocol.to_string()).clicked() {
            msgs.push(Message::DecoderAdd {
                protocol,
                items: None,
            });
            ui.close();
        }
    }
}
