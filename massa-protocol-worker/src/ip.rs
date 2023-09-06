use std::net::IpAddr;

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
