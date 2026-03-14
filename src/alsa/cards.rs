use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
pub struct AlsaCard {
    pub index: i32,
    pub name: String,
    pub longname: String,
}

#[derive(Clone, Serialize)]
pub struct PcmDevice {
    pub device: u32,
    pub name: String,
    pub id: String,
    pub subdevices_count: u32,
    pub subdevices_avail: u32,
}

#[derive(Clone, Serialize)]
pub struct CardDetail {
    pub card: AlsaCard,
    pub driver: String,
    pub mixer_name: String,
    pub components: String,
    pub playback_devices: Vec<PcmDevice>,
    pub capture_devices: Vec<PcmDevice>,
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

fn enumerate_pcm_devices(
    ctl: &alsa::Ctl,
    direction: alsa::Direction,
) -> Vec<PcmDevice> {
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

pub fn card_detail(index: i32) -> Result<Value> {
    let card = alsa::Card::new(index);
    let ctl = alsa::Ctl::from_card(&card, false)?;
    let info = ctl.card_info()?;

    let detail = CardDetail {
        card: AlsaCard {
            index,
            name: info.get_name()?.to_string(),
            longname: info.get_longname()?.to_string(),
        },
        driver: info.get_driver()?.to_string(),
        mixer_name: info.get_mixername()?.to_string(),
        components: info.get_components()?.to_string(),
        playback_devices: enumerate_pcm_devices(&ctl, alsa::Direction::Playback),
        capture_devices: enumerate_pcm_devices(&ctl, alsa::Direction::Capture),
    };

    Ok(serde_json::to_value(detail)?)
}
