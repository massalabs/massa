use std::net::IpAddr;

// to_canonical implementation (https://doc.rust-lang.org/src/core/net/ip_addr.rs.html#1733)
pub(crate) fn to_canonical(ip: IpAddr) -> IpAddr {
    match &ip {
        &v4 @ IpAddr::V4(_) => v4,
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return IpAddr::V4(mapped);
            }
            IpAddr::V6(*v6)
        }
    }
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
