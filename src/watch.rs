use anyhow::{Context, Result};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use watchexec::sources::fs::Watcher;
use watchexec::{Config, Watchexec};
use watchexec_filterer_tagged::{Filter, Matcher, Op, Pattern, TaggedFilterer};

use super::display_error;
use crate::args::Options;
use crate::deploy;

pub(crate) async fn watch(opt: Options) -> Result<()> {
    let config = Config::default();

    config.file_watcher(Watcher::Native);
    config.pathset(["."]);

    let filter = TaggedFilterer::new(".".into(), std::env::current_dir()?)
        .await
        .unwrap();

    let cache_dir_pattern = opt.cache_directory.display().to_string().replace('\\', "/");
    let cache_file_pattern = opt.cache_file.to_string_lossy().replace('\\', "/");

    filter
        .add_filters(&[
            Filter {
                in_path: None,
                on: Matcher::Path,
                op: Op::NotGlob,
                pat: Pattern::Glob(format!("{}/**", cache_dir_pattern)),
                negate: false,
            },
            Filter {
                in_path: None,
                on: Matcher::Path,
                op: Op::NotGlob,
                pat: Pattern::Glob(cache_file_pattern),
                negate: false,
            },
            Filter {
                in_path: None,
                on: Matcher::Path,
                op: Op::NotGlob,
                pat: Pattern::Glob(".git/**".into()),
                negate: false,
            },
            Filter {
                in_path: None,
                on: Matcher::Path,
                op: Op::NotEqual,
                pat: Pattern::Exact("DOTTER_SYMLINK_TEST".into()),
                negate: false,
            },
        ])
        .await?;
    config.filterer(filter);

    let last_deploy = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(10)));
    let debounce_duration = Duration::from_millis(500);

    config.on_action(move |mut action| {
        if action.signals().next().is_some() {
            action.quit();
            return action;
        }

        debug!("Changes detected in watched files.");
        trace!("Changed files: {:#?}", action.paths().collect::<Vec<_>>());

        let mut last = last_deploy.lock().unwrap();
        let now = Instant::now();
        if now.duration_since(*last) < debounce_duration {
            debug!("Skipping deployment due to debounce (too soon after previous deployment)");
            return action;
        }
        *last = now;
        drop(last);

        println!("[Dotter] Deploying...");
        if let Err(e) = deploy::deploy(&opt) {
            display_error(e);
        }

        action
    });

    config.on_error(move |e| {
        log::error!("Watcher error: {e:#?}");
    });

    let we = Watchexec::with_config(config)?;
    we.main().await.context("run watchexec main loop")??;
    Ok(())
}
