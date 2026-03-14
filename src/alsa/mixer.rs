use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct ChannelVolume {
    pub channel: String,
    pub volume: i64,
    pub volume_db: f32,
    pub switch: Option<bool>,
}

#[derive(Clone, Serialize)]
pub struct MixerElement {
    pub name: String,
    pub index: u32,
    pub has_playback_volume: bool,
    pub has_capture_volume: bool,
    pub has_playback_switch: bool,
    pub has_capture_switch: bool,
    pub playback_volume_range: Option<(i64, i64)>,
    pub capture_volume_range: Option<(i64, i64)>,
    pub playback_channels: Vec<ChannelVolume>,
    pub capture_channels: Vec<ChannelVolume>,
}

fn channel_name(ch: alsa::mixer::SelemChannelId) -> &'static str {
    match ch {
        alsa::mixer::SelemChannelId::FrontLeft => "front-left",
        alsa::mixer::SelemChannelId::FrontRight => "front-right",
        alsa::mixer::SelemChannelId::RearLeft => "rear-left",
        alsa::mixer::SelemChannelId::RearRight => "rear-right",
        alsa::mixer::SelemChannelId::FrontCenter => "front-center",
        alsa::mixer::SelemChannelId::Woofer => "woofer",
        alsa::mixer::SelemChannelId::SideLeft => "side-left",
        alsa::mixer::SelemChannelId::SideRight => "side-right",
        alsa::mixer::SelemChannelId::RearCenter => "rear-center",
        _ => "unknown",
    }
}

pub fn read_mixer(card_index: i32) -> Result<Vec<MixerElement>> {
    let card_name = format!("hw:{card_index}");
    let mixer = alsa::Mixer::new(&card_name, false)?;
    let mut elements = Vec::new();

    for elem in mixer.iter() {
        let Some(selem) = alsa::mixer::Selem::new(elem) else {
            continue;
        };
        let id = selem.get_id();
        let name = id.get_name().unwrap_or("?").to_string();
        let index = id.get_index();

        let has_pb_vol = selem.has_playback_volume();
        let has_cap_vol = selem.has_capture_volume();
        let has_pb_sw = selem.has_playback_switch();
        let has_cap_sw = selem.has_capture_switch();

        let pb_range = if has_pb_vol {
            Some(selem.get_playback_volume_range())
        } else {
            None
        };
        let cap_range = if has_cap_vol {
            Some(selem.get_capture_volume_range())
        } else {
            None
        };

        let mut playback_channels = Vec::new();
        let mut capture_channels = Vec::new();

        for &ch in alsa::mixer::SelemChannelId::all() {
            if has_pb_vol && selem.has_playback_channel(ch) {
                if let Ok(vol) = selem.get_playback_volume(ch) {
                    let db = selem
                        .get_playback_vol_db(ch)
                        .map(|mb| mb.to_db())
                        .unwrap_or(0.0);
                    let sw = if has_pb_sw {
                        selem.get_playback_switch(ch).ok().map(|v| v != 0)
                    } else {
                        None
                    };
                    playback_channels.push(ChannelVolume {
                        channel: channel_name(ch).to_string(),
                        volume: vol,
                        volume_db: db,
                        switch: sw,
                    });
                }
            }
            if has_cap_vol && selem.has_capture_channel(ch) {
                if let Ok(vol) = selem.get_capture_volume(ch) {
                    let db = selem
                        .get_capture_vol_db(ch)
                        .map(|mb| mb.to_db())
                        .unwrap_or(0.0);
                    let sw = if has_cap_sw {
                        selem.get_capture_switch(ch).ok().map(|v| v != 0)
                    } else {
                        None
                    };
                    capture_channels.push(ChannelVolume {
                        channel: channel_name(ch).to_string(),
                        volume: vol,
                        volume_db: db,
                        switch: sw,
                    });
                }
            }
        }

        elements.push(MixerElement {
            name,
            index,
            has_playback_volume: has_pb_vol,
            has_capture_volume: has_cap_vol,
            has_playback_switch: has_pb_sw,
            has_capture_switch: has_cap_sw,
            playback_volume_range: pb_range,
            capture_volume_range: cap_range,
            playback_channels,
            capture_channels,
        });
    }

    Ok(elements)
}

fn parse_channel(name: &str) -> alsa::mixer::SelemChannelId {
    match name {
        "front-left" => alsa::mixer::SelemChannelId::FrontLeft,
        "front-right" => alsa::mixer::SelemChannelId::FrontRight,
        "rear-left" => alsa::mixer::SelemChannelId::RearLeft,
        "rear-right" => alsa::mixer::SelemChannelId::RearRight,
        "front-center" => alsa::mixer::SelemChannelId::FrontCenter,
        "woofer" => alsa::mixer::SelemChannelId::Woofer,
        "side-left" => alsa::mixer::SelemChannelId::SideLeft,
        "side-right" => alsa::mixer::SelemChannelId::SideRight,
        "rear-center" => alsa::mixer::SelemChannelId::RearCenter,
        _ => alsa::mixer::SelemChannelId::mono(),
    }
}

pub fn set_volume(
    card_index: i32,
    element_name: &str,
    volume: i64,
    channel: Option<&str>,
) -> Result<()> {
    let card_name = format!("hw:{card_index}");
    let mixer = alsa::Mixer::new(&card_name, false)?;
    let id = alsa::mixer::SelemId::new(element_name, 0);
    let selem = mixer
        .find_selem(&id)
        .ok_or_else(|| anyhow::anyhow!("element '{element_name}' not found"))?;

    anyhow::ensure!(selem.has_playback_volume(), "element has no playback volume");

    match channel {
        Some(ch_name) => {
            let ch = parse_channel(ch_name);
            selem.set_playback_volume(ch, volume)?;
        }
        None => {
            selem.set_playback_volume_all(volume)?;
        }
    }
    Ok(())
}

pub fn set_switch(
    card_index: i32,
    element_name: &str,
    on: bool,
    channel: Option<&str>,
) -> Result<()> {
    let card_name = format!("hw:{card_index}");
    let mixer = alsa::Mixer::new(&card_name, false)?;
    let id = alsa::mixer::SelemId::new(element_name, 0);
    let selem = mixer
        .find_selem(&id)
        .ok_or_else(|| anyhow::anyhow!("element '{element_name}' not found"))?;

    anyhow::ensure!(
        selem.has_playback_switch(),
        "element has no playback switch"
    );

    let val = if on { 1 } else { 0 };
    match channel {
        Some(ch_name) => {
            let ch = parse_channel(ch_name);
            selem.set_playback_switch(ch, val)?;
        }
        None => {
            selem.set_playback_switch_all(val)?;
        }
    }
    Ok(())
}
