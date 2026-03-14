use std::collections::HashMap;

use crate::format::Table;

#[derive(Clone, Default)]
pub struct PwState {
    pub nodes: HashMap<u32, PwNodeSnapshot>,
    pub ports: HashMap<u32, PwPortSnapshot>,
    pub links: HashMap<u32, PwLinkSnapshot>,
}

#[derive(Clone)]
pub struct PwNodeSnapshot {
    pub id: u32,
    pub state: String,
    pub n_input_ports: u32,
    pub n_output_ports: u32,
    pub props: HashMap<String, String>,
}

#[derive(Clone)]
pub struct PwPortSnapshot {
    pub id: u32,
    pub node_id: u32,
    pub direction: String,
    pub props: HashMap<String, String>,
}

#[derive(Clone)]
pub struct PwLinkSnapshot {
    pub id: u32,
    pub output_node_id: u32,
    pub output_port_id: u32,
    pub input_node_id: u32,
    pub input_port_id: u32,
    pub state: String,
}

pub fn dict_to_map(dict: &pipewire::spa::utils::dict::DictRef) -> HashMap<String, String> {
    dict.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

impl PwState {
    fn prop(props: &HashMap<String, String>, key: &str) -> String {
        props.get(key).cloned().unwrap_or_default()
    }

    pub fn format_nodes(&self) -> String {
        let mut t = Table::new(&[
            "id",
            "state",
            "media.class",
            "node.name",
            "node.description",
            "ports(in/out)",
        ]);
        let mut nodes: Vec<_> = self.nodes.values().collect();
        nodes.sort_by_key(|n| n.id);
        for n in nodes {
            let id = n.id.to_string();
            let ports = format!("{}/{}", n.n_input_ports, n.n_output_ports);
            t.row(&[
                &id,
                &n.state,
                &Self::prop(&n.props, "media.class"),
                &Self::prop(&n.props, "node.name"),
                &Self::prop(&n.props, "node.description"),
                &ports,
            ]);
        }
        t.render()
    }

    pub fn format_node(&self, id: u32) -> Option<String> {
        let n = self.nodes.get(&id)?;
        let mut out = format!(
            "id, state, media.class, node.name, node.description, ports(in/out) ▌ \
             {}, {}, {}, {}, {}, {}/{}",
            n.id,
            n.state,
            Self::prop(&n.props, "media.class"),
            Self::prop(&n.props, "node.name"),
            Self::prop(&n.props, "node.description"),
            n.n_input_ports,
            n.n_output_ports,
        );
        // Include all props for single-node detail
        if !n.props.is_empty() {
            out.push_str(" ▌ key, value");
            let mut keys: Vec<_> = n.props.keys().collect();
            keys.sort();
            for k in keys {
                out.push_str(&format!(" ▌ {}, {}", k, n.props[k]));
            }
        }
        Some(out)
    }

    pub fn format_ports(&self) -> String {
        let mut t = Table::new(&[
            "id",
            "node_id",
            "direction",
            "port.name",
            "port.alias",
            "audio.channel",
        ]);
        let mut ports: Vec<_> = self.ports.values().collect();
        ports.sort_by_key(|p| (p.node_id, p.id));
        for p in ports {
            let id = p.id.to_string();
            let nid = p.node_id.to_string();
            t.row(&[
                &id,
                &nid,
                &p.direction,
                &Self::prop(&p.props, "port.name"),
                &Self::prop(&p.props, "port.alias"),
                &Self::prop(&p.props, "audio.channel"),
            ]);
        }
        t.render()
    }

    pub fn format_links(&self) -> String {
        let mut t = Table::new(&["id", "state", "out_node", "out_port", "in_node", "in_port"]);
        let mut links: Vec<_> = self.links.values().collect();
        links.sort_by_key(|l| l.id);
        for l in links {
            let id = l.id.to_string();
            let on = l.output_node_id.to_string();
            let op = l.output_port_id.to_string();
            let in_ = l.input_node_id.to_string();
            let ip = l.input_port_id.to_string();
            t.row(&[&id, &l.state, &on, &op, &in_, &ip]);
        }
        t.render()
    }
}
