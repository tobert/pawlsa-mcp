use anyhow::Result;
use serde::Serialize;

pub fn list_device_hints(category: &str) -> Result<String> {
    #[derive(Serialize)]
    struct DeviceHint {
        name: String,
        direction: String,
        description: String,
    }

    let hints = alsa::device_name::HintIter::new_str(None, category)?;
    let devices: Vec<DeviceHint> = hints
        .map(|h| {
            let dir = h.direction.map(|d| match d {
                alsa::Direction::Playback => "playback",
                alsa::Direction::Capture => "capture",
            });
            DeviceHint {
                name: h.name.unwrap_or_default(),
                direction: dir.unwrap_or("").to_string(),
                description: h.desc.unwrap_or_default(),
            }
        })
        .collect();
    Ok(serde_json::to_string_pretty(&devices)?)
}
