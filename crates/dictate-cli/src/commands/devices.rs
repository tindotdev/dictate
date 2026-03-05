use crate::ui;
use std::fmt::Write as _;
use unicode_width::UnicodeWidthStr;

use dictate_core::{AudioError, list_input_devices};

struct DeviceRow {
    index: String,
    name: String,
    sample_rate: String,
    channels: String,
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
            index: format!("[{}]", device.index),
            name: {
                let mut name = device.name.clone();
                if device.is_default {
                    name.push_str(" (default)");
                }
                name
            },
            sample_rate: device
                .sample_rate
                .map_or_else(|| "?".to_string(), |rate| format!("{rate} Hz")),
            channels: device
                .channels
                .map_or_else(|| "?".to_string(), |channels| channels.to_string()),
            format: device
                .sample_format
                .clone()
                .unwrap_or_else(|| "?".to_string()),
        })
        .collect();

    println!("{}", render_rows(&rows));

    Ok(())
}

fn render_rows(rows: &[DeviceRow]) -> String {
    let index_header = "#";
    let name_header = "Device";
    let sample_rate_header = "Sample Rate";
    let channels_header = "Channels";
    let format_header = "Format";

    let index_width = rows
        .iter()
        .map(|row| display_width(&row.index))
        .max()
        .unwrap_or(0)
        .max(display_width(index_header));
    let name_width = rows
        .iter()
        .map(|row| display_width(&row.name))
        .max()
        .unwrap_or(0)
        .max(display_width(name_header));
    let sample_rate_width = rows
        .iter()
        .map(|row| display_width(&row.sample_rate))
        .max()
        .unwrap_or(0)
        .max(display_width(sample_rate_header));
    let channels_width = rows
        .iter()
        .map(|row| display_width(&row.channels))
        .max()
        .unwrap_or(0)
        .max(display_width(channels_header));
    let format_width = rows
        .iter()
        .map(|row| display_width(&row.format))
        .max()
        .unwrap_or(0)
        .max(display_width(format_header));

    let mut output = String::new();
    append_cell(&mut output, index_header, index_width);
    output.push_str("  ");
    append_cell(&mut output, name_header, name_width);
    output.push_str("  ");
    append_cell(&mut output, sample_rate_header, sample_rate_width);
    output.push_str("  ");
    append_cell(&mut output, channels_header, channels_width);
    output.push_str("  ");
    append_cell(&mut output, format_header, format_width);
    output.push('\n');
    let _ = writeln!(
        output,
        "{}  {}  {}  {}  {}",
        "-".repeat(index_width),
        "-".repeat(name_width),
        "-".repeat(sample_rate_width),
        "-".repeat(channels_width),
        "-".repeat(format_width)
    );

    for row in rows {
        append_cell(&mut output, &row.index, index_width);
        output.push_str("  ");
        append_cell(&mut output, &row.name, name_width);
        output.push_str("  ");
        append_cell(&mut output, &row.sample_rate, sample_rate_width);
        output.push_str("  ");
        append_cell(&mut output, &row.channels, channels_width);
        output.push_str("  ");
        append_cell(&mut output, &row.format, format_width);
        output.push('\n');
    }

    output
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

fn append_cell(output: &mut String, value: &str, width: usize) {
    output.push_str(value);
    output.push_str(&" ".repeat(width.saturating_sub(display_width(value))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_rows_includes_headers_and_values() {
        let rows = vec![
            DeviceRow {
                index: "[0]".to_string(),
                name: "Built-in Mic (default)".to_string(),
                sample_rate: "48000 Hz".to_string(),
                channels: "2".to_string(),
                format: "f32".to_string(),
            },
            DeviceRow {
                index: "[1]".to_string(),
                name: "USB Audio".to_string(),
                sample_rate: "?".to_string(),
                channels: "1".to_string(),
                format: "?".to_string(),
            },
        ];

        let table = render_rows(&rows);
        assert!(table.contains('#'));
        assert!(table.contains("Device"));
        assert!(table.contains("Built-in Mic (default)"));
        assert!(table.contains("USB Audio"));
    }

    #[test]
    fn render_rows_aligns_columns_with_unicode_device_names() {
        let rows = vec![
            DeviceRow {
                index: "[0]".to_string(),
                name: "マイク".to_string(),
                sample_rate: "48000 Hz".to_string(),
                channels: "2".to_string(),
                format: "f32".to_string(),
            },
            DeviceRow {
                index: "[1]".to_string(),
                name: "Mícrófono USB".to_string(),
                sample_rate: "44100 Hz".to_string(),
                channels: "1".to_string(),
                format: "i16".to_string(),
            },
        ];

        let table = render_rows(&rows);
        let lines: Vec<&str> = table.lines().collect();

        let sample_rate_column_starts: Vec<usize> = [
            ("Sample Rate", lines[0]),
            ("48000 Hz", lines[2]),
            ("44100 Hz", lines[3]),
        ]
        .into_iter()
        .map(|(cell, line)| {
            let byte_start = line.find(cell).expect("column value should exist");
            display_width(&line[..byte_start])
        })
        .collect();

        assert!(
            sample_rate_column_starts
                .iter()
                .all(|start| *start == sample_rate_column_starts[0])
        );
    }
}
