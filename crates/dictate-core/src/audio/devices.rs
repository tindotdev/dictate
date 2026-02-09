use cpal::traits::{DeviceTrait, HostTrait};
use unicode_normalization::UnicodeNormalization;

use crate::error::AudioError;

fn device_display_name(device: &cpal::Device) -> String {
    device
        .description()
        .map_or_else(|_| "<unnamed>".to_string(), |desc| desc.to_string())
}

fn default_input_device_name(host: &cpal::Host) -> Option<String> {
    host.default_input_device()
        .and_then(|d| d.description().ok().map(|desc| desc.to_string()))
}

fn parse_device_index(query: &str) -> Option<usize> {
    let query = query.trim();
    let query = query
        .strip_prefix('#')
        .or_else(|| query.strip_prefix('@'))
        .unwrap_or(query);
    if query.is_empty() || !query.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    query.parse::<usize>().ok()
}

fn normalize_device_key(s: &str) -> String {
    s.nfc() // Unicode NFC normalization (Tier 2)
        .filter(|c| !c.is_whitespace()) // Remove all whitespace (Tier 1)
        .filter(|c| !matches!(c, '-' | '.' | '(' | ')' | '/' | '_' | ':' | '[' | ']')) // Strip punctuation (Tier 1)
        .flat_map(char::to_lowercase) // Lowercase conversion
        .collect()
}

struct InputDeviceCandidate {
    index: usize,
    name: String,
    key: String,
    is_default: bool,
    device: cpal::Device,
}

pub(crate) fn resolve_input_device(
    host: &cpal::Host,
    query: &str,
) -> Result<cpal::Device, AudioError> {
    let raw_query = query.trim();
    if raw_query.is_empty() {
        return Err(AudioError::device_not_found("empty device query"));
    }

    let default_name = default_input_device_name(host);

    let candidates: Vec<InputDeviceCandidate> = host
        .input_devices()
        .map_err(|e| AudioError::from_devices(&e))?
        .enumerate()
        .map(|(index, device)| {
            let name = device_display_name(&device);
            let is_default = default_name
                .as_ref()
                .is_some_and(|default_name| default_name == &name);
            let key = normalize_device_key(&name);
            InputDeviceCandidate {
                index,
                name,
                key,
                is_default,
                device,
            }
        })
        .collect();

    if let Some(index) = parse_device_index(raw_query) {
        return candidates
            .into_iter()
            .find_map(|candidate| (candidate.index == index).then_some(candidate.device))
            .ok_or_else(|| AudioError::device_not_found(format!("index {index}")));
    }

    let query_key = normalize_device_key(raw_query);

    let exact_matches: Vec<usize> = candidates
        .iter()
        .filter_map(|c| (c.key == query_key).then_some(c.index))
        .collect();

    if exact_matches.len() == 1 {
        let selected_index = exact_matches[0];
        return candidates
            .into_iter()
            .find_map(|candidate| (candidate.index == selected_index).then_some(candidate.device))
            .ok_or_else(|| AudioError::device_not_found(raw_query));
    }
    if exact_matches.len() > 1 {
        let matches = exact_matches
            .iter()
            .filter_map(|idx| candidates.iter().find(|c| c.index == *idx))
            .map(|c| {
                let default_marker = if c.is_default { " (default)" } else { "" };
                format!("  {}: {}{default_marker}", c.index, c.name)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(AudioError::device_ambiguous(raw_query, matches));
    }

    let substring_matches: Vec<usize> = candidates
        .iter()
        .filter_map(|c| c.key.contains(&query_key).then_some(c.index))
        .collect();

    match substring_matches.len() {
        0 => Err(AudioError::device_not_found(format!(
            "{raw_query} (run `dictate devices`)"
        ))),
        1 => {
            let selected_index = substring_matches[0];
            candidates
                .into_iter()
                .find_map(|candidate| {
                    (candidate.index == selected_index).then_some(candidate.device)
                })
                .ok_or_else(|| AudioError::device_not_found(raw_query))
        }
        _ => {
            let matches = substring_matches
                .iter()
                .filter_map(|idx| candidates.iter().find(|c| c.index == *idx))
                .map(|c| {
                    let default_marker = if c.is_default { " (default)" } else { "" };
                    format!("  {}: {}{default_marker}", c.index, c.name)
                })
                .collect::<Vec<_>>()
                .join("\n");
            Err(AudioError::device_ambiguous(raw_query, matches))
        }
    }
}

/// Information about an audio input device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Device index in the enumeration.
    pub index: usize,
    /// Human-readable device name.
    pub name: String,
    /// Whether this is the system's default input device.
    pub is_default: bool,
    /// Sample rate in Hz (if available).
    pub sample_rate: Option<u32>,
    /// Number of audio channels (if available).
    pub channels: Option<u16>,
    /// Sample format as a string (if available).
    pub sample_format: Option<String>,
}

/// Lists all available audio input devices on the system.
///
/// Returns device information including name, default status, and configuration
/// details. The index field can be used for user selection but is not a stable
/// device identifier.
///
/// # Errors
///
/// Returns [`AudioError::DeviceNotFound`] if the system cannot enumerate devices.
pub fn list_input_devices() -> Result<Vec<DeviceInfo>, AudioError> {
    let host = cpal::default_host();

    // Get default device description for comparison
    let default_name = default_input_device_name(&host);

    let devices = host
        .input_devices()
        .map_err(|e| AudioError::from_devices(&e))?;
    let mut device_infos = Vec::new();

    for (idx, device) in devices.enumerate() {
        let name = device_display_name(&device);

        let is_default = default_name
            .as_ref()
            .is_some_and(|default_name| default_name == &name);

        // Try to get device configuration
        let (sample_rate, channels, sample_format) =
            device
                .default_input_config()
                .ok()
                .map_or((None, None, None), |config| {
                    (
                        Some(config.sample_rate()),
                        Some(config.channels()),
                        Some(format!("{:?}", config.sample_format())),
                    )
                });

        device_infos.push(DeviceInfo {
            index: idx,
            name,
            is_default,
            sample_rate,
            channels,
            sample_format,
        });
    }

    Ok(device_infos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_index_accepts_hash_and_at_prefix() {
        assert_eq!(parse_device_index("#0"), Some(0));
        assert_eq!(parse_device_index("@12"), Some(12));
        assert_eq!(parse_device_index("7"), Some(7));
        assert_eq!(parse_device_index(""), None);
        assert_eq!(parse_device_index("#"), None);
        assert_eq!(parse_device_index("@"), None);
        assert_eq!(parse_device_index("#x"), None);
    }

    #[test]
    fn normalize_device_key_removes_whitespace_punctuation_and_lowercases() {
        // Whitespace removal
        assert_eq!(normalize_device_key("  USB   Mic  "), "usbmic");
        assert_eq!(normalize_device_key("MiXeD CaSe"), "mixedcase");

        // Punctuation stripping
        assert_eq!(normalize_device_key("USB-C Audio"), "usbcaudio");
        assert_eq!(normalize_device_key("Built-in Mic"), "builtinmic");
        assert_eq!(normalize_device_key("Audio (Default)"), "audiodefault");
        assert_eq!(normalize_device_key("USB_Audio/Device"), "usbaudiodevice");
        assert_eq!(normalize_device_key("Scarlett 2i2 [USB]"), "scarlett2i2usb");

        // Unicode NFC normalization (combining characters)
        assert_eq!(normalize_device_key("USB™ Audio"), "usb™audio");
        assert_eq!(normalize_device_key("Café Mic"), "cafémic");
    }
}
