use anyhow::Result;

use crate::format::Table;

pub fn list_device_hints(category: &str) -> Result<String> {
    let hints = alsa::device_name::HintIter::new_str(None, category)?;
    let mut t = Table::new(&["name", "direction", "desc"]);
    for h in hints {
        let dir = h.direction.map(|d| match d {
            alsa::Direction::Playback => "playback",
            alsa::Direction::Capture => "capture",
        });
        t.row(&[
            h.name.as_deref().unwrap_or(""),
            dir.unwrap_or(""),
            h.desc.as_deref().unwrap_or(""),
        ]);
    }
    Ok(t.render())
}
