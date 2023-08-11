use std::net::IpAddr;

/// Why not just to_canonical ?
/// Because the case in which the incoming ip is ipv4 but was mapped to ipv6 by the os,
/// it would fail the comparison with a canonicalized ipv4 from the config
/// (eg. Ipv4 is not converted to ipv6 by canonicalize)
pub(crate) fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(ip) => ip.to_ipv6_mapped(),
        IpAddr::V6(ip) => ip,
    }
    .to_canonical()
}

/// Convert a mio stream to std
/// Adapted from Tokio
pub(crate) fn mio_stream_to_std(mio_socket: mio::net::TcpStream) -> std::net::TcpStream {
    #[cfg(unix)]
    {
        use std::os::unix::io::{FromRawFd, IntoRawFd};
        unsafe { std::net::TcpStream::from_raw_fd(mio_socket.into_raw_fd()) }
    }

    #[cfg(windows)]
    {
        use std::os::windows::io::{FromRawSocket, IntoRawSocket};
        unsafe { std::net::TcpStream::from_raw_socket(mio_socket.into_raw_socket()) }
    }

    #[cfg(target_os = "wasi")]
    {
        use std::os::wasi::io::{FromRawFd, IntoRawFd};
        unsafe { std::net::TcpStream::from_raw_fd(io.into_raw_fd()) }
    }
}
