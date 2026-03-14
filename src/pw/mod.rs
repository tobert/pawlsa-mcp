pub mod state;

use state::{PwLinkSnapshot, PwNodeSnapshot, PwPortSnapshot, PwState, dict_to_map};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use pipewire as pw;
use pw::proxy::{Listener, ProxyT};
use pw::types::ObjectType;

pub struct PwHandle {
    pub state: Arc<RwLock<PwState>>,
    #[allow(dead_code)]
    pub join_handle: JoinHandle<()>,
}

pub fn spawn_pw_thread() -> PwHandle {
    let state = Arc::new(RwLock::new(PwState::default()));
    let state_clone = state.clone();

    let join_handle = std::thread::spawn(move || {
        run_pw_loop(state_clone);
    });

    PwHandle { state, join_handle }
}

struct Proxies {
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

    fn remove(&mut self, proxy_id: u32) {
        self.proxies_t.remove(&proxy_id);
        self.listeners.remove(&proxy_id);
    }
}

fn run_pw_loop(state: Arc<RwLock<PwState>>) {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None).expect("failed to create PW MainLoop");
    let context =
        pw::context::ContextRc::new(&mainloop, None).expect("failed to create PW Context");
    let core = context.connect_rc(None).expect("failed to connect PW Core");
    let registry = core.get_registry_rc().expect("failed to get PW Registry");

    let registry_weak = registry.downgrade();
    let proxies: Rc<RefCell<Proxies>> = Rc::new(RefCell::new(Proxies::new()));

    let state_remove = state.clone();
    let proxies_remove = proxies.clone();

    let _registry_listener = registry
        .add_listener_local()
        .global(move |obj| {
            let Some(registry) = registry_weak.upgrade() else {
                return;
            };

            let id = obj.id;
            let result: Option<(Box<dyn ProxyT>, Box<dyn Listener>)> = match obj.type_ {
                ObjectType::Node => bind_node(&registry, obj, &state),
                ObjectType::Port => bind_port(&registry, obj, &state),
                ObjectType::Link => bind_link(&registry, obj, &state),
                _ => {
                    tracing::trace!(id, type_ = ?obj.type_, "ignoring global");
                    None
                }
            };

            if let Some((proxy_t, listener)) = result {
                proxies.borrow_mut().add(proxy_t, listener);
            }
        })
        .global_remove(move |id| {
            proxies_remove.borrow_mut().remove(id);
            let mut st = state_remove.write().unwrap();
            st.nodes.remove(&id);
            st.ports.remove(&id);
            st.links.remove(&id);
        })
        .register();

    mainloop.run();
}

type DictRef = pw::spa::utils::dict::DictRef;

fn bind_node(
    registry: &pw::registry::RegistryRc,
    obj: &pw::registry::GlobalObject<&DictRef>,
    state: &Arc<RwLock<PwState>>,
) -> Option<(Box<dyn ProxyT>, Box<dyn Listener>)> {
    let id = obj.id;

    // Insert initial snapshot from global props
    let initial_props = obj.props.map(dict_to_map).unwrap_or_default();
    {
        let mut st = state.write().unwrap();
        st.nodes.insert(
            id,
            PwNodeSnapshot {
                id,
                state: "unknown".to_string(),
                max_input_ports: 0,
                max_output_ports: 0,
                n_input_ports: 0,
                n_output_ports: 0,
                props: initial_props,
            },
        );
    }

    let node: pw::node::Node = registry.bind(obj).ok()?;

    let state = state.clone();
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
            let mut st = state.write().unwrap();
            st.nodes.insert(
                id,
                PwNodeSnapshot {
                    id,
                    state: node_state,
                    max_input_ports: info.max_input_ports(),
                    max_output_ports: info.max_output_ports(),
                    n_input_ports: info.n_input_ports(),
                    n_output_ports: info.n_output_ports(),
                    props,
                },
            );
        })
        .register();

    Some((Box::new(node), Box::new(listener)))
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
                props: initial_props,
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
            let props = info.props().map(dict_to_map).unwrap_or_default();
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
                    props,
                },
            );
        })
        .register();

    Some((Box::new(link), Box::new(listener)))
}
