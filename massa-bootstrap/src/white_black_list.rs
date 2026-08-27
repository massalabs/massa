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

use crate::tools::to_canonical;

/// A wrapper around the white/black lists that allows efficient sharing between threads
// TODO: don't clone the path-bufs...
#[derive(Clone, Debug)]
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
        // Canonicalize on insert so the in-memory list matches the canonical
        // form used by `is_ip_allowed` (and by `load_list` on reload). Otherwise
        // a non-canonical entry such as `::ffff:a.b.c.d` would fail to block the
        // equivalent IPv4 peer until the next file reload.
        let ips = ips.into_iter().map(to_canonical).collect::<Vec<_>>();
        let mut write_lock = self.inner.write();
        if let Some(black_list) = &mut write_lock.black_list {
            black_list.extend(ips);
        } else {
            write_lock.black_list = Some(HashSet::from_iter(ips));
        };
        self.write_to_file(&self.black_path, write_lock.black_list.as_ref().unwrap())?;
        Ok(())
    }

    /// Remove IPs address from the black list
    pub fn remove_ips_from_blacklist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let ips = ips.into_iter().map(to_canonical).collect::<Vec<_>>();
        let mut write_lock = self.inner.write();
        if let Some(black_list) = &mut write_lock.black_list {
            for ip in ips {
                black_list.remove(&ip);
            }
            self.write_to_file(&self.black_path, black_list)?;
        }
        Ok(())
    }

    /// Add IP address to the white list
    pub fn add_ips_to_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        // See `add_ips_to_blacklist`: canonicalize on insert for consistency
        // with the canonicalized membership check in `is_ip_allowed`.
        let ips = ips.into_iter().map(to_canonical).collect::<Vec<_>>();
        let mut write_lock = self.inner.write();
        if let Some(white_list) = &mut write_lock.white_list {
            white_list.extend(ips);
        } else {
            write_lock.white_list = Some(HashSet::from_iter(ips));
        };
        self.write_to_file(&self.white_path, write_lock.white_list.as_ref().unwrap())?;
        Ok(())
    }

    /// Remove IPs address from the white list
    pub fn remove_ips_from_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), BootstrapError> {
        let ips = ips.into_iter().map(to_canonical).collect::<Vec<_>>();
        let mut write_lock = self.inner.write();
        if let Some(white_list) = &mut write_lock.white_list {
            for ip in ips {
                white_list.remove(&ip);
            }
            self.write_to_file(&self.white_path, white_list)?;
        }
        Ok(())
    }

    /// write list to file
    fn write_to_file(
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
        // Read the files before taking the lock, so a slow or hung filesystem
        // read cannot stall `is_ip_allowed` callers.
        let white_file = WhiteBlackListInner::read_list_file(&self.white_path);
        let black_file = WhiteBlackListInner::read_list_file(&self.black_path);
        let read_lock = self.inner.read();
        let new_white_file =
            WhiteBlackListInner::refresh_list(white_file, &read_lock.white_list, "whitelist")?;
        let new_black_file =
            WhiteBlackListInner::refresh_list(black_file, &read_lock.black_list, "blacklist")?;
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

    pub(crate) fn is_ip_allowed(&self, remote_addr: &SocketAddr) -> Result<(), BootstrapError> {
        let ip = to_canonical(remote_addr.ip());
        // whether the peer IP address is blacklisted
        let read = self.inner.read();
        if let Some(ip_list) = &read.black_list {
            if ip_list.contains(&ip) {
                massa_trace!("bootstrap.lib.run.select.accept.refuse_blacklisted", {"remote_addr": remote_addr});
                return Err(BootstrapError::BlackListed(ip.to_string()));
            }
            // whether the peer IP address is not present in the whitelist
        }
        if let Some(ip_list) = &read.white_list {
            if !ip_list.contains(&ip) {
                massa_trace!("bootstrap.lib.run.select.accept.refuse_not_whitelisted", {"remote_addr": remote_addr});
                return Err(BootstrapError::WhiteListed(ip.to_string()));
            }
        }
        Ok(())
    }
}

/// Outcome of reading a list file, distinguishing an explicitly absent file
/// from one that exists but cannot be read.
enum ListFileRead {
    /// The file does not exist: the list is deliberately not configured.
    Missing,
    /// The file exists but could not be read.
    Unreadable(std::io::Error),
    /// The file content, still to be parsed.
    Content(String),
}

impl WhiteBlackListInner {
    #[allow(clippy::type_complexity)]
    fn init_list(
        whitelist_path: &Path,
        blacklist_path: &Path,
    ) -> Result<(Option<HashSet<IpAddr>>, Option<HashSet<IpAddr>>), BootstrapError> {
        Ok((
            Self::load_list(whitelist_path, "whitelist")?,
            Self::load_list(blacklist_path, "blacklist")?,
        ))
    }

    /// Load a list at startup. A missing or unreadable file means the list is
    /// not configured: the feature is disabled with a warning.
    fn load_list(
        list_path: &Path,
        list_kind: &str,
    ) -> Result<Option<HashSet<IpAddr>>, BootstrapError> {
        match std::fs::read_to_string(list_path) {
            Err(e) => {
                warn!(
                    "error on load whitelist/blacklist file : {} | {}",
                    list_path.to_str().unwrap_or(" "),
                    e
                );
                Ok(None)
            }
            Ok(list) => Ok(Some(Self::parse_list(&list, list_kind)?)),
        }
    }

    /// Read a list file, keeping the distinction between "the file does not
    /// exist" and "the file exists but cannot be read".
    fn read_list_file(list_path: &Path) -> ListFileRead {
        match std::fs::read_to_string(list_path) {
            Ok(content) => ListFileRead::Content(content),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => ListFileRead::Missing,
            Err(e) => ListFileRead::Unreadable(e),
        }
    }

    /// Compute the new value of a list for a periodic refresh.
    ///
    /// A missing file is an explicit operator choice: the list is disabled. A
    /// file that exists but cannot be read must NOT disable a configured list:
    /// bootstrap access control would silently fail open if the whitelist file
    /// became unreadable at runtime, so the previously loaded list is kept.
    fn refresh_list(
        file: ListFileRead,
        current: &Option<HashSet<IpAddr>>,
        list_kind: &str,
    ) -> Result<Option<HashSet<IpAddr>>, BootstrapError> {
        match file {
            ListFileRead::Content(list) => Ok(Some(Self::parse_list(&list, list_kind)?)),
            ListFileRead::Missing => {
                if current.is_some() {
                    warn!(
                        "bootstrap {} file no longer exists: disabling the list",
                        list_kind
                    );
                }
                Ok(None)
            }
            ListFileRead::Unreadable(e) => {
                warn!(
                    "failed to read bootstrap {} file: {} | keeping the previously loaded list",
                    list_kind, e
                );
                Ok(current.clone())
            }
        }
    }

    /// Parse the JSON content of a list file into a canonicalized IP set.
    fn parse_list(list: &str, list_kind: &str) -> Result<HashSet<IpAddr>, BootstrapError> {
        Ok(serde_json::from_str::<HashSet<IpAddr>>(list)
            .map_err(|e| {
                BootstrapError::InitListError(format!(
                    "Failed to parse bootstrap {} : {}",
                    list_kind, e
                ))
            })?
            .into_iter()
            .map(to_canonical)
            .collect())
    }
}

#[derive(Default, Debug)]
pub(crate) struct WhiteBlackListInner {
    white_list: Option<HashSet<IpAddr>>,
    black_list: Option<HashSet<IpAddr>>,
}

#[cfg(test)]
mod tests {
    use super::SharedWhiteBlackList;
    use crate::error::BootstrapError;
    use std::net::{IpAddr, SocketAddr};
    use tempfile::TempDir;

    #[test]
    fn blacklisting_mapped_ipv6_blocks_equivalent_ipv4_immediately() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        let list = SharedWhiteBlackList::new(white, black).unwrap();

        // Add the IPv4-mapped IPv6 form of 127.0.0.2 through the private API.
        let mapped: IpAddr = "::ffff:127.0.0.2".parse().unwrap();
        list.add_ips_to_blacklist(vec![mapped]).unwrap();

        // The equivalent plain IPv4 peer must be blocked right away, without
        // waiting for a file reload to canonicalize the stored entry.
        let peer = SocketAddr::new("127.0.0.2".parse().unwrap(), 12345);
        assert!(matches!(
            list.is_ip_allowed(&peer),
            Err(BootstrapError::BlackListed(_))
        ));
    }

    #[test]
    fn whitelist_file_removal_disables_the_list() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        std::fs::write(&white, r#"["127.0.0.1"]"#).unwrap();
        let mut list = SharedWhiteBlackList::new(white.clone(), black).unwrap();

        let refused = SocketAddr::new("127.0.0.2".parse().unwrap(), 12345);
        assert!(matches!(
            list.is_ip_allowed(&refused),
            Err(BootstrapError::WhiteListed(_))
        ));

        // Deleting the file is an explicit operator action: the refresh must
        // disable the whitelist.
        std::fs::remove_file(&white).unwrap();
        list.update().unwrap();

        assert!(list.is_ip_allowed(&refused).is_ok());
    }

    #[test]
    fn unreadable_whitelist_file_keeps_the_loaded_list() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        std::fs::write(&white, r#"["127.0.0.1"]"#).unwrap();
        let mut list = SharedWhiteBlackList::new(white.clone(), black).unwrap();

        let allowed = SocketAddr::new("127.0.0.1".parse().unwrap(), 12345);
        let refused = SocketAddr::new("127.0.0.2".parse().unwrap(), 12345);
        assert!(list.is_ip_allowed(&allowed).is_ok());
        assert!(matches!(
            list.is_ip_allowed(&refused),
            Err(BootstrapError::WhiteListed(_))
        ));

        // The path exists but reading it fails (it is now a directory): the
        // refresh must keep the previously loaded list instead of failing open.
        std::fs::remove_file(&white).unwrap();
        std::fs::create_dir(&white).unwrap();
        list.update().unwrap();

        assert!(list.is_ip_allowed(&allowed).is_ok());
        assert!(matches!(
            list.is_ip_allowed(&refused),
            Err(BootstrapError::WhiteListed(_))
        ));
    }

    #[test]
    fn blacklist_file_removal_disables_the_list() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        std::fs::write(&black, r#"["127.0.0.2"]"#).unwrap();
        let mut list = SharedWhiteBlackList::new(white, black.clone()).unwrap();

        let blocked = SocketAddr::new("127.0.0.2".parse().unwrap(), 12345);
        assert!(matches!(
            list.is_ip_allowed(&blocked),
            Err(BootstrapError::BlackListed(_))
        ));

        std::fs::remove_file(&black).unwrap();
        list.update().unwrap();

        assert!(list.is_ip_allowed(&blocked).is_ok());
    }

    #[test]
    fn refresh_without_any_configured_list_keeps_access_open() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        // no file at all: the common case of a node without access lists
        let mut list = SharedWhiteBlackList::new(white, black).unwrap();

        list.update().unwrap();

        let peer = SocketAddr::new("127.0.0.1".parse().unwrap(), 12345);
        assert!(list.is_ip_allowed(&peer).is_ok());
    }

    #[test]
    fn recreated_whitelist_file_is_reloaded_on_refresh() {
        let dir = TempDir::new().unwrap();
        let white = dir.path().join("whitelist.json");
        let black = dir.path().join("blacklist.json");
        std::fs::write(&white, r#"["127.0.0.1"]"#).unwrap();
        let mut list = SharedWhiteBlackList::new(white.clone(), black).unwrap();

        // the file disappears, then comes back with a different content:
        // the refresh must pick up the new list
        std::fs::remove_file(&white).unwrap();
        list.update().unwrap();
        std::fs::write(&white, r#"["127.0.0.3"]"#).unwrap();
        list.update().unwrap();

        let newly_allowed = SocketAddr::new("127.0.0.3".parse().unwrap(), 12345);
        let formerly_allowed = SocketAddr::new("127.0.0.1".parse().unwrap(), 12345);
        assert!(list.is_ip_allowed(&newly_allowed).is_ok());
        assert!(matches!(
            list.is_ip_allowed(&formerly_allowed),
            Err(BootstrapError::WhiteListed(_))
        ));
    }
}
