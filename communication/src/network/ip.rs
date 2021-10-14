use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Ipv6MulticastScope};

impl IpAddr {
    // IpAddr::is_global from https://doc.rust-lang.org/stable/src/std/net/ip.rs.html#197-202
    pub const fn is_global(&self) -> bool {
        match self {
            IpAddr::V4(ip) => ip.is_global(),
            IpAddr::V6(ip) => ip.is_global(),
        }
    }
}

impl Ipv4Addr {
    // Ipv4Addr::is_global from https://doc.rust-lang.org/stable/src/std/net/ip.rs.html#549-568
    pub const fn is_global(&self) -> bool {
        // check if this address is 192.0.0.9 or 192.0.0.10. These addresses are the only two
        // globally routable addresses in the 192.0.0.0/24 range.
        if u32::from_be_bytes(self.octets()) == 0xc0000009
            || u32::from_be_bytes(self.octets()) == 0xc000000a
        {
            return true;
        }
        !self.is_private()
            && !self.is_loopback()
            && !self.is_link_local()
            && !self.is_broadcast()
            && !self.is_documentation()
            && !self.is_shared()
            && !self.is_ietf_protocol_assignment()
            && !self.is_reserved()
            && !self.is_benchmarking()
            // Make sure the address is not in 0.0.0.0/8
            && self.octets()[0] != 0
    }
}

impl Ipv6Addr {
    // Ipv6Addr::is_global https://doc.rust-lang.org/stable/src/std/net/ip.rs.html#1239-1245
    pub const fn is_global(&self) -> bool {
        match self.multicast_scope() {
            Some(Ipv6MulticastScope::Global) => true,
            None => self.is_unicast_global(),
            _ => false,
        }
    }
}
