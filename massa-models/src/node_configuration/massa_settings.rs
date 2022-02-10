//! Build a settings for an object that implement massa_settings
//!
//! ---
//! The massa node configuration is composed from 2 parts, one part is the
//! file configuration often named `config.toml` located in the project
//! directory.
//!
//! The First thing that the node will try is to read the configuration located
//! in the `path` described in the environment variable `MASSA_CONFIG_PATH`.
//! If no path found in the environment variable, the relativ path
//! `base_confg/config.toml` is used as default. The default path should exist
//! because it's a config file pushed in the repository.
//!
//! The next step will try to read the file at the given path. It will `panic`
//! if no file found.
//!
//! Whatever configuration you used (the one from de environment variable or the
//! default one) You always have a next possibility. Using the default path of
//! configuration for the massa-project. The default configuration directories
//! is set on Setting creation. All the configuration in this file will be merged
//! with the previous step (override if duplicated)
//!
//! The last step is to merge the enironment variable prefixed with
//! `MASSA_CLIENT`, override if duplicated
//!
use directories::ProjectDirs;
use serde::Deserialize;
use std::path::Path;

#[inline]
pub fn build_massa_settings<T: Deserialize<'static>>(app_name: &str, env_prefix: &str) -> T {
    let mut settings = config::Config::default();
    let config_path = std::env::var("MASSA_CONFIG_PATH")
        .unwrap_or_else(|_| "base_config/config.toml".to_string());
    settings
        .merge(config::File::with_name(&config_path))
        .unwrap_or_else(|_| {
            panic!(
                "failed to read {} config {}",
                config_path,
                std::env::current_dir().unwrap().as_path().to_str().unwrap()
            )
        });
    let config_override_path = std::env::var("MASSA_CONFIG_OVERRIDE_PATH")
        .unwrap_or_else(|_| "config/config.toml".to_string());
    if Path::new(&config_override_path).is_file() {
        settings
            .merge(config::File::with_name(&config_override_path))
            .unwrap_or_else(|_| {
                panic!(
                    "failed to read {} override config {}",
                    config_override_path,
                    std::env::current_dir().unwrap().as_path().to_str().unwrap()
                )
            });
    }
    if let Some(proj_dirs) = ProjectDirs::from("com", "MassaLabs", app_name) {
        // Portable user config loading
        let user_config_path = proj_dirs.config_dir();
        if user_config_path.exists() {
            let path_str = user_config_path.to_str().unwrap();
            settings
                .merge(config::File::with_name(path_str))
                .unwrap_or_else(|_| panic!("failed to read {} user config", path_str));
        }
    }
    settings
        .merge(config::Environment::with_prefix(env_prefix))
        .unwrap();
    settings.try_into().unwrap()
}
