use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
pub use arcstr::ArcStr;
use notify::{EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Deserialize;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::parameter_type::MarketDataArg;

pub type SubTaskCreator = dyn Fn(Handle, MarketDataArg) -> JoinHandle<()> + Send + Sync;

#[derive(Debug, Deserialize)]
struct ConfigStructure {
    pub ipcs: Vec<MarketDataArg>,
}

pub type SubTask = Arc<JoinHandle<()>>;

pub struct DynamicConfigHandler {
    /// config path
    config_path: PathBuf,
    /// To allow spawning green thread in handler.
    handle: Handle,
    /// generate subscription task
    to_task: Box<SubTaskCreator>,
    /// Save the runtime environment
    all_sub_task: HashMap<MarketDataArg, SubTask>,
}

impl DynamicConfigHandler {
    pub fn new(config_path: &str, handle: Handle, to_task: Box<SubTaskCreator>) -> Self {
        let mut config = Self {
            config_path: PathBuf::from(config_path),
            handle,
            to_task,
            all_sub_task: HashMap::with_capacity(5),
        };
        println!("config_path {}", config_path);
        config.on_config_update().expect("init config error");
        config
    }

    fn cleanup(&mut self, tasks_to_clean_up: impl Iterator<Item = MarketDataArg>) {
        for market_data_arg in tasks_to_clean_up {
            if let Some(task) = self.all_sub_task.remove(&market_data_arg) {
                task.abort();
            }
        }
    }

    fn register(&mut self, tasks_to_register: impl Iterator<Item = MarketDataArg>) {
        for market_data_arg in tasks_to_register {
            self.all_sub_task.insert(
                market_data_arg.clone(),
                (self.to_task)(self.handle.clone(), market_data_arg).into(),
            );
        }
    }

    fn on_config_update(&mut self) -> anyhow::Result<()> {
        let content = std::fs::read_to_string(&self.config_path)?;
        let config: ConfigStructure = serde_json::from_str(&content)
            .context("failed to deserialize the configuration file")?;
        
        tracing::debug!("config {:?}", config);

        let all_sub_task: HashSet<MarketDataArg> =
            self.all_sub_task.keys().map(|m| m.clone()).collect();
        let ipcs: HashSet<MarketDataArg> = config.ipcs.iter().map(|m| m.clone()).collect();

        let added = ipcs.difference(&all_sub_task).map(|m| m.clone());
        let removed = all_sub_task.difference(&ipcs).map(|m| m.clone());

        self.cleanup(removed);
        self.register(added);

        Ok(())
    }

    pub fn watcher(self) -> anyhow::Result<RecommendedWatcher> {
        let path = self.config_path.to_owned();
        let mut watcher = notify::recommended_watcher(self)?;

        watcher
            .watch(&path, RecursiveMode::NonRecursive)
            .context("failed to watch the specified path")?;

        Ok(watcher)
    }
}

impl EventHandler for DynamicConfigHandler {
    fn handle_event(&mut self, event: notify::Result<notify::Event>) {
        let span = tracing::info_span!("handle_event");
        let _span = span.enter();

        match event {
            Ok(event) => {
                if let EventKind::Modify(_) = event.kind {
                    if self.on_config_update().is_err() {
                        tracing::debug!("Successfully handled event");
                    }
                }
            }
            Err(err) => tracing::error!("Failed to handle event: {}", err),
        };
    }
}
