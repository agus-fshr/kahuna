//! Settings dialog for a protocol decoder.
//!
//! Edits a working copy and only emits a message when something actually
//! changed, so that dragging a slider does not fill the undo stack with one
//! entry per frame.

use egui::{Align, ComboBox, Layout, RichText, Window};

use super::{
    BitOrder, DecoderSettings, Protocol, RoleBindings, WordFormat,
    i2c::I2cSettings,
    spi::SpiSettings,
    uart::{Parity, UartSettings},
};
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
                DecoderSettings::I2c(i2c) => draw_i2c_settings(ui, i2c),
                DecoderSettings::Uart(uart) => draw_uart_settings(ui, uart),
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

/// Word format picker, shared by every protocol that renders values.
fn format_combo(ui: &mut egui::Ui, id: &str, format: &mut WordFormat) {
    ComboBox::from_id_salt(id)
        .selected_text(format.to_string())
        .show_ui(ui, |ui| {
            for f in WordFormat::ALL {
                if ui.selectable_label(*format == f, f.to_string()).clicked() {
                    *format = f;
                }
            }
        });
}

fn bit_order_combo(ui: &mut egui::Ui, id: &str, order: &mut BitOrder) {
    ComboBox::from_id_salt(id)
        .selected_text(order.to_string())
        .show_ui(ui, |ui| {
            for o in BitOrder::ALL {
                if ui.selectable_label(*order == o, o.to_string()).clicked() {
                    *order = o;
                }
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
            bit_order_combo(ui, "spi_bit_order", &mut spi.bit_order);
            ui.end_row();

            ui.label("Word size");
            ui.add(egui::DragValue::new(&mut spi.word_size).range(1..=64));
            ui.end_row();

            ui.label("Format");
            format_combo(ui, "spi_format", &mut spi.format);
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

fn draw_i2c_settings(ui: &mut egui::Ui, i2c: &mut I2cSettings) {
    egui::Grid::new("i2c_settings")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Address");
            ComboBox::from_id_salt("i2c_address")
                .selected_text(if i2c.split_address {
                    "7-bit + R/W"
                } else {
                    "Raw frame"
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(i2c.split_address, "7-bit + R/W")
                        .clicked()
                    {
                        i2c.split_address = true;
                    }
                    if ui
                        .selectable_label(!i2c.split_address, "Raw frame")
                        .clicked()
                    {
                        i2c.split_address = false;
                    }
                });
            ui.end_row();

            ui.label("Format");
            format_combo(ui, "i2c_format", &mut i2c.format);
            ui.end_row();
        });
}

fn draw_uart_settings(ui: &mut egui::Ui, uart: &mut UartSettings) {
    egui::Grid::new("uart_settings")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Bit period");
            ui.horizontal(|ui| {
                let mut automatic = uart.bit_period.is_none();
                if ui.checkbox(&mut automatic, "Measure").changed() {
                    // Seed the manual value with whatever was measured, so
                    // switching to manual does not jump to an arbitrary number.
                    uart.bit_period = if automatic { None } else { Some(1000) };
                }
                if let Some(period) = &mut uart.bit_period {
                    ui.add(
                        egui::DragValue::new(period)
                            .range(1..=u64::MAX)
                            .suffix(" ticks"),
                    );
                } else {
                    ui.label("from narrowest pulse");
                }
            });
            ui.end_row();

            ui.label("Data bits");
            ui.add(egui::DragValue::new(&mut uart.data_bits).range(5..=9));
            ui.end_row();

            ui.label("Parity");
            ComboBox::from_id_salt("uart_parity")
                .selected_text(uart.parity.to_string())
                .show_ui(ui, |ui| {
                    for p in Parity::ALL {
                        if ui
                            .selectable_label(uart.parity == p, p.to_string())
                            .clicked()
                        {
                            uart.parity = p;
                        }
                    }
                });
            ui.end_row();

            ui.label("Stop bits");
            ui.add(egui::DragValue::new(&mut uart.stop_bits).range(1..=2));
            ui.end_row();

            ui.label("Bit order");
            bit_order_combo(ui, "uart_bit_order", &mut uart.bit_order);
            ui.end_row();

            ui.label("Idle level");
            ComboBox::from_id_salt("uart_idle")
                .selected_text(if uart.idle_high { "High" } else { "Low" })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(uart.idle_high, "High").clicked() {
                        uart.idle_high = true;
                    }
                    if ui.selectable_label(!uart.idle_high, "Low").clicked() {
                        uart.idle_high = false;
                    }
                });
            ui.end_row();

            ui.label("Format");
            format_combo(ui, "uart_format", &mut uart.format);
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
