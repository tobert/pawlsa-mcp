use serde::Serialize;
use std::collections::HashMap;

use crate::format::Table;

#[derive(Clone, Default, Serialize)]
pub struct PwState {
    pub nodes: HashMap<u32, PwNodeSnapshot>,
    pub ports: HashMap<u32, PwPortSnapshot>,
    pub links: HashMap<u32, PwLinkSnapshot>,
    pub metadata: HashMap<u32, PwMetadataSnapshot>,
    pub devices: HashMap<u32, PwDeviceSnapshot>,
}

#[derive(Clone, Serialize)]
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
}

#[derive(Clone, Default, Serialize)]
pub struct PwMetadataSnapshot {
    pub id: u32,
    pub name: String,
    pub props: HashMap<String, String>,
    pub properties: Vec<MetadataProperty>,
}

#[derive(Clone, Serialize)]
pub struct MetadataProperty {
    pub subject: u32,
    pub key: String,
    pub type_: String,
    pub value: String,
}

#[derive(Clone, Default, Serialize)]
pub struct PwDeviceSnapshot {
    pub id: u32,
    pub props: HashMap<String, String>,
    pub profiles: Vec<PwProfileSnapshot>,
    pub active_profile_index: Option<u32>,
    pub routes: Vec<PwRouteSnapshot>,
    pub active_routes: Vec<u32>,
}

#[derive(Clone, Serialize)]
pub struct PwProfileSnapshot {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub priority: u32,
    pub available: String,
}

#[derive(Clone, Serialize)]
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
        serde_json::to_string_pretty(n).ok()
    }

    pub fn format_ports(&self) -> String {
        let mut t = Table::new(&[
            "id",
            "node_id",
            "node.name",
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
            let node_name = self
                .nodes
                .get(&p.node_id)
                .map(|n| Self::prop(&n.props, "node.name"))
                .unwrap_or_default();
            t.row(&[
                &id,
                &nid,
                &node_name,
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
        // Group properties by subject
        let mut grouped: HashMap<u32, Vec<&MetadataProperty>> = HashMap::new();
        for p in &m.properties {
            grouped.entry(p.subject).or_default().push(p);
        }
        
        #[derive(Serialize)]
        struct SubjectProps<'a> {
            subject: u32,
            properties: Vec<&'a MetadataProperty>,
        }
        
        #[derive(Serialize)]
        struct MetadataDetail<'a> {
            id: u32,
            name: &'a str,
            props: &'a HashMap<String, String>,
            subjects: Vec<SubjectProps<'a>>,
        }
        
        let mut subjects: Vec<_> = grouped.into_iter().map(|(subject, properties)| {
            SubjectProps { subject, properties }
        }).collect();
        subjects.sort_by_key(|s| s.subject);
        
        let detail = MetadataDetail {
            id: m.id,
            name: &m.name,
            props: &m.props,
            subjects,
        };
        
        serde_json::to_string_pretty(&detail).ok()
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
        serde_json::to_string_pretty(d).ok()
    }

    pub fn format_links(&self) -> String {
        let mut t = Table::new(&["id", "state", "out_node", "out_name", "out_port", "in_node", "in_name", "in_port"]);
        let mut links: Vec<_> = self.links.values().collect();
        links.sort_by_key(|l| l.id);
        for l in links {
            let id = l.id.to_string();
            let on = l.output_node_id.to_string();
            let op = l.output_port_id.to_string();
            let in_id = l.input_node_id.to_string();
            let ip = l.input_port_id.to_string();
            
            let out_name = self.nodes.get(&l.output_node_id)
                .map(|n| Self::prop(&n.props, "node.name"))
                .unwrap_or_default();
                
            let in_name = self.nodes.get(&l.input_node_id)
                .map(|n| Self::prop(&n.props, "node.name"))
                .unwrap_or_default();

            t.row(&[&id, &l.state, &on, &out_name, &op, &in_id, &in_name, &ip]);
        }
        t.render()
    }
}
