use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Serialize, Default)]
pub struct PwState {
    pub nodes: HashMap<u32, PwNodeSnapshot>,
    pub ports: HashMap<u32, PwPortSnapshot>,
    pub links: HashMap<u32, PwLinkSnapshot>,
}

#[derive(Clone, Serialize)]
pub struct PwNodeSnapshot {
    pub id: u32,
    pub state: String,
    pub max_input_ports: u32,
    pub max_output_ports: u32,
    pub n_input_ports: u32,
    pub n_output_ports: u32,
    pub props: HashMap<String, String>,
}

#[derive(Clone, Serialize)]
pub struct PwPortSnapshot {
    pub id: u32,
    pub node_id: u32,
    pub direction: String,
    pub props: HashMap<String, String>,
}

#[derive(Clone, Serialize)]
pub struct PwLinkSnapshot {
    pub id: u32,
    pub output_node_id: u32,
    pub output_port_id: u32,
    pub input_node_id: u32,
    pub input_port_id: u32,
    pub state: String,
    pub props: HashMap<String, String>,
}

pub fn dict_to_map(dict: &pipewire::spa::utils::dict::DictRef) -> HashMap<String, String> {
    dict.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
