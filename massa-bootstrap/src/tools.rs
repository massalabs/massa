use std::net::IpAddr;

/// Why not just to_canonical ?
/// Because the case in which the incoming ip is ipv4 but was mapped to ipv6 by the os,
/// it would fail the comparison with a canonicalized ipv4 from the config
/// (eg. Ipv4 is not converted to ipv6 by canonicalize)
pub(crate) fn normalize_ip(ip: IpAddr) -> IpAddr {
    let ip = match ip {
        IpAddr::V4(ip) => ip.to_ipv6_mapped(),
        IpAddr::V6(ip) => ip,
    };
    // to_canonical implementation (https://doc.rust-lang.org/src/core/net/ip_addr.rs.html#1733)
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return IpAddr::V4(mapped);
    }
    IpAddr::V6(ip)
}
