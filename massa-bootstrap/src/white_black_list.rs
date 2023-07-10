use std::{
    borrow::Cow,
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::error::BootstrapError;
use massa_logging::massa_trace;
use parking_lot::RwLock;
use tracing::{info, warn};

use crate::tools::normalize_ip;

/// A wrapper around the white/black lists that allows efficient sharing between threads
// TODO: don't clone the path-bufs...
#[derive(Clone)]
pub struct SharedWhiteBlackList<'a> {
    inner: Arc<RwLock<WhiteBlackListInner>>,
    white_path: Cow<'a, Path>,
    black_path: Cow<'a, Path>,
}

impl SharedWhiteBlackList<'_> {
    pub(crate) fn new(white_path: PathBuf, black_path: PathBuf) -> Result<Self, BootstrapError> {
        let (white_list, black_list) = WhiteBlackListInner::init_list(&white_path, &black_path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(WhiteBlackListInner {
                white_list,
                black_list,
            })),
            white_path: Cow::from(white_path),
            black_path: Cow::from(black_path),
        })
    }

    /// get the white list
    pub fn get_white_list(&self) -> Option<HashSet<IpAddr>> {
        self.inner.read().white_list.clone()
    }

    /// get the black list
    pub fn get_black_list(&self) -> Option<HashSet<IpAddr>> {
        self.inner.read().black_list.clone()
    }

    /// Add IP address to the black list
    pub fn add_ips_to_blacklist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let mut write_lock = self.inner.write();
        let list = if let Some(black_list) = &mut write_lock.black_list {
            black_list.extend(ips);
            black_list.clone()
        } else {
            let hash_set = HashSet::from_iter(ips);
            write_lock.black_list = Some(hash_set.clone());
            hash_set
        };
        self.write_list_to_file(&self.black_path, &list)?;
        Ok(())
    }

    /// Remove IPs address from the black list
    pub fn remove_ips_from_blacklist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let mut write_lock = self.inner.write();
        if let Some(black_list) = &mut write_lock.black_list {
            for ip in ips {
                black_list.remove(&ip);
            }
            self.write_list_to_file(&self.black_path, black_list)?;
        }
        Ok(())
    }

    /// Add IP address to the white list
    pub fn add_ips_to_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let mut write_lock = self.inner.write();
        let list = if let Some(white_list) = &mut write_lock.white_list {
            white_list.extend(ips);
            white_list.clone()
        } else {
            let hash_set = HashSet::from_iter(ips);
            write_lock.white_list = Some(hash_set.clone());
            hash_set.clone()
        };
        self.write_list_to_file(&self.white_path, &list)?;
        Ok(())
    }

    /// Remove IPs address from the white list
    pub fn remove_ips_from_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let mut write_lock = self.inner.write();
        if let Some(white_list) = &mut write_lock.white_list {
            for ip in ips {
                white_list.remove(&ip);
            }
            self.write_list_to_file(&self.white_path, white_list)?;
        }
        Ok(())
    }

    /// write list to file
    fn write_list_to_file(
        &self,
        file_path: &Path,
        data: &HashSet<IpAddr>,
    ) -> Result<(), BootstrapError> {
        let list = serde_json::to_string(data).map_err(|e| {
            warn!(error = ?e, "failed to serialize list");
            BootstrapError::SerializationError(e.to_string())
        })?;
        std::fs::write(file_path, list).map_err(|e| {
            warn!(error = ?e, "failed to write list to file");
            BootstrapError::IoError(e)
        })?;
        Ok(())
    }

    /// Checks if the white/black list is up to date with a read-lock
    /// Creates a new list, and replaces the old one in a write-lock
    pub(crate) fn update(&mut self) -> Result<(), BootstrapError> {
        let read_lock = self.inner.read();
        let (new_white_file, new_black_file) =
            WhiteBlackListInner::update_list(&self.white_path, &self.black_path)?;
        let white_delta = new_white_file != read_lock.white_list;
        let black_delta = new_black_file != read_lock.black_list;
        if white_delta || black_delta {
            // Ideally this scope would be atomic
            let mut mut_inner = {
                drop(read_lock);
                self.inner.write()
            };

            if white_delta {
                info!("whitelist has updated !");
                mut_inner.white_list = new_white_file;
            }
            if black_delta {
                info!("blacklist has updated !");
                mut_inner.black_list = new_black_file;
            }
        }
        Ok(())
    }

    #[cfg_attr(test, allow(unreachable_code, unused_variables))]
    pub(crate) fn is_ip_allowed(&self, remote_addr: &SocketAddr) -> Result<(), BootstrapError> {
        #[cfg(test)]
        return Ok(());

        let ip = normalize_ip(remote_addr.ip());
        // whether the peer IP address is blacklisted
        let read = self.inner.read();
        if let Some(ip_list) = &read.black_list && ip_list.contains(&ip) {
            massa_trace!("bootstrap.lib.run.select.accept.refuse_blacklisted", {"remote_addr": remote_addr});
            Err(BootstrapError::BlackListed(ip.to_string()))
            // whether the peer IP address is not present in the whitelist
        } else if let Some(ip_list) = &read.white_list && !ip_list.contains(&ip) {
            massa_trace!("bootstrap.lib.run.select.accept.refuse_not_whitelisted", {"remote_addr": remote_addr});
            Err(BootstrapError::WhiteListed(ip.to_string()))
        } else {
            Ok(())
        }
    }
}

impl WhiteBlackListInner {
    #[allow(clippy::type_complexity)]
    fn update_list(
        whitelist_path: &Path,
        blacklist_path: &Path,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
        Ok((
            Self::load_list(whitelist_path, false)?,
            Self::load_list(blacklist_path, false)?,
        ))
    }

    #[allow(clippy::type_complexity)]
    fn init_list(
        whitelist_path: &Path,
        blacklist_path: &Path,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
        Ok((
            Self::load_list(whitelist_path, true)?,
            Self::load_list(blacklist_path, true)?,
        ))
    }

    fn load_list(
        list_path: &Path,
        is_init: bool,
    ) -> Result<Option<HashSet<IpAddr>>, BootstrapError> {
        match std::fs::read_to_string(list_path) {
            Err(e) => {
                if is_init {
                    warn!(
                        "error on load whitelist/blacklist file : {} | {}",
                        list_path.to_str().unwrap_or(" "),
                        e
                    );
                }
                Ok(None)
            }
            Ok(list) => {
                let res = Some(
                    serde_json::from_str::<HashSet<IpAddr>>(list.as_str())
                        .map_err(|e| {
                            BootstrapError::InitListError(format!(
                                "Failed to parse bootstrap whitelist : {}",
                                e
                            ))
                        })?
                        .into_iter()
                        .map(normalize_ip)
                        .collect(),
                );
                Ok(res)
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct WhiteBlackListInner {
    white_list: Option<HashSet<IpAddr>>,
    black_list: Option<HashSet<IpAddr>>,
}
