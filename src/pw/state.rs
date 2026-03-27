use std::collections::HashMap;

use crate::format::Table;

#[derive(Clone, Default)]
pub struct PwState {
    pub nodes: HashMap<u32, PwNodeSnapshot>,
    pub ports: HashMap<u32, PwPortSnapshot>,
    pub links: HashMap<u32, PwLinkSnapshot>,
    pub metadata: HashMap<u32, PwMetadataSnapshot>,
    pub devices: HashMap<u32, PwDeviceSnapshot>,
}

#[derive(Clone)]
pub struct PwNodeSnapshot {
    pub id: u32,
    pub state: String,
    pub n_input_ports: u32,
    pub n_output_ports: u32,
    pub props: HashMap<String, String>,
    pub volume: Option<f32>,
    pub mute: Option<bool>,
    pub channel_volumes: Option<Vec<f32>>,
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

#[derive(Clone, Default)]
pub struct PwMetadataSnapshot {
    pub id: u32,
    pub name: String,
    pub props: HashMap<String, String>,
    pub properties: Vec<MetadataProperty>,
}

#[derive(Clone)]
pub struct MetadataProperty {
    pub subject: u32,
    pub key: String,
    pub type_: String,
    pub value: String,
}

#[derive(Clone, Default)]
pub struct PwDeviceSnapshot {
    pub id: u32,
    pub props: HashMap<String, String>,
    pub profiles: Vec<PwProfileSnapshot>,
    pub active_profile_index: Option<u32>,
    pub routes: Vec<PwRouteSnapshot>,
    pub active_routes: Vec<u32>,
}

#[derive(Clone)]
pub struct PwProfileSnapshot {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub priority: u32,
    pub available: String,
}

#[derive(Clone)]
pub struct PwRouteSnapshot {
    pub index: u32,
    pub direction: String,
    pub name: String,
    pub description: String,
    pub available: String,
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
            "vol",
            "mute",
        ]);
        let mut nodes: Vec<_> = self.nodes.values().collect();
        nodes.sort_by_key(|n| n.id);
        for n in nodes {
            let id = n.id.to_string();
            let ports = format!("{}/{}", n.n_input_ports, n.n_output_ports);
            let vol = n
                .volume
                .map(|v| format!("{v:.3}"))
                .unwrap_or_default();
            let mute = n
                .mute
                .map(|m| m.to_string())
                .unwrap_or_default();
            t.row(&[
                &id,
                &n.state,
                &Self::prop(&n.props, "media.class"),
                &Self::prop(&n.props, "node.name"),
                &Self::prop(&n.props, "node.description"),
                &ports,
                &vol,
                &mute,
            ]);
        }
        t.render()
    }

    pub fn format_node(&self, id: u32) -> Option<String> {
        let n = self.nodes.get(&id)?;
        let vol = n.volume.map(|v| format!("{v:.3}")).unwrap_or_default();
        let mute = n.mute.map(|m| m.to_string()).unwrap_or_default();
        let mut out = format!(
            "id, state, media.class, node.name, node.description, ports(in/out), vol, mute ▌ \
             {}, {}, {}, {}, {}, {}/{}, {}, {}",
            n.id,
            n.state,
            Self::prop(&n.props, "media.class"),
            Self::prop(&n.props, "node.name"),
            Self::prop(&n.props, "node.description"),
            n.n_input_ports,
            n.n_output_ports,
            vol,
            mute,
        );
        // Channel volumes
        if let Some(ref cvols) = n.channel_volumes {
            let formatted: Vec<String> = cvols.iter().map(|v| format!("{v:.3}")).collect();
            out.push_str(&format!(" ▌ channel_volumes: [{}]", formatted.join(", ")));
        }
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

    pub fn format_metadata(&self) -> String {
        let mut t = Table::new(&["id", "metadata.name", "properties"]);
        let mut metas: Vec<_> = self.metadata.values().collect();
        metas.sort_by_key(|m| m.id);
        for m in metas {
            let id = m.id.to_string();
            let count = m.properties.len().to_string();
            t.row(&[&id, &m.name, &count]);
        }
        t.render()
    }

    pub fn format_metadata_detail(&self, id: u32) -> Option<String> {
        let m = self.metadata.get(&id)?;
        let mut out = format!(
            "id, metadata.name, properties ▌ {}, {}, {}",
            m.id,
            m.name,
            m.properties.len()
        );
        if !m.properties.is_empty() {
            out.push_str(" ▌ subject, key, type, value");
            let mut props = m.properties.clone();
            props.sort_by(|a, b| a.subject.cmp(&b.subject).then(a.key.cmp(&b.key)));
            for p in &props {
                out.push_str(&format!(
                    " ▌ {}, {}, {}, {}",
                    p.subject, p.key, p.type_, p.value
                ));
            }
        }
        Some(out)
    }

    pub fn format_devices(&self) -> String {
        let mut t = Table::new(&[
            "id",
            "device.name",
            "media.class",
            "active_profile",
            "profiles",
            "routes",
        ]);
        let mut devs: Vec<_> = self.devices.values().collect();
        devs.sort_by_key(|d| d.id);
        for d in devs {
            let id = d.id.to_string();
            let active = d
                .active_profile_index
                .and_then(|idx| d.profiles.iter().find(|p| p.index == idx))
                .map(|p| p.description.clone())
                .unwrap_or_else(|| {
                    d.active_profile_index
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "N/A".to_string())
                });
            let n_profiles = d.profiles.len().to_string();
            let n_routes = d.routes.len().to_string();
            t.row(&[
                &id,
                &Self::prop(&d.props, "device.name"),
                &Self::prop(&d.props, "media.class"),
                &active,
                &n_profiles,
                &n_routes,
            ]);
        }
        t.render()
    }

    pub fn format_device(&self, id: u32) -> Option<String> {
        let d = self.devices.get(&id)?;
        let active_desc = d
            .active_profile_index
            .and_then(|idx| d.profiles.iter().find(|p| p.index == idx))
            .map(|p| p.description.clone())
            .unwrap_or_else(|| "N/A".to_string());
        let mut out = format!(
            "id, device.name, media.class, active_profile ▌ {}, {}, {}, {}",
            d.id,
            Self::prop(&d.props, "device.name"),
            Self::prop(&d.props, "media.class"),
            active_desc,
        );
        // Props
        if !d.props.is_empty() {
            out.push_str(" ▌ key, value");
            let mut keys: Vec<_> = d.props.keys().collect();
            keys.sort();
            for k in keys {
                out.push_str(&format!(" ▌ {}, {}", k, d.props[k]));
            }
        }
        // Profiles
        if !d.profiles.is_empty() {
            out.push_str(" ▌ --- profiles --- ▌ index, name, description, priority, available, active");
            let mut profiles = d.profiles.clone();
            profiles.sort_by_key(|p| p.index);
            for p in &profiles {
                let active = d.active_profile_index == Some(p.index);
                out.push_str(&format!(
                    " ▌ {}, {}, {}, {}, {}, {}",
                    p.index, p.name, p.description, p.priority, p.available, active
                ));
            }
        }
        // Routes
        if !d.routes.is_empty() {
            out.push_str(" ▌ --- routes --- ▌ index, direction, name, description, available, active");
            let mut routes = d.routes.clone();
            routes.sort_by_key(|r| r.index);
            for r in &routes {
                let active = d.active_routes.contains(&r.index);
                out.push_str(&format!(
                    " ▌ {}, {}, {}, {}, {}, {}",
                    r.index, r.direction, r.name, r.description, r.available, active
                ));
            }
        }
        Some(out)
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
