//! Build a settings for an object that implement `massa_settings`
//!
//! ---
//! The massa node configuration is composed from 2 parts, one part is the
//! file configuration often named `config.toml` located in the project
//! directory.
//!
//! The First thing that the node will try is to read the configuration located
//! in the `path` described in the environment variable `MASSA_CONFIG_PATH`.
//! If no path found in the environment variable, the relative path
//! `base_config/config.toml` is used as default. The default path should exist
//! because it's a configuration file pushed in the repository.
//!
//! The next step will try to read the file at the given path. It will `panic`
//! if no file found.
//!
//! Whatever configuration you used (the one from the environment variable or the
//! default one) You always have a next possibility. Using the default path of
//! configuration for the massa-project. The default configuration directories
//! is set on Setting creation. All the configuration in this file will be merged
//! with the previous step (override if duplicated)
//!
//! The last step is to merge the environment variable prefixed with
//! `MASSA_CLIENT`, override if duplicated
//!
use directories::ProjectDirs;
use serde::Deserialize;
use std::path::Path;

/// Merge the settings
/// 1. default
/// 2. in path specified in `MASSA_CONFIG_PATH` environment variable (`base_config/config.toml` by default)
/// 3. in path specified in `MASSA_CONFIG_OVERRIDE_PATH` environment variable (`config/config.toml` by default)
#[inline]
pub fn build_massa_settings<T: Deserialize<'static>>(app_name: &str, env_prefix: &str) -> T {
    let mut builder = config::Config::builder();
    let config_path = std::env::var("MASSA_CONFIG_PATH")
        .unwrap_or_else(|_| "base_config/config.toml".to_string());

    builder = builder.add_source(config::File::with_name(&config_path));

    let config_override_path = std::env::var("MASSA_CONFIG_OVERRIDE_PATH")
        .unwrap_or_else(|_| "config/config.toml".to_string());

    if Path::new(&config_override_path).is_file() {
        builder = builder.add_source(config::File::with_name(&config_override_path));
    }

    if let Some(proj_dirs) = ProjectDirs::from("com", "MassaLabs", app_name) {
        // Portable user config loading
        let user_config_path = proj_dirs.config_dir();
        if user_config_path.exists() {
            let path_str = user_config_path.to_str().unwrap();
            builder = builder.add_source(config::File::with_name(path_str));
        }
    }

    let s = builder
        .add_source(config::Environment::with_prefix(env_prefix))
        .build()
        .unwrap();

    s.try_deserialize().unwrap()
}
