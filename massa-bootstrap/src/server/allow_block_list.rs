use std::{
    borrow::Cow,
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use massa_logging::massa_trace;
use parking_lot::RwLock;

use crate::tools::normalize_ip;

/// A wrapper around the allow/block lists that allows efficient sharing between threads
// TODO: don't clone the path-bufs...
#[derive(Clone)]
pub(crate) struct SharedAllowBlockList<'a> {
    inner: Arc<RwLock<AllowBlockListInner>>,
    allow_path: Cow<'a, Path>,
    block_path: Cow<'a, Path>,
}

impl SharedAllowBlockList<'_> {
    pub(crate) fn new(allow_path: PathBuf, block_path: PathBuf) -> Result<Self, String> {
        let (allow_list, block_list) =
            AllowBlockListInner::load_allow_block_lists(&allow_path, &block_path)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(AllowBlockListInner {
                allow_list,
                block_list,
            })),
            allow_path: Cow::from(allow_path),
            block_path: Cow::from(block_path),
        })
    }

    /// Checks if the allow/block list is up to date with a read-lock
    /// Creates a new list, and replaces the old one in a write-lock
    pub(crate) fn update(&mut self) -> Result<(), String> {
        let read_lock = self.inner.read();
        let (new_allow, new_block) =
            AllowBlockListInner::load_allow_block_lists(&self.allow_path, &self.block_path)?;
        let allow_delta = new_allow != read_lock.allow_list;
        let block_delta = new_block != read_lock.block_list;
        if allow_delta || block_delta {
            // Ideally this scope would be atomic
            let mut mut_inner = {
                drop(read_lock);
                self.inner.write()
            };

            if allow_delta {
                mut_inner.allow_list = new_allow;
            }
            if block_delta {
                mut_inner.block_list = new_block;
            }
        }
        Ok(())
    }

    #[cfg_attr(test, allow(unreachable_code, unused_variables))]
    pub(crate) fn is_ip_allowed(&self, remote_addr: &SocketAddr) -> Result<(), String> {
        #[cfg(test)]
        return Ok(());

        let ip = normalize_ip(remote_addr.ip());
        // whether the peer IP address is blacklisted
        let read = self.inner.read();
        if let Some(ip_list) = &read.block_list && ip_list.contains(&ip) {
            massa_trace!("bootstrap.lib.run.select.accept.refuse_blacklisted", {"remote_addr": remote_addr});
            Err(format!("IP {} is blacklisted", &ip))
            // whether the peer IP address is not present in the whitelist
        } else if let Some(ip_list) = &read.allow_list && !ip_list.contains(&ip){
            massa_trace!("bootstrap.lib.run.select.accept.refuse_not_whitelisted", {"remote_addr": remote_addr});
            Err(format!("A whitelist exists and the IP {} is not whitelisted", &ip))
        } else {
            Ok(())
        }
    }
}

impl AllowBlockListInner {
    #[allow(clippy::result_large_err)]
    #[allow(clippy::type_complexity)]
    fn load_allow_block_lists(
        allowlist_path: &Path,
        blocklist_path: &Path,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), String> {
        let allow_list = Self::load_list(allowlist_path)?;
        let block_list = Self::load_list(blocklist_path)?;
        Ok((allow_list, block_list))
    }

    fn load_list(list_path: &Path) -> Result<Option<HashSet<IpAddr>>, String> {
        let Ok(list) = std::fs::read_to_string(list_path) else {
            return Ok(None);
        };
        let res = Some(
            serde_json::from_str::<HashSet<IpAddr>>(list.as_str())
                .map_err(|_| String::from("Failed to parse bootstrap whitelist"))?
                .into_iter()
                .map(normalize_ip)
                .collect(),
        );
        Ok(res)
    }
}

#[derive(Default)]
pub(crate) struct AllowBlockListInner {
    allow_list: Option<HashSet<IpAddr>>,
    block_list: Option<HashSet<IpAddr>>,
}
