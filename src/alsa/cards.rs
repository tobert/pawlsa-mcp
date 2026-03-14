use anyhow::Result;

use crate::format::Table;

#[derive(Clone)]
pub struct AlsaCard {
    pub index: i32,
    pub name: String,
    pub longname: String,
}

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

pub fn card_detail(index: i32) -> Result<String> {
    let card = alsa::Card::new(index);
    let ctl = alsa::Ctl::from_card(&card, false)?;
    let info = ctl.card_info()?;

    let mut out = format!(
        "card {}, {}, {}, driver:{}, mixer:{}, components:{}",
        index,
        info.get_name()?,
        info.get_longname()?,
        info.get_driver()?,
        info.get_mixername()?,
        info.get_components()?,
    );

    let pb = enumerate_pcm_devices(&ctl, alsa::Direction::Playback);
    let cap = enumerate_pcm_devices(&ctl, alsa::Direction::Capture);

    if !pb.is_empty() || !cap.is_empty() {
        out.push_str(" ▌ dir, dev, id, name, subdevs");
        for d in &pb {
            out.push_str(&format!(
                " ▌ playback, {}, {}, {}, {}/{}",
                d.device, d.id, d.name, d.subdevices_avail, d.subdevices_count
            ));
        }
        for d in &cap {
            out.push_str(&format!(
                " ▌ capture, {}, {}, {}, {}/{}",
                d.device, d.id, d.name, d.subdevices_avail, d.subdevices_count
            ));
        }
    }

    Ok(out)
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
