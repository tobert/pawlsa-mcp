use anyhow::Result;

use crate::format::Table;

pub fn list_midi_ports() -> Result<String> {
    let seq = alsa::seq::Seq::open(None, None, false)?;
    let mut t = Table::new(&["client", "client_name", "port", "port_name", "caps", "r/w"]);

    for client_info in alsa::seq::ClientIter::new(&seq) {
        let client_id = client_info.get_client();
        let client_name = client_info.get_name().unwrap_or("?").to_string();

        for port_info in alsa::seq::PortIter::new(&seq, client_id) {
            let caps = port_info.get_capability();
            t.row(&[
                &client_id.to_string(),
                &client_name,
                &port_info.get_port().to_string(),
                port_info.get_name().unwrap_or("?"),
                &format!("{:?}", caps),
                &format!("{}/{}", port_info.get_read_use(), port_info.get_write_use()),
            ]);
        }
    }

    Ok(t.render())
}
