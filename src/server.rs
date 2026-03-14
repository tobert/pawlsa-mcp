use std::sync::{Arc, RwLock};

use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::ErrorData;

use crate::alsa;
use crate::pw::state::PwState;

pub struct PawlsaServer {
    pw_state: Arc<RwLock<PwState>>,
}

impl PawlsaServer {
    pub fn new(pw_state: Arc<RwLock<PwState>>) -> Self {
        Self { pw_state }
    }

    fn read_alsa_resource(&self, path: &str) -> Result<ReadResourceResult, ErrorData> {
        match path {
            "cards" => {
                let cards = alsa::cards::list_cards()
                    .map_err(|e| ErrorData::internal_error(format!("alsa cards: {e}"), None))?;
                let json = serde_json::to_string_pretty(&cards)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(json, "pawlsa://alsa/cards")],
                })
            }
            "midi/ports" => {
                let clients = alsa::midi::list_midi_ports()
                    .map_err(|e| ErrorData::internal_error(format!("alsa midi: {e}"), None))?;
                let json = serde_json::to_string_pretty(&clients)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(json, "pawlsa://alsa/midi/ports")],
                })
            }
            _ => {
                if let Some(rest) = path.strip_prefix("cards/") {
                    let index: i32 = rest.parse().map_err(|_| {
                        ErrorData::invalid_params("invalid card index", None)
                    })?;
                    let detail = alsa::cards::card_detail(index).map_err(|e| {
                        ErrorData::internal_error(format!("alsa card detail: {e}"), None)
                    })?;
                    let json = serde_json::to_string_pretty(&detail)
                        .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                    let uri = format!("pawlsa://alsa/cards/{index}");
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(json, uri)],
                    })
                } else if let Some(category) = path.strip_prefix("devices/") {
                    let hints = alsa::devices::list_device_hints(category).map_err(|e| {
                        ErrorData::internal_error(format!("alsa devices: {e}"), None)
                    })?;
                    let json = serde_json::to_string_pretty(&hints)
                        .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                    let uri = format!("pawlsa://alsa/devices/{category}");
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(json, uri)],
                    })
                } else if let Some(rest) = path.strip_prefix("mixer/") {
                    let card_index: i32 = rest.parse().map_err(|_| {
                        ErrorData::invalid_params("invalid card index", None)
                    })?;
                    let elements = alsa::mixer::read_mixer(card_index).map_err(|e| {
                        ErrorData::internal_error(format!("alsa mixer: {e}"), None)
                    })?;
                    let json = serde_json::to_string_pretty(&elements)
                        .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                    let uri = format!("pawlsa://alsa/mixer/{card_index}");
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(json, uri)],
                    })
                } else {
                    Err(ErrorData::resource_not_found(
                        format!("unknown alsa resource: {path}"),
                        None,
                    ))
                }
            }
        }
    }

    fn read_pw_resource(&self, path: &str) -> Result<ReadResourceResult, ErrorData> {
        let st = self.pw_state.read().unwrap();

        match path {
            "nodes" => {
                let nodes: Vec<_> = st.nodes.values().cloned().collect();
                let json = serde_json::to_string_pretty(&nodes)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(json, "pawlsa://pw/nodes")],
                })
            }
            "ports" => {
                let ports: Vec<_> = st.ports.values().cloned().collect();
                let json = serde_json::to_string_pretty(&ports)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(json, "pawlsa://pw/ports")],
                })
            }
            "links" => {
                let links: Vec<_> = st.links.values().cloned().collect();
                let json = serde_json::to_string_pretty(&links)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(json, "pawlsa://pw/links")],
                })
            }
            _ => {
                if let Some(rest) = path.strip_prefix("nodes/") {
                    let id: u32 = rest.parse().map_err(|_| {
                        ErrorData::invalid_params("invalid node id", None)
                    })?;
                    let node = st.nodes.get(&id).ok_or_else(|| {
                        ErrorData::resource_not_found(format!("pw node {id} not found"), None)
                    })?;
                    let json = serde_json::to_string_pretty(node)
                        .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                    let uri = format!("pawlsa://pw/nodes/{id}");
                    Ok(ReadResourceResult {
                        contents: vec![ResourceContents::text(json, uri)],
                    })
                } else {
                    Err(ErrorData::resource_not_found(
                        format!("unknown pw resource: {path}"),
                        None,
                    ))
                }
            }
        }
    }
}

impl ServerHandler for PawlsaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities {
                resources: Some(ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "pawlsa-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Read-only Linux audio system state: ALSA hardware/MIDI + PipeWire graph"
                    .to_string(),
            ),
            ..Default::default()
        }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl Future<Output = Result<ListResourcesResult, ErrorData>> + Send + '_ {
        async {
            let resources = vec![
                RawResource::new("pawlsa://alsa/cards", "ALSA Sound Cards").no_annotation(),
                RawResource::new("pawlsa://alsa/midi/ports", "MIDI Sequencer Clients/Ports")
                    .no_annotation(),
                RawResource::new("pawlsa://pw/nodes", "PipeWire Nodes").no_annotation(),
                RawResource::new("pawlsa://pw/ports", "PipeWire Ports").no_annotation(),
                RawResource::new("pawlsa://pw/links", "PipeWire Links").no_annotation(),
            ];
            Ok(ListResourcesResult {
                resources,
                next_cursor: None,
            })
        }
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl Future<Output = Result<ListResourceTemplatesResult, ErrorData>> + Send + '_ {
        async {
            let templates = vec![
                RawResourceTemplate {
                    uri_template: "pawlsa://alsa/cards/{index}".to_string(),
                    name: "ALSA Card Detail".to_string(),
                    title: None,
                    description: Some(
                        "Detail for ALSA card N (PCM devices, controls)".to_string(),
                    ),
                    mime_type: Some("application/json".to_string()),
                }
                .no_annotation(),
                RawResourceTemplate {
                    uri_template: "pawlsa://alsa/devices/{category}".to_string(),
                    name: "ALSA Device Hints".to_string(),
                    title: None,
                    description: Some(
                        "Device hints for category (pcm, rawmidi, seq, ctl)".to_string(),
                    ),
                    mime_type: Some("application/json".to_string()),
                }
                .no_annotation(),
                RawResourceTemplate {
                    uri_template: "pawlsa://alsa/mixer/{card_index}".to_string(),
                    name: "ALSA Mixer Elements".to_string(),
                    title: None,
                    description: Some(
                        "Mixer elements (volume, mute, switches) for a card".to_string(),
                    ),
                    mime_type: Some("application/json".to_string()),
                }
                .no_annotation(),
                RawResourceTemplate {
                    uri_template: "pawlsa://pw/nodes/{id}".to_string(),
                    name: "PipeWire Node Detail".to_string(),
                    title: None,
                    description: Some("Detail for a specific PipeWire node".to_string()),
                    mime_type: Some("application/json".to_string()),
                }
                .no_annotation(),
            ];
            Ok(ListResourceTemplatesResult {
                resource_templates: templates,
                next_cursor: None,
            })
        }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl Future<Output = Result<ReadResourceResult, ErrorData>> + Send + '_ {
        async move {
            let uri = &request.uri;
            let path = uri.strip_prefix("pawlsa://").ok_or_else(|| {
                ErrorData::invalid_params(format!("unknown URI scheme: {uri}"), None)
            })?;

            if let Some(alsa_path) = path.strip_prefix("alsa/") {
                self.read_alsa_resource(alsa_path)
            } else if let Some(pw_path) = path.strip_prefix("pw/") {
                self.read_pw_resource(pw_path)
            } else {
                Err(ErrorData::resource_not_found(
                    format!("unknown resource: {uri}"),
                    None,
                ))
            }
        }
    }
}
