use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
};

use ip_rfc::global;
use peernet::transports::TransportType;

// TODO: Use std one when stable
pub(crate) fn to_canonical(ip: IpAddr) -> IpAddr {
    match ip {
        v4 @ IpAddr::V4(_) => v4,
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return IpAddr::V4(mapped);
            }
            IpAddr::V6(v6)
        }
    }
}

/// Returns true if `addr` is an endpoint the peer management pipeline is allowed
/// to probe or to re-advertise to other peers.
///
/// Announcements and peer lists are signed, but their content is entirely
/// peer-controlled: without this check a peer could make us open connections to
/// (or gossip around) loopback, private, link-local or otherwise reserved
/// addresses. Only globally routable endpoints are kept, unless the node is
/// configured to deal with local peers (see [`allow_local_peers`]).
///
/// [`allow_local_peers`]: massa_protocol_exports::ProtocolConfig::allow_local_peers
pub(crate) fn is_routable_peer_addr(addr: &SocketAddr, allow_local_peers: bool) -> bool {
    let ip = to_canonical(addr.ip());
    // a null port is never connectable, whatever the address class
    if addr.port() == 0 {
        return false;
    }
    // multicast and broadcast are seen as global by `ip_rfc` but can never be a
    // peer endpoint, so they are refused even when local peers are allowed
    if ip.is_multicast() || matches!(ip, IpAddr::V4(ipv4) if ipv4.is_broadcast()) {
        return false;
    }
    allow_local_peers || global(&ip)
}

/// Drops from `listeners` every endpoint we must not probe nor re-advertise.
/// See [`is_routable_peer_addr`].
pub(crate) fn filter_routable_listeners(
    listeners: HashMap<SocketAddr, TransportType>,
    allow_local_peers: bool,
) -> HashMap<SocketAddr, TransportType> {
    listeners
        .into_iter()
        .filter(|(addr, _)| is_routable_peer_addr(addr, allow_local_peers))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{filter_routable_listeners, is_routable_peer_addr};
    use peernet::transports::TransportType;
    use std::collections::HashMap;
    use std::net::SocketAddr;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn test_only_global_addresses_are_routable() {
        assert!(is_routable_peer_addr(&addr("1.2.3.4:31244"), false));
        assert!(is_routable_peer_addr(&addr("[2001:db9::1]:31244"), false));

        // non global address classes are refused
        for non_global in [
            "127.0.0.1:31244",
            "0.0.0.0:31244",
            "192.168.1.2:31244",
            "10.0.0.1:31244",
            "169.254.1.1:31244",
            "224.0.0.1:31244",
            "[::1]:31244",
            "[fe80::1]:31244",
            "[fc00::1]:31244",
            // ipv4 mapped ipv6 must be canonicalized before being checked
            "[::ffff:127.0.0.1]:31244",
        ] {
            assert!(
                !is_routable_peer_addr(&addr(non_global), false),
                "{} should not be routable",
                non_global
            );
        }

        // a null port is never connectable, even for a global address
        assert!(!is_routable_peer_addr(&addr("1.2.3.4:0"), false));
        assert!(!is_routable_peer_addr(&addr("1.2.3.4:0"), true));
    }

    #[test]
    fn test_local_peers_allowed_by_configuration() {
        assert!(is_routable_peer_addr(&addr("127.0.0.1:31244"), true));
        assert!(is_routable_peer_addr(&addr("192.168.1.2:31244"), true));
        // ... but multicast and broadcast never are
        assert!(!is_routable_peer_addr(&addr("224.0.0.1:31244"), true));
        assert!(!is_routable_peer_addr(&addr("255.255.255.255:31244"), true));
        assert!(!is_routable_peer_addr(&addr("[ff02::1]:31244"), true));
    }

    #[test]
    fn test_filter_routable_listeners() {
        let listeners = HashMap::from([
            (addr("1.2.3.4:31244"), TransportType::Tcp),
            (addr("127.0.0.1:31245"), TransportType::Tcp),
            (addr("192.168.1.2:31246"), TransportType::Quic),
        ]);

        let filtered = filter_routable_listeners(listeners.clone(), false);
        assert_eq!(filtered.len(), 1);
        assert!(filtered.contains_key(&addr("1.2.3.4:31244")));

        assert_eq!(filter_routable_listeners(listeners, true).len(), 3);
    }
}
