use std::{
    borrow::Cow,
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use massa_logging::massa_trace;
use parking_lot::RwLock;
use tracing::log::error;
use crate::error::BootstrapError;

use crate::tools::normalize_ip;

/// A wrapper around the white/black lists that allows efficient sharing between threads
// TODO: don't clone the path-bufs...
#[derive(Clone)]
pub(crate) struct SharedWhiteBlackList<'a> {
    inner: Arc<RwLock<WhiteBlackListInner>>,
    white_path: Cow<'a, Path>,
    black_path: Cow<'a, Path>,
}

impl SharedWhiteBlackList<'_> {
    pub(crate) fn new(white_path: PathBuf, black_path: PathBuf) -> Result<Self, BootstrapError> {
        let (white_list, black_list) =
            WhiteBlackListInner::load_white_black_lists(&white_path, &black_path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(WhiteBlackListInner {
                white_list,
                black_list,
            })),
            white_path: Cow::from(white_path),
            black_path: Cow::from(black_path),
        })
    }

    /// Checks if the white/black list is up to date with a read-lock
    /// Creates a new list, and replaces the old one in a write-lock
    pub(crate) fn update(&mut self) -> Result<(), BootstrapError> {
        let read_lock = self.inner.read();
        let (new_white, new_black) =
            WhiteBlackListInner::load_white_black_lists(&self.white_path, &self.black_path)?;
        let white_delta = new_white != read_lock.white_list;
        let black_delta = new_black != read_lock.black_list;
        if white_delta || black_delta {
            // Ideally this scope would be atomic
            let mut mut_inner = {
                drop(read_lock);
                self.inner.write()
            };

            if white_delta {
                mut_inner.white_list = new_white;
            }
            if black_delta {
                mut_inner.black_list = new_black;
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
    #[allow(clippy::result_large_err)]
    #[allow(clippy::type_complexity)]
    fn load_white_black_lists(
        whitelist_path: &Path,
        blacklist_path: &Path,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
        let white_list = Self::load_list(whitelist_path)?;
        let black_list = Self::load_list(blacklist_path)?;
        Ok((white_list, black_list))
    }

    fn load_list(list_path: &Path) -> Result<Option<HashSet<IpAddr>>, BootstrapError> {
        match std::fs::read_to_string(list_path) {
            Err(e) => {
                error!("error on load whitelist/blacklist file : {}", e);
                Ok(None)
            },
            Ok(list) => {
                let res = Some(
                    serde_json::from_str::<HashSet<IpAddr>>(list.as_str())
                        .map_err(|e| BootstrapError::InitListError(format!("Failed to parse bootstrap whitelist : {}", e.to_string())))?
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
