mod client;
mod server;
use std::{
    io::{self, ErrorKind},
    time::{Duration, Instant},
};

pub(crate) use client::*;
pub(crate) use server::*;

trait BindingReadExact: io::Read {
    /// similar to std::io::Read::read_exact, but with a timeout that is function-global instead of per-individual-read
    fn read_exact_timeout(
        &mut self,
        buf: &mut [u8],
        deadline: Option<Instant>,
    ) -> Result<(), (std::io::Error, usize)> {
        let mut count = 0;
        self.set_read_timeout(None).map_err(|err| (err, count))?;
        while count < buf.len() {
            // update the timeout
            if let Some(deadline) = deadline {
                let dur = deadline.saturating_duration_since(Instant::now());
                if dur.is_zero() {
                    return Err((
                        std::io::Error::new(ErrorKind::TimedOut, "deadline has elapsed"),
                        count,
                    ));
                }
                self.set_read_timeout(Some(dur))
                    .map_err(|err| (err, count))?;
            }

            // do the read
            match self.read(&mut buf[count..]) {
                Ok(0) => break,
                Ok(n) => {
                    count += n;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err((e, count)),
            }
        }
        if count != buf.len() {
            Err((
                std::io::Error::new(ErrorKind::UnexpectedEof, "failed to fill whole buffer"),
                count,
            ))
        } else {
            Ok(())
        }
    }

    /// Internal helper
    fn set_read_timeout(&mut self, duration: Option<Duration>) -> Result<(), std::io::Error>;
}
