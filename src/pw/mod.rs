pub mod state;

use state::{
    PwDeviceSnapshot, PwLinkSnapshot, PwMetadataSnapshot, PwNodeSnapshot, PwPortSnapshot, PwState,
    dict_to_map,
};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use pipewire as pw;
use pw::proxy::{Listener, ProxyT};
use pw::types::ObjectType;

// -- Command channel types --

pub enum PwCommand {
    CreateLink {
        output_node: u32,
        output_port: u32,
        input_node: u32,
        input_port: u32,
        reply: tokio::sync::oneshot::Sender<Result<u32, String>>,
    },
    DestroyLink {
        id: u32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetMetadataProperty {
        metadata_id: u32,
        subject: u32,
        key: String,
        type_: String,
        value: Option<String>,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetDeviceProfile {
        device_id: u32,
        profile_index: u32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetNodeVolume {
        node_id: u32,
        volume: f32,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    SetNodeMute {
        node_id: u32,
        mute: bool,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}

pub struct PwHandle {
    pub state: Arc<RwLock<PwState>>,
    pub cmd_tx: pw::channel::Sender<PwCommand>,
    #[allow(dead_code)]
    pub join_handle: JoinHandle<()>,
}

pub fn spawn_pw_thread() -> PwHandle {
    let state = Arc::new(RwLock::new(PwState::default()));
    let state_clone = state.clone();

    let (cmd_tx, cmd_rx) = pw::channel::channel::<PwCommand>();

    let join_handle = std::thread::spawn(move || {
        run_pw_loop(state_clone, cmd_rx);
    });

    PwHandle {
        state,
        cmd_tx,
        join_handle,
    }
}

struct Proxies {
    /// Type-erased proxies for objects we only need to keep alive (Port, Link, created Links)
    proxies_t: HashMap<u32, Box<dyn ProxyT>>,
    listeners: HashMap<u32, Vec<Box<dyn Listener>>>,
}

impl Proxies {
    fn new() -> Self {
        Self {
            proxies_t: HashMap::new(),
            listeners: HashMap::new(),
        }
    }

    fn add(&mut self, proxy_t: Box<dyn ProxyT>, listener: Box<dyn Listener>) {
        let proxy_id = proxy_t.upcast_ref().id();
        self.proxies_t.insert(proxy_id, proxy_t);
        self.listeners.entry(proxy_id).or_default().push(listener);
    }

    fn add_proxy_only(&mut self, proxy_t: Box<dyn ProxyT>) {
        let proxy_id = proxy_t.upcast_ref().id();
        self.proxies_t.insert(proxy_id, proxy_t);
    }

    /// Store only a listener — proxy lifetime managed by a typed map
    fn add_listener(&mut self, id: u32, listener: Box<dyn Listener>) {
        self.listeners.entry(id).or_default().push(listener);
    }

    fn remove(&mut self, proxy_id: u32) {
        self.proxies_t.remove(&proxy_id);
        self.listeners.remove(&proxy_id);
    }
}

/// Concrete-typed proxy maps for objects we need to call methods on
/// (subscribe_params, set_param, set_property, etc.)
/// These are Rc<RefCell<>> because they live entirely on the PW thread.
struct TypedProxies {
    nodes: HashMap<u32, pw::node::Node>,
    metadata: HashMap<u32, pw::metadata::Metadata>,
    devices: HashMap<u32, pw::device::Device>,
}

impl TypedProxies {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            metadata: HashMap::new(),
            devices: HashMap::new(),
        }
    }

    fn remove(&mut self, id: u32) {
        self.nodes.remove(&id);
        self.metadata.remove(&id);
        self.devices.remove(&id);
    }
}

fn run_pw_loop(state: Arc<RwLock<PwState>>, cmd_rx: pw::channel::Receiver<PwCommand>) {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).expect("failed to create PW MainLoop");
    let context =
        pw::context::ContextRc::new(&mainloop, None).expect("failed to create PW Context");
    let core = context.connect_rc(None).expect("failed to connect PW Core");
    let registry = core.get_registry_rc().expect("failed to get PW Registry");

    let registry_weak = registry.downgrade();
    let proxies: Rc<RefCell<Proxies>> = Rc::new(RefCell::new(Proxies::new()));
    let typed: Rc<RefCell<TypedProxies>> = Rc::new(RefCell::new(TypedProxies::new()));

    // -- Command receiver --
    let core_for_cmd = core.clone();
    let registry_for_cmd = registry.clone();
    let proxies_for_cmd = proxies.clone();
    let typed_for_cmd = typed.clone();
    let _cmd_receiver = cmd_rx.attach(mainloop.loop_(), move |cmd| {
        handle_command(
            cmd,
            &core_for_cmd,
            &registry_for_cmd,
            &proxies_for_cmd,
            &typed_for_cmd,
        );
    });

    // -- Registry listener --
    let state_remove = state.clone();
    let proxies_remove = proxies.clone();
    let typed_remove = typed.clone();
    let typed_for_reg = typed.clone();

    let _registry_listener = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(registry) = registry_weak.upgrade() else {
                return;
            };

            let id = obj.id;
            match obj.type_ {
                ObjectType::Node => {
                    if let Some((node, listener)) = bind_node(&registry, obj, &state) {
                        typed_for_reg.borrow_mut().nodes.insert(id, node);
                        proxies.borrow_mut().add_listener(id, listener);
                    }
                }
                ObjectType::Port => {
                    if let Some((proxy_t, listener)) = bind_port(&registry, obj, &state) {
                        proxies.borrow_mut().add(proxy_t, listener);
                    }
                }
                ObjectType::Link => {
                    if let Some((proxy_t, listener)) = bind_link(&registry, obj, &state) {
                        proxies.borrow_mut().add(proxy_t, listener);
                    }
                }
                ObjectType::Metadata => {
                    if let Some((metadata, listener)) =
                        bind_metadata(&registry, obj, &state)
                    {
                        typed_for_reg.borrow_mut().metadata.insert(id, metadata);
                        proxies.borrow_mut().add_listener(id, listener);
                    }
                }
                ObjectType::Device => {
                    if let Some((device, listener)) =
                        bind_device(&registry, obj, &state)
                    {
                        typed_for_reg.borrow_mut().devices.insert(id, device);
                        proxies.borrow_mut().add_listener(id, listener);
                    }
                }
                _ => {
                    tracing::trace!(id, type_ = ?obj.type_, "ignoring global");
                }
            }
        })
        .global_remove(move |id| {
            proxies_remove.borrow_mut().remove(id);
            typed_remove.borrow_mut().remove(id);
            let mut st = state_remove.write().unwrap();
            st.nodes.remove(&id);
            st.ports.remove(&id);
            st.links.remove(&id);
            st.metadata.remove(&id);
            st.devices.remove(&id);
        })
        .register();

    mainloop.run();
}

fn handle_command(
    cmd: PwCommand,
    core: &pw::core::CoreRc,
    registry: &pw::registry::RegistryRc,
    proxies: &Rc<RefCell<Proxies>>,
    typed: &Rc<RefCell<TypedProxies>>,
) {
    match cmd {
        PwCommand::CreateLink {
            output_node,
            output_port,
            input_node,
            input_port,
            reply,
        } => {
            let props = pw::properties::properties! {
                "link.output.node" => output_node.to_string(),
                "link.output.port" => output_port.to_string(),
                "link.input.node" => input_node.to_string(),
                "link.input.port" => input_port.to_string(),
                "object.linger" => "true"
            };
            match core.create_object::<pw::link::Link>("link-factory", &props) {
                Ok(link) => {
                    let id = link.upcast_ref().id();
                    // Keep the proxy alive so the link persists
                    proxies.borrow_mut().add_proxy_only(Box::new(link));
                    let _ = reply.send(Ok(id));
                }
                Err(e) => {
                    let _ = reply.send(Err(format!("create_object failed: {e}")));
                }
            }
        }
        PwCommand::DestroyLink { id, reply } => {
            let result = registry.destroy_global(id);
            match result.into_result() {
                Ok(_) => {
                    proxies.borrow_mut().remove(id);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(format!("destroy_global failed: {e}")));
                }
            }
        }
        PwCommand::SetMetadataProperty {
            metadata_id,
            subject,
            key,
            type_,
            value,
            reply,
        } => {
            let typed = typed.borrow();
            if let Some(metadata) = typed.metadata.get(&metadata_id) {
                metadata.set_property(
                    subject,
                    &key,
                    Some(type_.as_str()),
                    value.as_deref(),
                );
                let _ = reply.send(Ok(()));
            } else {
                let _ = reply.send(Err(format!("metadata {metadata_id} not found")));
            }
        }
        PwCommand::SetDeviceProfile {
            device_id,
            profile_index,
            reply,
        } => {
            let typed = typed.borrow();
            if let Some(device) = typed.devices.get(&device_id) {
                let pod_bytes = build_profile_pod(profile_index);
                let pod = pw::spa::pod::Pod::from_bytes(&pod_bytes);
                match pod {
                    Some(pod) => {
                        device.set_param(
                            pw::spa::param::ParamType::Profile,
                            0,
                            pod,
                        );
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err("failed to build profile pod".to_string()));
                    }
                }
            } else {
                let _ = reply.send(Err(format!("device {device_id} not found")));
            }
        }
        PwCommand::SetNodeVolume {
            node_id,
            volume,
            reply,
        } => {
            let typed = typed.borrow();
            if let Some(node) = typed.nodes.get(&node_id) {
                let pod_bytes = build_volume_pod(volume);
                match pw::spa::pod::Pod::from_bytes(&pod_bytes) {
                    Some(pod) => {
                        node.set_param(pw::spa::param::ParamType::Props, 0, pod);
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err("failed to build volume pod".to_string()));
                    }
                }
            } else {
                let _ = reply.send(Err(format!("node {node_id} not found")));
            }
        }
        PwCommand::SetNodeMute {
            node_id,
            mute,
            reply,
        } => {
            let typed = typed.borrow();
            if let Some(node) = typed.nodes.get(&node_id) {
                let pod_bytes = build_mute_pod(mute);
                match pw::spa::pod::Pod::from_bytes(&pod_bytes) {
                    Some(pod) => {
                        node.set_param(pw::spa::param::ParamType::Props, 0, pod);
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err("failed to build mute pod".to_string()));
                    }
                }
            } else {
                let _ = reply.send(Err(format!("node {node_id} not found")));
            }
        }
    }
}

type DictRef = pw::spa::utils::dict::DictRef;

fn bind_node(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(pw::node::Node, Box<dyn Listener>)> {
    let id = obj.id;

    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();
    {
        let mut st = state.write().unwrap();
        st.nodes.insert(
            id,
            PwNodeSnapshot {
                id,
                state: "unknown".to_string(),
                n_input_ports: 0,
                n_output_ports: 0,
                props: initial_props,
                volume: None,
                mute: None,
                channel_volumes: None,
            },
        );
    }

    let node: pw::node::Node = registry.bind(obj).ok()?;

    let state_info = state.clone();
    let state_param = state.clone();
    let listener = node
        .add_listener_local()
        .info(move |info| {
            let node_state = match info.state() {
                pw::node::NodeState::Error(e) => format!("error: {e}"),
                pw::node::NodeState::Creating => "creating".to_string(),
                pw::node::NodeState::Suspended => "suspended".to_string(),
                pw::node::NodeState::Idle => "idle".to_string(),
                pw::node::NodeState::Running => "running".to_string(),
            };
            let props = info.props().map(dict_to_map).unwrap_or_default();
            let mut st = state_info.write().unwrap();
            // Preserve volume/mute from param events across info updates
            let (vol, mute, cvols) = st
                .nodes
                .get(&id)
                .map(|n| (n.volume, n.mute, n.channel_volumes.clone()))
                .unwrap_or_default();
            st.nodes.insert(
                id,
                PwNodeSnapshot {
                    id,
                    state: node_state,
                    n_input_ports: info.n_input_ports(),
                    n_output_ports: info.n_output_ports(),
                    props,
                    volume: vol,
                    mute,
                    channel_volumes: cvols,
                },
            );
        })
        .param(move |_seq, param_id, _index, _next, param| {
            if param_id == pw::spa::param::ParamType::Props
                && let Some(pod) = param
            {
                parse_node_props(pod, &state_param, id);
            }
        })
        .register();

    // Subscribe to Props param changes and enumerate initial values
    node.subscribe_params(&[pw::spa::param::ParamType::Props]);
    node.enum_params(0, Some(pw::spa::param::ParamType::Props), 0, u32::MAX);

    Some((node, Box::new(listener)))
}

fn parse_node_props(pod: &pw::spa::pod::Pod, state: &Arc<RwLock<PwState>>, node_id: u32) {
    let Ok(obj) = pod.as_object() else {
        return;
    };

    let mut volume: Option<f32> = None;
    let mut mute: Option<bool> = None;
    let mut channel_volumes: Option<Vec<f32>> = None;

    for prop in obj.props() {
        match prop.key().0 {
            pw::spa::sys::SPA_PROP_volume => {
                volume = prop.value().get_float().ok();
            }
            pw::spa::sys::SPA_PROP_mute => {
                mute = prop.value().get_bool().ok();
            }
            pw::spa::sys::SPA_PROP_channelVolumes if prop.value().is_array() => {
                // channelVolumes is an array of f32 pods
                channel_volumes = parse_float_array(prop.value());
            }
            _ => {}
        }
    }

    let mut st = state.write().unwrap();
    if let Some(node) = st.nodes.get_mut(&node_id) {
        if let Some(v) = volume {
            node.volume = Some(v);
        }
        if let Some(m) = mute {
            node.mute = Some(m);
        }
        if let Some(cv) = channel_volumes {
            node.channel_volumes = Some(cv);
        }
    }
}

/// Parse a SPA array pod of floats into Vec<f32>.
fn parse_float_array(pod: &pw::spa::pod::Pod) -> Option<Vec<f32>> {
    // The pod is an Array type. Its body starts with a child type header
    // followed by the repeated child elements. We use spa_pod_get_float
    // on each child via the SPA iteration macros... but since libspa-rs
    // doesn't wrap array iteration, we'll use the raw bytes approach.
    //
    // Array pod layout: spa_pod header (8 bytes) + spa_pod child_header (8 bytes) + N * child_size
    // For f32 children: child type = SPA_TYPE_Float (6), child_size = 4
    let bytes = pod.as_raw_ptr();
    unsafe {
        let pod_raw = &*bytes;
        // Check it really is an array
        if pod_raw.type_ != pw::spa::sys::SPA_TYPE_Array {
            return None;
        }
        let child_header_size = std::mem::size_of::<pw::spa::sys::spa_pod>();
        // Body must be large enough to contain the child type header
        if (pod_raw.size as usize) < child_header_size {
            return None;
        }
        let body_ptr = (bytes as *const u8).add(std::mem::size_of::<pw::spa::sys::spa_pod>());
        // First 8 bytes of body is the child pod header (type + size)
        let child_pod = &*(body_ptr as *const pw::spa::sys::spa_pod);
        if child_pod.type_ != pw::spa::sys::SPA_TYPE_Float {
            return None;
        }
        let child_size = child_pod.size as usize;
        if child_size != 4 {
            return None;
        }
        // Data starts after child header
        let data_ptr = body_ptr.add(child_header_size);
        let total_data = pod_raw.size as usize - child_header_size;
        let n_elements = total_data / child_size;
        let mut result = Vec::with_capacity(n_elements);
        for i in 0..n_elements {
            let float_ptr = data_ptr.add(i * child_size) as *const f32;
            result.push(*float_ptr);
        }
        Some(result)
    }
}

fn bind_port(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(Box<dyn ProxyT>, Box<dyn Listener>)> {
    let id = obj.id;

    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();
    let node_id: u32 = initial_props
        .get("node.id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let direction = initial_props
        .get("port.direction")
        .cloned()
        .unwrap_or_default();
    {
        let mut st = state.write().unwrap();
        st.ports.insert(
            id,
            PwPortSnapshot {
                id,
                node_id,
                direction,
                props: initial_props,
            },
        );
    }

    let port: pw::port::Port = registry.bind(obj).ok()?;

    let state = state.clone();
    let listener = port
        .add_listener_local()
        .info(move |info| {
            let props = info.props().map(dict_to_map).unwrap_or_default();
            let node_id: u32 = props
                .get("node.id")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let dir = match info.direction() {
                pw::spa::utils::Direction::Input => "input",
                pw::spa::utils::Direction::Output => "output",
                _ => "unknown",
            };
            let mut st = state.write().unwrap();
            st.ports.insert(
                id,
                PwPortSnapshot {
                    id,
                    node_id,
                    direction: dir.to_string(),
                    props,
                },
            );
        })
        .register();

    Some((Box::new(port), Box::new(listener)))
}

fn bind_link(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(Box<dyn ProxyT>, Box<dyn Listener>)> {
    let id = obj.id;

    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();
    let parse_prop = |key: &str| -> u32 {
        initial_props
            .get(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    {
        let mut st = state.write().unwrap();
        st.links.insert(
            id,
            PwLinkSnapshot {
                id,
                output_node_id: parse_prop("link.output.node"),
                output_port_id: parse_prop("link.output.port"),
                input_node_id: parse_prop("link.input.node"),
                input_port_id: parse_prop("link.input.port"),
                state: "unknown".to_string(),
            },
        );
    }

    let link: pw::link::Link = registry.bind(obj).ok()?;

    let state = state.clone();
    let listener = link
        .add_listener_local()
        .info(move |info| {
            let link_state = match info.state() {
                pw::link::LinkState::Error(e) => format!("error: {e}"),
                pw::link::LinkState::Unlinked => "unlinked".to_string(),
                pw::link::LinkState::Init => "init".to_string(),
                pw::link::LinkState::Negotiating => "negotiating".to_string(),
                pw::link::LinkState::Allocating => "allocating".to_string(),
                pw::link::LinkState::Paused => "paused".to_string(),
                pw::link::LinkState::Active => "active".to_string(),
            };
            let mut st = state.write().unwrap();
            st.links.insert(
                id,
                PwLinkSnapshot {
                    id,
                    output_node_id: info.output_node_id(),
                    output_port_id: info.output_port_id(),
                    input_node_id: info.input_node_id(),
                    input_port_id: info.input_port_id(),
                    state: link_state,
                },
            );
        })
        .register();

    Some((Box::new(link), Box::new(listener)))
}

fn bind_metadata(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(pw::metadata::Metadata, Box<dyn Listener>)> {
    let id = obj.id;
    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();
    let name = initial_props
        .get("metadata.name")
        .cloned()
        .unwrap_or_default();

    {
        let mut st = state.write().unwrap();
        st.metadata.insert(
            id,
            PwMetadataSnapshot {
                id,
                name: name.clone(),
                props: initial_props,
                properties: Vec::new(),
            },
        );
    }

    let metadata: pw::metadata::Metadata = registry.bind(obj).ok()?;

    let state = state.clone();
    let listener = metadata
        .add_listener_local()
        .property(move |subject, key, type_, value| {
            let mut st = state.write().unwrap();
            if let Some(snap) = st.metadata.get_mut(&id) {
                match key {
                    None => {
                        // All properties removed for this subject
                        snap.properties.retain(|p| p.subject != subject);
                    }
                    Some(key) => match value {
                        None => {
                            // Single property removed
                            snap.properties
                                .retain(|p| !(p.subject == subject && p.key == key));
                        }
                        Some(value) => {
                            let type_str = type_.unwrap_or("").to_string();
                            // Update existing or insert new
                            if let Some(existing) = snap
                                .properties
                                .iter_mut()
                                .find(|p| p.subject == subject && p.key == key)
                            {
                                existing.type_ = type_str;
                                existing.value = value.to_string();
                            } else {
                                snap.properties.push(state::MetadataProperty {
                                    subject,
                                    key: key.to_string(),
                                    type_: type_str,
                                    value: value.to_string(),
                                });
                            }
                        }
                    },
                }
            }
            0
        })
        .register();

    tracing::debug!(id, name = %name, "bound metadata");
    Some((metadata, Box::new(listener)))
}

fn bind_device(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(pw::device::Device, Box<dyn Listener>)> {
    let id = obj.id;
    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();

    {
        let mut st = state.write().unwrap();
        st.devices.insert(
            id,
            PwDeviceSnapshot {
                id,
                props: initial_props,
                ..Default::default()
            },
        );
    }

    let device: pw::device::Device = registry.bind(obj).ok()?;

    let state_info = state.clone();
    let state_param = state.clone();
    let listener = device
        .add_listener_local()
        .info(move |info| {
            let props = info.props().map(dict_to_map).unwrap_or_default();
            let mut st = state_info.write().unwrap();
            if let Some(snap) = st.devices.get_mut(&id) {
                snap.props = props;
            }
        })
        .param(move |_seq, param_id, index, _next, param| {
            if let Some(pod) = param {
                parse_device_param(param_id, index, pod, &state_param, id);

            }
        })
        .register();

    device.subscribe_params(&[
        pw::spa::param::ParamType::EnumProfile,
        pw::spa::param::ParamType::Profile,
        pw::spa::param::ParamType::EnumRoute,
        pw::spa::param::ParamType::Route,
    ]);
    device.enum_params(0, None, 0, u32::MAX);

    let dev_name = state
        .read()
        .unwrap()
        .devices
        .get(&id)
        .and_then(|d| d.props.get("device.name").cloned())
        .unwrap_or_default();
    tracing::debug!(id, name = %dev_name, "bound device");

    Some((device, Box::new(listener)))
}

fn parse_device_param(
    param_id: pw::spa::param::ParamType,
    param_index: u32,
    pod: &pw::spa::pod::Pod,
    state: &Arc<RwLock<PwState>>,
    device_id: u32,
) {
    let Ok(obj) = pod.as_object() else {
        return;
    };

    match param_id {
        pw::spa::param::ParamType::EnumProfile => {
            let mut index = 0u32;
            let mut name = String::new();
            let mut description = String::new();
            let mut priority = 0u32;
            let mut available = String::from("unknown");

            for prop in obj.props() {
                match prop.key().0 {
                    pw::spa::sys::SPA_PARAM_PROFILE_index => {
                        if let Ok(v) = prop.value().get_int() {
                            index = v as u32;
                        }
                    }
                    pw::spa::sys::SPA_PARAM_PROFILE_name => {
                        name = pod_get_string(prop.value()).unwrap_or_default();
                    }
                    pw::spa::sys::SPA_PARAM_PROFILE_description => {
                        description = pod_get_string(prop.value()).unwrap_or_default();
                    }
                    pw::spa::sys::SPA_PARAM_PROFILE_priority => {
                        if let Ok(v) = prop.value().get_int() {
                            priority = v as u32;
                        }
                    }
                    pw::spa::sys::SPA_PARAM_PROFILE_available => {
                        let v = prop.value().get_id().map(|id| id.0)
                            .or_else(|_| prop.value().get_int().map(|i| i as u32));
                        if let Ok(v) = v {
                            available = match v {
                                pw::spa::sys::SPA_PARAM_AVAILABILITY_no => "no".to_string(),
                                pw::spa::sys::SPA_PARAM_AVAILABILITY_yes => "yes".to_string(),
                                _ => "unknown".to_string(),
                            };
                        }
                    }
                    _ => {}
                }
            }

            let profile = state::PwProfileSnapshot {
                index,
                name,
                description,
                priority,
                available,
            };

            let mut st = state.write().unwrap();
            if let Some(snap) = st.devices.get_mut(&device_id) {
                // Update existing profile or insert new one
                if let Some(existing) = snap.profiles.iter_mut().find(|p| p.index == index) {
                    *existing = profile;
                } else {
                    snap.profiles.push(profile);
                }
            }
        }
        pw::spa::param::ParamType::Profile => {
            // Active profile
            for prop in obj.props() {
                if prop.key().0 == pw::spa::sys::SPA_PARAM_PROFILE_index
                    && let Ok(v) = prop.value().get_int()
                {
                    let mut st = state.write().unwrap();
                    if let Some(snap) = st.devices.get_mut(&device_id) {
                        snap.active_profile_index = Some(v as u32);
                    }
                }
            }
        }
        pw::spa::param::ParamType::EnumRoute => {
            let mut index = 0u32;
            let mut direction = String::from("unknown");
            let mut name = String::new();
            let mut description = String::new();
            let mut available = String::from("unknown");

            for prop in obj.props() {
                match prop.key().0 {
                    pw::spa::sys::SPA_PARAM_ROUTE_index => {
                        if let Ok(v) = prop.value().get_int() {
                            index = v as u32;
                        }
                    }
                    pw::spa::sys::SPA_PARAM_ROUTE_direction => {
                        if let Ok(v) = prop.value().get_id() {
                            direction = match v.0 {
                                pw::spa::sys::SPA_DIRECTION_INPUT => "input".to_string(),
                                pw::spa::sys::SPA_DIRECTION_OUTPUT => "output".to_string(),
                                _ => "unknown".to_string(),
                            };
                        }
                    }
                    pw::spa::sys::SPA_PARAM_ROUTE_name => {
                        name = pod_get_string(prop.value()).unwrap_or_default();
                    }
                    pw::spa::sys::SPA_PARAM_ROUTE_description => {
                        description = pod_get_string(prop.value()).unwrap_or_default();
                    }
                    pw::spa::sys::SPA_PARAM_ROUTE_available => {
                        let v = prop.value().get_id().map(|id| id.0)
                            .or_else(|_| prop.value().get_int().map(|i| i as u32));
                        if let Ok(v) = v {
                            available = match v {
                                pw::spa::sys::SPA_PARAM_AVAILABILITY_no => "no".to_string(),
                                pw::spa::sys::SPA_PARAM_AVAILABILITY_yes => "yes".to_string(),
                                _ => "unknown".to_string(),
                            };
                        }
                    }
                    _ => {}
                }
            }

            let route = state::PwRouteSnapshot {
                index,
                direction,
                name,
                description,
                available,
            };

            let mut st = state.write().unwrap();
            if let Some(snap) = st.devices.get_mut(&device_id) {
                // Update existing route or insert new one
                if let Some(existing) = snap.routes.iter_mut().find(|r| r.index == index) {
                    *existing = route;
                } else {
                    snap.routes.push(route);
                }
            }
        }
        pw::spa::param::ParamType::Route => {
            // Active route — param_index == 0 signals start of a new batch
            for prop in obj.props() {
                if prop.key().0 == pw::spa::sys::SPA_PARAM_ROUTE_index
                    && let Ok(v) = prop.value().get_int()
                {
                    let idx = v as u32;
                    let mut st = state.write().unwrap();
                    if let Some(snap) = st.devices.get_mut(&device_id) {
                        if param_index == 0 {
                            snap.active_routes.clear();
                        }
                        if !snap.active_routes.contains(&idx) {
                            snap.active_routes.push(idx);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Extract a string from a Pod. libspa-rs has `is_string()` but no `get_string()` (TODO in upstream).
fn pod_get_string(pod: &pw::spa::pod::Pod) -> Option<String> {
    if !pod.is_string() {
        return None;
    }
    unsafe {
        let mut str_ptr: *const std::ffi::c_char = std::ptr::null();
        let res = pw::spa::sys::spa_pod_get_string(pod.as_raw_ptr(), &mut str_ptr);
        if res >= 0 && !str_ptr.is_null() {
            Some(
                std::ffi::CStr::from_ptr(str_ptr)
                    .to_string_lossy()
                    .into_owned(),
            )
        } else {
            None
        }
    }
}

// -- Pod building helpers --

use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Property, PropertyFlags, Value};

/// Build a Props pod setting volume.
fn build_volume_pod(volume: f32) -> Vec<u8> {
    let obj = Value::Object(Object {
        type_: pw::spa::sys::SPA_TYPE_OBJECT_Props,
        id: pw::spa::sys::SPA_PARAM_Props,
        properties: vec![Property {
            key: pw::spa::sys::SPA_PROP_volume,
            flags: PropertyFlags::empty(),
            value: Value::Float(volume),
        }],
    });
    let buf = std::io::Cursor::new(Vec::<u8>::new());
    let (cursor, _len) = PodSerializer::serialize(buf, &obj).expect("serialize volume pod");
    cursor.into_inner()
}

/// Build a Props pod setting mute.
fn build_mute_pod(mute: bool) -> Vec<u8> {
    let obj = Value::Object(Object {
        type_: pw::spa::sys::SPA_TYPE_OBJECT_Props,
        id: pw::spa::sys::SPA_PARAM_Props,
        properties: vec![Property {
            key: pw::spa::sys::SPA_PROP_mute,
            flags: PropertyFlags::empty(),
            value: Value::Bool(mute),
        }],
    });
    let buf = std::io::Cursor::new(Vec::<u8>::new());
    let (cursor, _len) = PodSerializer::serialize(buf, &obj).expect("serialize mute pod");
    cursor.into_inner()
}

/// Build a Profile pod to set active profile on a device.
fn build_profile_pod(profile_index: u32) -> Vec<u8> {
    let obj = Value::Object(Object {
        type_: pw::spa::sys::SPA_TYPE_OBJECT_ParamProfile,
        id: pw::spa::sys::SPA_PARAM_Profile,
        properties: vec![Property {
            key: pw::spa::sys::SPA_PARAM_PROFILE_index,
            flags: PropertyFlags::empty(),
            value: Value::Int(profile_index as i32),
        }],
    });
    let buf = std::io::Cursor::new(Vec::<u8>::new());
    let (cursor, _len) = PodSerializer::serialize(buf, &obj).expect("serialize profile pod");
    cursor.into_inner()
}
