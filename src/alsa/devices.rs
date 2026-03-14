use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct DeviceHint {
    pub name: Option<String>,
    pub desc: Option<String>,
    pub direction: Option<String>,
}

pub fn list_device_hints(category: &str) -> Result<Vec<DeviceHint>> {
    let hints = alsa::device_name::HintIter::new_str(None, category)?;
    Ok(hints
        .map(|h| DeviceHint {
            name: h.name,
            desc: h.desc,
            direction: h.direction.map(|d| match d {
                alsa::Direction::Playback => "playback".to_string(),
                alsa::Direction::Capture => "capture".to_string(),
            }),
        })
        .collect())
}
