use anyhow::Result;
use serde::Serialize;

use crate::format::Table;

#[derive(Clone, Serialize)]
pub struct AlsaCard {
    pub index: i32,
    pub name: String,
    pub longname: String,
}

#[derive(Serialize)]
struct PcmDevice {
    device: u32,
    name: String,
    id: String,
    subdevices_count: u32,
    subdevices_avail: u32,
}

pub fn list_cards() -> Result<Vec<AlsaCard>> {
    let mut cards = Vec::new();
    for res in alsa::card::Iter::new() {
        let card = res?;
        cards.push(AlsaCard {
            index: card.get_index(),
            name: card.get_name()?,
            longname: card.get_longname()?,
        });
    }
    Ok(cards)
}

fn enumerate_pcm_devices(ctl: &alsa::Ctl, direction: alsa::Direction) -> Vec<PcmDevice> {
    let mut devices = Vec::new();
    for device_id in alsa::ctl::DeviceIter::new(ctl) {
        match ctl.pcm_info(device_id as u32, 0, direction) {
            Ok(info) => {
                devices.push(PcmDevice {
                    device: info.get_device(),
                    name: info.get_name().unwrap_or("?").to_string(),
                    id: info.get_id().unwrap_or("?").to_string(),
                    subdevices_count: info.get_subdevices_count(),
                    subdevices_avail: info.get_subdevices_avail(),
                });
            }
            Err(_) => continue,
        }
    }
    devices
}

pub fn card_detail(index: i32) -> Result<String> {
    let card = alsa::Card::new(index);
    let ctl = alsa::Ctl::from_card(&card, false)?;
    let info = ctl.card_info()?;

    #[derive(Serialize)]
    struct CardDetail {
        index: i32,
        name: String,
        longname: String,
        driver: String,
        mixer: String,
        components: String,
        playback_devices: Vec<PcmDevice>,
        capture_devices: Vec<PcmDevice>,
    }

    let detail = CardDetail {
        index,
        name: info.get_name()?.to_string(),
        longname: info.get_longname()?.to_string(),
        driver: info.get_driver()?.to_string(),
        mixer: info.get_mixername()?.to_string(),
        components: info.get_components()?.to_string(),
        playback_devices: enumerate_pcm_devices(&ctl, alsa::Direction::Playback),
        capture_devices: enumerate_pcm_devices(&ctl, alsa::Direction::Capture),
    };

    Ok(serde_json::to_string_pretty(&detail)?)
}

pub fn list_cards_formatted() -> Result<String> {
    let cards = list_cards()?;
    let mut t = Table::new(&["index", "name", "longname"]);
    for c in &cards {
        let idx = c.index.to_string();
        t.row(&[&idx, &c.name, &c.longname]);
    }
    Ok(t.render())
}
