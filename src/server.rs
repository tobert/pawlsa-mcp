use std::sync::{Arc, RwLock};

use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use rmcp::ErrorData;
use serde_json::json;

use crate::alsa;
use crate::pw::PwCommand;
use crate::pw::state::PwState;

fn json_resource(text: String, uri: impl Into<String>) -> ResourceContents {
    ResourceContents::TextResourceContents {
        uri: uri.into(),
        mime_type: Some("application/json".to_string()),
        text,
        meta: None,
    }
}

pub struct PawlsaServer {
    pw_state: Arc<RwLock<PwState>>,
    pw_cmd: pipewire::channel::Sender<PwCommand>,
}

impl PawlsaServer {
    pub fn new(
        pw_state: Arc<RwLock<PwState>>,
        pw_cmd: pipewire::channel::Sender<PwCommand>,
    ) -> Self {
        Self { pw_state, pw_cmd }
    }

    fn read_alsa_resource(&self, path: &str) -> Result<ReadResourceResult, ErrorData> {
        match path {
            "cards" => {
                let cards = alsa::cards::list_cards()
                    .map_err(|e| ErrorData::internal_error(format!("alsa cards: {e}"), None))?;
                let json = serde_json::to_string_pretty(&cards)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![json_resource(json, "pawlsa://alsa/cards")],
                })
            }
            "midi/ports" => {
                let clients = alsa::midi::list_midi_ports()
                    .map_err(|e| ErrorData::internal_error(format!("alsa midi: {e}"), None))?;
                let json = serde_json::to_string_pretty(&clients)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![json_resource(json, "pawlsa://alsa/midi/ports")],
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
                        contents: vec![json_resource(json, uri)],
                    })
                } else if let Some(category) = path.strip_prefix("devices/") {
                    let hints = alsa::devices::list_device_hints(category).map_err(|e| {
                        ErrorData::internal_error(format!("alsa devices: {e}"), None)
                    })?;
                    let json = serde_json::to_string_pretty(&hints)
                        .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                    let uri = format!("pawlsa://alsa/devices/{category}");
                    Ok(ReadResourceResult {
                        contents: vec![json_resource(json, uri)],
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
                        contents: vec![json_resource(json, uri)],
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
                    contents: vec![json_resource(json, "pawlsa://pw/nodes")],
                })
            }
            "ports" => {
                let ports: Vec<_> = st.ports.values().cloned().collect();
                let json = serde_json::to_string_pretty(&ports)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![json_resource(json, "pawlsa://pw/ports")],
                })
            }
            "links" => {
                let links: Vec<_> = st.links.values().cloned().collect();
                let json = serde_json::to_string_pretty(&links)
                    .map_err(|e| ErrorData::internal_error(format!("serialize: {e}"), None))?;
                Ok(ReadResourceResult {
                    contents: vec![json_resource(json, "pawlsa://pw/links")],
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
                        contents: vec![json_resource(json, uri)],
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

    // -- Tool implementations --

    async fn tool_pw_link_create(
        &self,
        args: &serde_json::Value,
    ) -> Result<CallToolResult, ErrorData> {
        let output_node = args["output_node"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing output_node", None))?
            as u32;
        let output_port = args["output_port"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing output_port", None))?
            as u32;
        let input_node = args["input_node"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing input_node", None))?
            as u32;
        let input_port = args["input_port"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing input_port", None))?
            as u32;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pw_cmd
            .send(PwCommand::CreateLink {
                output_node,
                output_port,
                input_node,
                input_port,
                reply: tx,
            })
            .map_err(|_| ErrorData::internal_error("pw thread not running", None))?;

        let result = rx
            .await
            .map_err(|_| ErrorData::internal_error("pw thread dropped reply", None))?;

        match result {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Link created: id={id} ({output_node}:{output_port} → {input_node}:{input_port})"
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    async fn tool_pw_link_destroy(
        &self,
        args: &serde_json::Value,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args["id"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing id", None))?
            as u32;

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pw_cmd
            .send(PwCommand::DestroyLink { id, reply: tx })
            .map_err(|_| ErrorData::internal_error("pw thread not running", None))?;

        let result = rx
            .await
            .map_err(|_| ErrorData::internal_error("pw thread dropped reply", None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Link {id} destroyed"
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    fn tool_mixer_set_volume(
        &self,
        args: &serde_json::Value,
    ) -> Result<CallToolResult, ErrorData> {
        let card_index = args["card_index"]
            .as_i64()
            .ok_or_else(|| ErrorData::invalid_params("missing card_index", None))?
            as i32;
        let element_name = args["element"]
            .as_str()
            .ok_or_else(|| ErrorData::invalid_params("missing element", None))?;
        let volume = args["volume"]
            .as_i64()
            .ok_or_else(|| ErrorData::invalid_params("missing volume", None))?;
        let channel = args.get("channel").and_then(|v| v.as_str());

        alsa::mixer::set_volume(card_index, element_name, volume, channel).map_err(|e| {
            ErrorData::internal_error(format!("set_volume: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Volume set: card={card_index} element={element_name} volume={volume}{}",
            channel.map_or(String::new(), |c| format!(" channel={c}"))
        ))]))
    }

    fn tool_mixer_set_switch(
        &self,
        args: &serde_json::Value,
    ) -> Result<CallToolResult, ErrorData> {
        let card_index = args["card_index"]
            .as_i64()
            .ok_or_else(|| ErrorData::invalid_params("missing card_index", None))?
            as i32;
        let element_name = args["element"]
            .as_str()
            .ok_or_else(|| ErrorData::invalid_params("missing element", None))?;
        let on = args["on"]
            .as_bool()
            .ok_or_else(|| ErrorData::invalid_params("missing on", None))?;
        let channel = args.get("channel").and_then(|v| v.as_str());

        alsa::mixer::set_switch(card_index, element_name, on, channel).map_err(|e| {
            ErrorData::internal_error(format!("set_switch: {e}"), None)
        })?;

        let state_str = if on { "unmuted" } else { "muted" };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switch set: card={card_index} element={element_name} {state_str}{}",
            channel.map_or(String::new(), |c| format!(" channel={c}"))
        ))]))
    }

    async fn tool_pw_set_node_props(
        &self,
        args: &serde_json::Value,
    ) -> Result<CallToolResult, ErrorData> {
        let id = args["id"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing id", None))?
            as u32;
        let props_val = args
            .get("props")
            .ok_or_else(|| ErrorData::invalid_params("missing props", None))?;
        let props_obj = props_val
            .as_object()
            .ok_or_else(|| ErrorData::invalid_params("props must be an object", None))?;

        let props: std::collections::HashMap<String, String> = props_obj
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_str().map(String::from).unwrap_or_else(|| v.to_string()),
                )
            })
            .collect();

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pw_cmd
            .send(PwCommand::SetNodeProps {
                id,
                props,
                reply: tx,
            })
            .map_err(|_| ErrorData::internal_error("pw thread not running", None))?;

        let result = rx
            .await
            .map_err(|_| ErrorData::internal_error("pw thread dropped reply", None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Node {id} properties updated"
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }
}

fn tool_schema(schema: serde_json::Value) -> std::sync::Arc<JsonObject> {
    match schema {
        serde_json::Value::Object(map) => std::sync::Arc::new(map),
        _ => unreachable!(),
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
                tools: Some(ToolsCapability {
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
                "Linux audio system state: ALSA hardware/MIDI + PipeWire graph. \
                 Tools for PipeWire link routing and ALSA mixer control."
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

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        async {
            let tools = vec![
                Tool::new(
                    "pw_link_create",
                    "Create a PipeWire link between an output port and an input port",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "output_node": { "type": "integer", "description": "Output node ID" },
                            "output_port": { "type": "integer", "description": "Output port ID" },
                            "input_node": { "type": "integer", "description": "Input node ID" },
                            "input_port": { "type": "integer", "description": "Input port ID" }
                        },
                        "required": ["output_node", "output_port", "input_node", "input_port"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(false)
                        .open_world(false),
                ),
                Tool::new(
                    "pw_link_destroy",
                    "Destroy (disconnect) a PipeWire link by its ID",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "Link ID to destroy" }
                        },
                        "required": ["id"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(true)
                        .open_world(false),
                ),
                Tool::new(
                    "mixer_set_volume",
                    "Set the volume of an ALSA mixer element. Use pawlsa://alsa/mixer/{card_index} to discover elements and their volume ranges.",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "card_index": { "type": "integer", "description": "ALSA card index" },
                            "element": { "type": "string", "description": "Mixer element name (e.g. 'Master', 'PCM')" },
                            "volume": { "type": "integer", "description": "Volume value (within the element's range)" },
                            "channel": {
                                "type": "string",
                                "description": "Channel name (e.g. 'front-left', 'front-right'). Omit to set all channels.",
                                "enum": ["front-left", "front-right", "rear-left", "rear-right", "front-center", "woofer", "side-left", "side-right", "rear-center"]
                            }
                        },
                        "required": ["card_index", "element", "volume"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(false)
                        .idempotent(true)
                        .open_world(false),
                ),
                Tool::new(
                    "mixer_set_switch",
                    "Mute or unmute an ALSA mixer element (playback switch). Use pawlsa://alsa/mixer/{card_index} to discover elements.",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "card_index": { "type": "integer", "description": "ALSA card index" },
                            "element": { "type": "string", "description": "Mixer element name (e.g. 'Master', 'PCM')" },
                            "on": { "type": "boolean", "description": "true = unmute, false = mute" },
                            "channel": {
                                "type": "string",
                                "description": "Channel name. Omit to set all channels.",
                                "enum": ["front-left", "front-right", "rear-left", "rear-right", "front-center", "woofer", "side-left", "side-right", "rear-center"]
                            }
                        },
                        "required": ["card_index", "element", "on"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(false)
                        .idempotent(true)
                        .open_world(false),
                ),
                Tool::new(
                    "pw_set_node_props",
                    "Set properties on a PipeWire node (requires metadata interface — may not be supported on all setups)",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "PipeWire node ID" },
                            "props": {
                                "type": "object",
                                "description": "Key-value properties to set",
                                "additionalProperties": { "type": "string" }
                            }
                        },
                        "required": ["id", "props"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(false)
                        .open_world(false),
                ),
            ];
            Ok(ListToolsResult {
                tools,
                next_cursor: None,
            })
        }
    }

    fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl Future<Output = Result<CallToolResult, ErrorData>> + Send + '_ {
        async move {
            let args = serde_json::Value::Object(request.arguments.unwrap_or_default());

            match request.name.as_ref() {
                "pw_link_create" => self.tool_pw_link_create(&args).await,
                "pw_link_destroy" => self.tool_pw_link_destroy(&args).await,
                "mixer_set_volume" => self.tool_mixer_set_volume(&args),
                "mixer_set_switch" => self.tool_mixer_set_switch(&args),
                "pw_set_node_props" => self.tool_pw_set_node_props(&args).await,
                _ => Err(ErrorData::invalid_params(
                    format!("unknown tool: {}", request.name),
                    None,
                )),
            }
        }
    }
}
