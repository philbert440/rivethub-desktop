use std::collections::{BTreeMap, HashMap};

use gateway::{Endpoint, Gateway, GatewayError, PluginEvent};
use model::plugin::canvas::{Event, Reply};
use model::plugin::view::{Answer, Value, View};
use model::plugin::{Context, LogLine, Notice, Params, Permission, Plugin, PluginRef, Settings};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::Hub;

/// The plug-ins of every node that answered, each labelled by its node.
#[derive(Debug, Default)]
pub struct Installed {
    pub plugins: Vec<Plugin>,
    /// The nodes that listed their plug-ins, by name, even when they have none.
    pub nodes: Vec<String>,
    pub unreachable: Vec<String>,
}

/// Something a node said about its plug-ins. `None` means its stream came back after a break, so
/// anything may have changed.
#[derive(Debug, Clone)]
pub struct NodeNotice {
    pub node: String,
    pub notice: Option<Notice>,
}

impl Hub {
    /// Every reachable node's plug-ins. A node with an rh too old to host them has none.
    pub async fn plugins(&self) -> Installed {
        let hub = self.clone();
        self.on_runtime(async move {
            let mut installed = Installed::default();
            // This node first: its listing names it, so the mesh walk below can leave it out.
            match hub.inner.local.plugins().await {
                Ok(listing) => {
                    hub.inner
                        .local_node
                        .write()
                        .get_or_insert_with(|| listing.node.clone());
                    installed.nodes.push(listing.node);
                    installed.plugins.extend(listing.plugins);
                }
                Err(err) if err.is_missing_route() => {}
                Err(_) => installed.unreachable.push("this node".to_owned()),
            }
            let local = hub.inner.local_node.read().clone();
            let mut others = Vec::new();
            for node in hub.inner.local.mesh().await.unwrap_or_default() {
                let is_local = local.as_deref().is_some_and(|name| node.answers_to(name));
                if node.online
                    && !is_local
                    && let Ok(gateway) = hub.gateway_for(Some(&node.name)).await
                {
                    others.push((node.name, gateway));
                }
            }
            let listed = futures::future::join_all(
                others
                    .into_iter()
                    .map(|(node, gateway)| async move { (node, gateway.plugins().await) }),
            )
            .await;
            for (node, listing) in listed {
                match listing {
                    Ok(listing) => {
                        installed.nodes.push(listing.node);
                        installed.plugins.extend(listing.plugins);
                    }
                    Err(err) if err.is_missing_route() => {}
                    Err(_) => installed.unreachable.push(node),
                }
            }
            Ok(installed)
        })
        .await
        .unwrap_or_default()
    }

    pub async fn plugin_view(
        &self,
        plugin: PluginRef,
        view: String,
        params: Option<Params>,
        context: Context,
    ) -> Result<View, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway
                .plugin_view(&plugin.id, &view, params.as_ref(), &context)
                .await
        })
        .await
    }

    pub async fn plugin_action(
        &self,
        plugin: PluginRef,
        action: String,
        params: Option<Params>,
        values: BTreeMap<String, Value>,
        context: Context,
    ) -> Result<Answer, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway
                .plugin_action(&plugin.id, &action, params.as_ref(), &values, &context)
                .await
        })
        .await
    }

    pub async fn canvas_event(
        &self,
        plugin: PluginRef,
        canvas: String,
        event: Event,
        params: Option<Params>,
        context: Context,
    ) -> Result<Reply, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway
                .canvas_event(&plugin.id, &canvas, &event, params.as_ref(), &context)
                .await
        })
        .await
    }

    /// One of a plug-in's own files, and its media type when the plug-in names one.
    pub async fn plugin_file(
        &self,
        plugin: PluginRef,
        path: String,
    ) -> Result<(Vec<u8>, Option<String>), GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway.plugin_file(&plugin.id, &path).await
        })
        .await
    }

    pub async fn plugin_logs(&self, plugin: PluginRef) -> Result<Vec<LogLine>, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway.plugin_logs(&plugin.id).await
        })
        .await
    }

    pub async fn plugin_settings(&self, plugin: PluginRef) -> Result<Settings, GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway.plugin_settings(&plugin.id).await
        })
        .await
    }

    pub async fn save_plugin_settings(
        &self,
        plugin: PluginRef,
        values: BTreeMap<String, Option<Value>>,
    ) -> Result<(), GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway.save_plugin_settings(&plugin.id, &values).await
        })
        .await
    }

    /// Starts a plug-in, granting it the permissions it waits on, which `approve` names exactly.
    pub async fn enable_plugin(
        &self,
        plugin: PluginRef,
        approve: Vec<Permission>,
    ) -> Result<(), GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.device_gateway(&plugin.node).await?;
            gateway.enable_plugin(&plugin.id, &approve).await
        })
        .await
    }

    /// `node`'s gateway reached with this device's certificate, which rh asks for before it takes
    /// an approval as the person's. Other nodes are always reached with it; this client's own
    /// node is otherwise reached without.
    async fn device_gateway(&self, node: &str) -> Result<Gateway, GatewayError> {
        if !self.is_local(node) {
            return self.gateway_for(Some(node)).await;
        }
        match &self.inner.identity {
            Some(identity) => Gateway::connect(&Endpoint {
                base_url: self.inner.local.base_url().to_owned(),
                ca_pem: self.inner.ca_pem.clone(),
                identity: Some(identity.clone()),
            }),
            None => Ok(self.inner.local.clone()),
        }
    }

    pub async fn disable_plugin(&self, plugin: PluginRef) -> Result<(), GatewayError> {
        let hub = self.clone();
        self.on_runtime(async move {
            let gateway = hub.gateway_for(Some(&plugin.node)).await?;
            gateway.disable_plugin(&plugin.id).await
        })
        .await
    }

    /// Follows what nodes say about their plug-ins; tell the watch which nodes to follow.
    pub fn watch_plugins(&self) -> (PluginWatch, mpsc::UnboundedReceiver<NodeNotice>) {
        let (notices, receiver) = mpsc::unbounded_channel();
        let watch = PluginWatch {
            hub: self.clone(),
            notices,
            tasks: Mutex::new(HashMap::new()),
        };
        (watch, receiver)
    }
}

/// One stream per followed node, stopped when the node is no longer followed or the watch goes.
#[derive(Debug)]
pub struct PluginWatch {
    hub: Hub,
    notices: mpsc::UnboundedSender<NodeNotice>,
    tasks: Mutex<HashMap<String, AbortHandle>>,
}

impl PluginWatch {
    /// Follows exactly these nodes, by name.
    pub fn follow(&self, nodes: &[String]) {
        let mut tasks = self.tasks.lock();
        tasks.retain(|node, task| {
            let keep = nodes.contains(node);
            if !keep {
                task.abort();
            }
            keep
        });
        for node in nodes {
            if tasks.contains_key(node) {
                continue;
            }
            let task =
                self.hub
                    .inner
                    .runtime
                    .spawn(follow(self.hub.clone(), node.clone(), self.notices.clone()));
            tasks.insert(node.clone(), task.abort_handle());
        }
    }
}

impl Drop for PluginWatch {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().values() {
            task.abort();
        }
    }
}

async fn follow(hub: Hub, node: String, notices: mpsc::UnboundedSender<NodeNotice>) {
    let mut retry = hub.backoff(Some(&node));
    let mut broke = false;
    loop {
        let connected = match hub.gateway_for(Some(&node)).await {
            Ok(gateway) => gateway.watch_plugins().await,
            Err(err) => Err(err),
        };
        if let Ok(mut events) = connected {
            retry.reset();
            if broke {
                let resumed = NodeNotice {
                    node: node.clone(),
                    notice: None,
                };
                if notices.send(resumed).is_err() {
                    return;
                }
            }
            while let Some(PluginEvent::Notice(notice)) = events.recv().await {
                let said = NodeNotice {
                    node: node.clone(),
                    notice: Some(notice),
                };
                if notices.send(said).is_err() {
                    return;
                }
            }
        }
        if notices.is_closed() {
            return;
        }
        broke = true;
        tokio::time::sleep(retry.next_wait()).await;
    }
}
