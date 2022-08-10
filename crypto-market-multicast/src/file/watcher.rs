
use std::path::Path;

use anyhow::Context;
use notify::{EventHandler, RecommendedWatcher, RecursiveMode, Watcher};

pub fn create_watcher(
    path: impl AsRef<Path>,
    handler: impl EventHandler,
) -> anyhow::Result<RecommendedWatcher> {
    let path = path.as_ref();

    let mut watcher = notify::recommended_watcher(handler)?;

    watcher
        .watch(path, RecursiveMode::NonRecursive)
        .context("failed to watch the specified path")?;

    Ok(watcher)
}
