use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct MidiClient {
    pub client_id: i32,
    pub name: String,
    pub ports: Vec<MidiPort>,
}

#[derive(Clone, Serialize)]
pub struct MidiPort {
    pub port_id: i32,
    pub name: String,
    pub capability: String,
    pub port_type: String,
    pub midi_channels: i32,
    pub midi_voices: i32,
    pub synth_voices: i32,
    pub read_use: i32,
    pub write_use: i32,
}

pub fn list_midi_ports() -> Result<Vec<MidiClient>> {
    let seq = alsa::seq::Seq::open(None, None, false)?;
    let mut clients = Vec::new();

    for client_info in alsa::seq::ClientIter::new(&seq) {
        let client_id = client_info.get_client();
        let client_name = client_info.get_name().unwrap_or("?").to_string();

        let mut ports = Vec::new();
        for port_info in alsa::seq::PortIter::new(&seq, client_id) {
            let caps = port_info.get_capability();
            let ptype = port_info.get_type();
            ports.push(MidiPort {
                port_id: port_info.get_port(),
                name: port_info.get_name().unwrap_or("?").to_string(),
                capability: format!("{:?}", caps),
                port_type: format!("{:?}", ptype),
                midi_channels: port_info.get_midi_channels(),
                midi_voices: port_info.get_midi_voices(),
                synth_voices: port_info.get_synth_voices(),
                read_use: port_info.get_read_use(),
                write_use: port_info.get_write_use(),
            });
        }

        clients.push(MidiClient {
            client_id,
            name: client_name,
            ports,
        });
    }

    Ok(clients)
}
