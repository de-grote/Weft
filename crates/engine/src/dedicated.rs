//! `weft --server`: the world and the network with nobody playing on this machine, no window and
//! no GPU. Options: `--world name` (made if new), `--seed n`, `--port n`, `--run-dir dir`. What it
//! keeps is where a game keeps it (`config/`, `saves/`). `config/whitelist.txt`, keys one a line,
//! limits who may join; without it anyone can, and nobody may do more than play until `op`.
//! `config/disabled_mods.txt`, ids one a line, leaves mods out as the mods screen does.

use std::io::BufRead;

use crate::app::Options;
use crate::content::content;
use crate::server::{self, ServerConfig};
use crate::{config, modloader, net, saves, settings::Settings};

pub fn run(opts: Options) -> Result<(), String> {
    let off = modloader::off_file(&opts.run_dir, false);
    let _mods = modloader::load(&opts.mod_dirs, &opts.cache_dir, &off, &opts.mods_off)?;
    crate::settings::migrate(&opts.run_dir);
    config::load(&opts.run_dir);
    if let Some(port) = crate::app::api::launch_options().get("port") {
        config::override_for_run("core:server_port", port);
    }
    if let Some(v) = opts.view {
        config::override_for_run("core:view_distance", &v.to_string());
    }
    let settings = Settings::new(&opts.run_dir);

    let name = opts.world.as_deref().unwrap_or("world");
    let saves_dir = settings.saves_dir();
    let meta = saves::list(&saves_dir)
        .into_iter()
        .find(|m| m.name == name)
        .unwrap_or_else(|| saves::create(&saves_dir, name, opts.seed.unwrap_or_else(saves::now_secs)));
    if meta.format > saves::SAVE_FORMAT {
        return Err(format!("world '{name}' was saved in format {}, this build reads {}", meta.format, saves::SAVE_FORMAT));
    }
    for m in meta.missing_mods(content()) {
        log::warn!("the world was played with mod {m}, which is not loaded");
    }
    let storage = saves::Storage::open(meta.clone(), content());
    let whitelist = crate::paths::config(&opts.run_dir).join("whitelist.txt");
    let mut server = server::spawn(
        ServerConfig { seed: meta.seed, view_radius: settings.view_distance(), dedicated: true, whitelist: Some(whitelist) },
        Some(storage),
    );
    net::host(&server, &settings);
    log::info!("server running world '{name}'; type stop to save and leave, anything else for help");

    // With no input at all (a service), it runs until stopped from outside.
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.trim() == "stop" {
            server.stop();
            log::info!("saved, stopped");
            return Ok(());
        }
        let _ = server.console.send(line);
    }
    loop {
        std::thread::park();
    }
}
