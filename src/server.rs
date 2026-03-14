use std::sync::{Arc, RwLock};

use rmcp::ErrorData;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
use serde_json::json;

use crate::alsa;
use crate::pw::PwCommand;
use crate::pw::state::PwState;

fn text_resource(text: String, uri: impl Into<String>) -> ResourceContents {
    ResourceContents::TextResourceContents {
        uri: uri.into(),
        mime_type: Some("text/plain".to_string()),
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
        let err = |e: anyhow::Error| ErrorData::internal_error(e.to_string(), None);

        match path {
            "cards" => {
                let text = alsa::cards::list_cards_formatted().map_err(err)?;
                Ok(ReadResourceResult {
                    contents: vec![text_resource(text, "pawlsa://alsa/cards")],
                })
            }
            "midi/ports" => {
                let text = alsa::midi::list_midi_ports().map_err(err)?;
                Ok(ReadResourceResult {
                    contents: vec![text_resource(text, "pawlsa://alsa/midi/ports")],
                })
            }
            _ => {
                if let Some(rest) = path.strip_prefix("cards/") {
                    let index: i32 = rest
                        .parse()
                        .map_err(|_| ErrorData::invalid_params("invalid card index", None))?;
                    let text = alsa::cards::card_detail(index).map_err(err)?;
                    let uri = format!("pawlsa://alsa/cards/{index}");
                    Ok(ReadResourceResult {
                        contents: vec![text_resource(text, uri)],
                    })
                } else if let Some(category) = path.strip_prefix("devices/") {
                    let text = alsa::devices::list_device_hints(category).map_err(err)?;
                    let uri = format!("pawlsa://alsa/devices/{category}");
                    Ok(ReadResourceResult {
                        contents: vec![text_resource(text, uri)],
                    })
                } else if let Some(rest) = path.strip_prefix("mixer/") {
                    let card_index: i32 = rest
                        .parse()
                        .map_err(|_| ErrorData::invalid_params("invalid card index", None))?;
                    let text = alsa::mixer::read_mixer_formatted(card_index).map_err(err)?;
                    let uri = format!("pawlsa://alsa/mixer/{card_index}");
                    Ok(ReadResourceResult {
                        contents: vec![text_resource(text, uri)],
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
            "nodes" => Ok(ReadResourceResult {
                contents: vec![text_resource(st.format_nodes(), "pawlsa://pw/nodes")],
            }),
            "ports" => Ok(ReadResourceResult {
                contents: vec![text_resource(st.format_ports(), "pawlsa://pw/ports")],
            }),
            "links" => Ok(ReadResourceResult {
                contents: vec![text_resource(st.format_links(), "pawlsa://pw/links")],
            }),
            _ => {
                if let Some(rest) = path.strip_prefix("nodes/") {
                    let id: u32 = rest
                        .parse()
                        .map_err(|_| ErrorData::invalid_params("invalid node id", None))?;
                    let text = st.format_node(id).ok_or_else(|| {
                        ErrorData::resource_not_found(format!("pw node {id} not found"), None)
                    })?;
                    let uri = format!("pawlsa://pw/nodes/{id}");
                    Ok(ReadResourceResult {
                        contents: vec![text_resource(text, uri)],
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
            .ok_or_else(|| ErrorData::invalid_params("missing id", None))? as u32;

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

    fn tool_mixer_set_volume(&self, args: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
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

        alsa::mixer::set_volume(card_index, element_name, volume, channel)
            .map_err(|e| ErrorData::internal_error(format!("set_volume: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Volume set: card={card_index} element={element_name} volume={volume}{}",
            channel.map_or(String::new(), |c| format!(" channel={c}"))
        ))]))
    }

    fn tool_mixer_set_switch(&self, args: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
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

        alsa::mixer::set_switch(card_index, element_name, on, channel)
            .map_err(|e| ErrorData::internal_error(format!("set_switch: {e}"), None))?;

        let state_str = if on { "unmuted" } else { "muted" };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switch set: card={card_index} element={element_name} {state_str}{}",
            channel.map_or(String::new(), |c| format!(" channel={c}"))
        ))]))
    }

    async fn tool_play_wav(&self, args: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
        let file_path = args["file_path"]
            .as_str()
            .ok_or_else(|| ErrorData::invalid_params("missing file_path", None))?
            .to_string();
        let device = args
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("pipewire")
            .to_string();

        let path = std::path::PathBuf::from(&file_path);

        let result = tokio::task::spawn_blocking(move || {
            alsa::playback::play_wav_file(&path, &device)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task panicked: {e}"), None))?
        .map_err(|e| ErrorData::internal_error(format!("{e:#}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Played {file_path}: {} frames ({} ms) at {} Hz, {} ch",
            result.frames_played, result.duration_ms, result.sample_rate, result.channels
        ))]))
    }

    async fn tool_play_pcm(&self, args: &serde_json::Value) -> Result<CallToolResult, ErrorData> {
        use base64::Engine;

        let pcm_b64 = args["pcm_samples"]
            .as_str()
            .ok_or_else(|| ErrorData::invalid_params("missing pcm_samples", None))?;
        let sample_rate = args["sample_rate"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing sample_rate", None))?
            as u32;
        let channels = args["channels"]
            .as_u64()
            .ok_or_else(|| ErrorData::invalid_params("missing channels", None))?
            as u16;
        let sample_format = args["sample_format"]
            .as_str()
            .ok_or_else(|| ErrorData::invalid_params("missing sample_format", None))?
            .to_string();
        let device = args
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("pipewire")
            .to_string();

        let pcm_data = base64::engine::general_purpose::STANDARD
            .decode(pcm_b64)
            .map_err(|e| ErrorData::invalid_params(format!("invalid base64: {e}"), None))?;

        let result = tokio::task::spawn_blocking(move || {
            alsa::playback::play_pcm(&pcm_data, sample_rate, channels, &sample_format, &device)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task panicked: {e}"), None))?
        .map_err(|e| ErrorData::internal_error(format!("{e:#}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Played {} frames ({} ms) at {} Hz, {} ch",
            result.frames_played, result.duration_ms, result.sample_rate, result.channels
        ))]))
    }

    // Future: pw_set_node_props via PipeWire metadata interface
}

fn tool_schema(schema: serde_json::Value) -> std::sync::Arc<JsonObject> {
    match schema {
        serde_json::Value::Object(map) => std::sync::Arc::new(map),
        _ => unreachable!(),
    }
}

#[allow(clippy::manual_async_fn)] // ServerHandler trait requires impl Future signatures
impl ServerHandler for PawlsaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities {
                resources: Some(ResourcesCapability {
                    subscribe: None,
                    list_changed: None,
                }),
                tools: Some(ToolsCapability { list_changed: None }),
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
                 Tools for PipeWire link routing and ALSA mixer control. \
                 Tools for audio playback via WAV files or base64-encoded raw PCM data."
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
                RawResource {
                    uri: "pawlsa://alsa/cards".into(),
                    name: "ALSA Sound Cards".into(),
                    title: None,
                    description: Some("All ALSA sound cards with index, name, and longname".into()),
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                }
                .no_annotation(),
                RawResource {
                    uri: "pawlsa://alsa/midi/ports".into(),
                    name: "MIDI Sequencer Clients/Ports".into(),
                    title: None,
                    description: Some(
                        "ALSA sequencer clients and their ports with capabilities and type flags"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                }
                .no_annotation(),
                RawResource {
                    uri: "pawlsa://pw/nodes".into(),
                    name: "PipeWire Nodes".into(),
                    title: None,
                    description: Some(
                        "All PipeWire nodes with state, port counts, and properties \
                         (media.class, node.name, node.description, etc.)"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                }
                .no_annotation(),
                RawResource {
                    uri: "pawlsa://pw/ports".into(),
                    name: "PipeWire Ports".into(),
                    title: None,
                    description: Some(
                        "All PipeWire ports with node_id, direction, and properties".into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                }
                .no_annotation(),
                RawResource {
                    uri: "pawlsa://pw/links".into(),
                    name: "PipeWire Links".into(),
                    title: None,
                    description: Some(
                        "Active PipeWire links showing output/input node and port IDs with state"
                            .into(),
                    ),
                    mime_type: Some("text/plain".into()),
                    size: None,
                    icons: None,
                }
                .no_annotation(),
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
                    description: Some("Detail for ALSA card N (PCM devices, controls)".to_string()),
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
                    "play_wav",
                    "Play a WAV file through an ALSA device",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "file_path": { "type": "string", "description": "Path to a WAV file" },
                            "device": { "type": "string", "description": "ALSA device name (default: 'pipewire')" }
                        },
                        "required": ["file_path"]
                    })),
                )
                .annotate(
                    ToolAnnotations::new()
                        .destructive(false)
                        .open_world(false),
                ),
                Tool::new(
                    "play_pcm",
                    "Play base64-encoded raw PCM samples through an ALSA device",
                    tool_schema(json!({
                        "type": "object",
                        "properties": {
                            "pcm_samples": { "type": "string", "description": "Base64-encoded raw PCM sample data" },
                            "sample_rate": { "type": "integer", "description": "Sample rate in Hz (e.g. 44100, 48000)" },
                            "channels": { "type": "integer", "description": "Number of channels (1 = mono, 2 = stereo)" },
                            "sample_format": {
                                "type": "string",
                                "description": "PCM sample format",
                                "enum": ["s16le", "s32le", "f32le", "f64le"]
                            },
                            "device": { "type": "string", "description": "ALSA device name (default: 'pipewire')" }
                        },
                        "required": ["pcm_samples", "sample_rate", "channels", "sample_format"]
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
                "play_wav" => self.tool_play_wav(&args).await,
                "play_pcm" => self.tool_play_pcm(&args).await,
                _ => Err(ErrorData::invalid_params(
                    format!("unknown tool: {}", request.name),
                    None,
                )),
            }
        }
    }
}
