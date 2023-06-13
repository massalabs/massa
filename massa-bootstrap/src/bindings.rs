mod client;
mod server;
use std::{
    io::{self, ErrorKind},
    time::{Duration, Instant},
};

pub(crate) use client::*;
pub(crate) use server::*;

use crate::BootstrapError;

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

trait BindingWriteExact: io::Write {
    fn write_all_timeout(
        &mut self,
        write_buf: &[u8],
        deadline: Option<Instant>,
    ) -> Result<(), (std::io::Error, usize)> {
        self.set_write_timeout(None).map_err(|e| (e, 0))?;
        let mut total_bytes_written = 0;

        while total_bytes_written < write_buf.len() {
            if let Some(deadline) = deadline {
                let dur = deadline.saturating_duration_since(Instant::now());
                if dur.is_zero() {
                    return Err((
                        std::io::Error::new(ErrorKind::TimedOut, "deadline has elapsed"),
                        total_bytes_written,
                    ));
                }
                self.set_write_timeout(Some(dur))
                    .map_err(|e| (e, total_bytes_written))?;
            }

            match self.write(&write_buf[total_bytes_written..]) {
                Ok(bytes_written) => {
                    total_bytes_written += bytes_written;
                }
                Err(err) => {
                    return Err((err, total_bytes_written));
                }
            }
        }

        Ok(())
    }
    /// Internal helper
    fn set_write_timeout(&mut self, duration: Option<Duration>) -> Result<(), std::io::Error>;
}
