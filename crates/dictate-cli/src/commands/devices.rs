use crate::ui;
use dictate_core::{AudioError, list_input_devices};
use owo_colors::OwoColorize;
use tabled::{
    Table, Tabled,
    settings::{Style, formatting::AlignmentStrategy},
};

#[derive(Tabled)]
struct DeviceRow {
    #[tabled(rename = "#")]
    index: String,
    #[tabled(rename = "Device")]
    name: String,
    #[tabled(rename = "Sample Rate")]
    sample_rate: String,
    #[tabled(rename = "Channels")]
    channels: String,
    #[tabled(rename = "Format")]
    format: String,
}

pub fn run() -> Result<(), AudioError> {
    println!("Available audio input devices:\n");

    let devices = list_input_devices()?;

    if devices.is_empty() {
        ui::info("(no input devices found)");
        return Ok(());
    }

    let rows: Vec<DeviceRow> = devices
        .iter()
        .map(|device| DeviceRow {
            index: ui::device_index(device.index),
            name: {
                let mut name = device.name.bold().to_string();
                if device.is_default {
                    name.push_str(&ui::default_marker());
                }
                name
            },
            sample_rate: device.sample_rate.map_or_else(
                || "?".dimmed().to_string(),
                |r| format!("{r} Hz").dimmed().to_string(),
            ),
            channels: device.channels.map_or_else(
                || "?".dimmed().to_string(),
                |c| c.to_string().dimmed().to_string(),
            ),
            format: device.sample_format.as_ref().map_or_else(
                || "?".dimmed().to_string(),
                |f| f.clone().dimmed().to_string(),
            ),
        })
        .collect();

    let table = Table::new(rows)
        .with(Style::rounded())
        .with(AlignmentStrategy::PerLine)
        .to_string();

    println!("{table}");

    Ok(())
}
