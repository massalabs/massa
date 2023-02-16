use std::{
    collections::HashSet,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
};

use massa_logging::massa_trace;
use parking_lot::RwLock;

use crate::{error::BootstrapError, tools::normalize_ip};

/// A wrapper around the allow/block lists that allows efficient sharing between threads
// TODO: don't clone the path-bufs...
#[derive(Clone)]
pub(crate) struct SharedAllowBlockList {
    inner: Arc<RwLock<AllowBlockListInner>>,
    allow_path: PathBuf,
    block_path: PathBuf,
}

impl SharedAllowBlockList {
    pub(crate) fn new(allow_path: PathBuf, block_path: PathBuf) -> Self {
        let Ok((allow_list, block_list)) = AllowBlockListInner::load_whitelist_blacklist(&allow_path, &block_path) else {
            todo!("handle path loading error");
        };
        Self {
            inner: Arc::new(RwLock::new(AllowBlockListInner {
                allow_list,
                block_list,
            })),
            allow_path,
            block_path,
        }
    }
    pub(crate) fn update(&mut self) {
        let read_lock = self.inner.read();
        let Ok((new_allow, new_block)) = AllowBlockListInner::load_whitelist_blacklist(&self.allow_path, &self.block_path) else {
            todo!("handle reload error");
        };
        let allow_delta = new_allow != read_lock.allow_list;
        let block_delta = new_block != read_lock.block_list;
        if allow_delta || block_delta {
            drop(read_lock);
            let mut mut_inner = self.inner.write();

            if allow_delta {
                mut_inner.allow_list = new_allow;
            }
            if block_delta {
                mut_inner.block_list = new_block;
            }
        }
    }
    pub(crate) fn is_ip_allowed(&self, remote_addr: &SocketAddr) -> Result<(), String> {
        let ip = normalize_ip(remote_addr.ip());
        // whether the peer IP address is blacklisted
        let read = self.inner.read();
        let not_allowed_msg = if let Some(ip_list) = &read.block_list && ip_list.contains(&ip) {
            massa_trace!("bootstrap.lib.run.select.accept.refuse_blacklisted", {"remote_addr": remote_addr});
            Err(format!("IP {} is blacklisted", &ip))
            // whether the peer IP address is not present in the whitelist
        } else if let Some(ip_list) = &read.allow_list && !ip_list.contains(&ip){
            massa_trace!("bootstrap.lib.run.select.accept.refuse_not_whitelisted", {"remote_addr": remote_addr});
            Err(format!("A whitelist exists and the IP {} is not whitelisted", &ip))
        } else {
            Ok(())
        };
        return not_allowed_msg;
    }
}

impl AllowBlockListInner {
    #[allow(clippy::result_large_err)]
    #[allow(clippy::type_complexity)]
    fn load_whitelist_blacklist(
        allowlist_path: &PathBuf,
        blocklist_path: &PathBuf,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
        let whitelist = if let Ok(whitelist) = std::fs::read_to_string(allowlist_path) {
            Some(
                serde_json::from_str::<HashSet<IpAddr>>(whitelist.as_str())
                    .map_err(|_| {
                        BootstrapError::GeneralError(String::from(
                            "Failed to parse bootstrap whitelist",
                        ))
                    })?
                    .into_iter()
                    .map(normalize_ip)
                    .collect(),
            )
        } else {
            None
        };

        let blacklist = if let Ok(blacklist) = std::fs::read_to_string(blocklist_path) {
            Some(
                serde_json::from_str::<HashSet<IpAddr>>(blacklist.as_str())
                    .map_err(|_| {
                        BootstrapError::GeneralError(String::from(
                            "Failed to parse bootstrap blacklist",
                        ))
                    })?
                    .into_iter()
                    .map(normalize_ip)
                    .collect(),
            )
        } else {
            None
        };
        Ok((whitelist, blacklist))
    }
}

#[derive(Default)]
pub(crate) struct AllowBlockListInner {
    allow_list: Option<HashSet<IpAddr>>,
    block_list: Option<HashSet<IpAddr>>,
}
