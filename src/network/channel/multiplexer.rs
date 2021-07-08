use failure::{bail, Error};
use num_enum::TryFromPrimitive;
use std::cmp;
use std::convert::TryInto;
use std::u16;

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Clone)]
#[repr(u8)]
enum ChunkType {
    // 3 bit
    SetMaxChunkSize = 0,
    SingleFrame = 1,
    StartWithFullChunk = 2,
    StartWithPartialChunk = 3,
    FullChunk = 4,
    PartialChunk = 5,
    Stop = 6,
}

impl Default for ChunkType {
    fn default() -> Self {
        ChunkType::Stop {}
    }
}

#[derive(Debug, Clone)]
pub enum MultiplexerEvent {
    Data { data: Vec<u8> },
    Start { length: usize },
    Stop {},
    Finish {},
}

impl Default for MultiplexerEvent {
    fn default() -> Self {
        MultiplexerEvent::Stop {}
    }
}

#[derive(Debug, Clone)]
struct SubChannel {
    max_chunk_size: u16,
    transmission_initiated: bool,
    transmisison_length: u32,
    transmission_cursor: u32,
}

#[derive(Debug, Clone)]
pub struct MultiplexedReader {
    chunk_cursor: usize,
    chunk_subchannel: u8,
    chunk_type: ChunkType,
    length_24: u32,
    length_16: u16,
    subchannels: [SubChannel; 32],
    events: [Vec<MultiplexerEvent>; 32],
}

impl MultiplexedReader {
    pub fn input(&mut self, data: &[u8]) -> Result<[Vec<MultiplexerEvent>; 32], Error> {
        let mut data_cursor: usize = 0;
        while data_cursor < data.len() {
            if self.chunk_cursor == 0 {
                // we are at the beginning of a chunk: read header byte
                let header_byte = data[data_cursor];
                self.chunk_subchannel = (header_byte & 0b11111000_u8) >> 3;
                self.chunk_type = (header_byte & 0b00000111_u8).try_into()?;
            }
            match self.chunk_type {
                ChunkType::SetMaxChunkSize => {
                    match self.chunk_cursor {
                        0 => {
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        1..=2 => {
                            // big endian 16bit value
                            let (n_read, finished) =
                                self.read_u16_be(self.chunk_cursor - 1, &data[data_cursor..])?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                if self.length_16 == 0 {
                                    bail!("Attempting to set chunk size to zero");
                                }
                                self.subchannels[self.chunk_subchannel as usize].max_chunk_size =
                                    self.length_16;
                                self.chunk_reset()?;
                            }
                        }
                        _ => {
                            bail!("Unexpected position of reading cursor");
                        }
                    };
                }
                ChunkType::SingleFrame | ChunkType::StartWithFullChunk => {
                    match self.chunk_cursor {
                        0 => {
                            if self.subchannels[self.chunk_subchannel as usize]
                                .transmission_initiated
                            {
                                self.data_stop()?; // stop any existing object transmission
                            }
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        1..=3 => {
                            // big endian 24bit length
                            let (n_read, finished) =
                                self.read_u24_be(self.chunk_cursor - 1, &data[data_cursor..])?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.data_start(self.length_24)?;
                            }
                        }
                        _ => {
                            // read data
                            let max_read_n = if self.chunk_type == ChunkType::StartWithFullChunk {
                                self.subchannels[self.chunk_subchannel as usize].max_chunk_size
                                    as u32
                            } else {
                                self.length_24
                            };
                            let (n_read, finished) = self.data_chunk(
                                (self.chunk_cursor - 4) as u32,
                                max_read_n,
                                &data[data_cursor..],
                            )?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.chunk_reset()?;
                            }
                        }
                    };
                }
                ChunkType::StartWithPartialChunk => {
                    match self.chunk_cursor {
                        0 => {
                            if self.subchannels[self.chunk_subchannel as usize]
                                .transmission_initiated
                            {
                                self.data_stop()?; // stop any existing object transmission
                            }
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        1..=3 => {
                            // big endian 24bit length
                            let (n_read, finished) =
                                self.read_u24_be(self.chunk_cursor - 1, &data[data_cursor..])?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.data_start(self.length_24)?;
                            }
                        }
                        4..=5 => {
                            // big endian 16bit length
                            let (n_read, finished) =
                                self.read_u16_be(self.chunk_cursor - 4, &data[data_cursor..])?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished && self.length_16 == 0 {
                                bail!("Received chunk of size zero");
                            }
                        }
                        _ => {
                            // read data
                            let (n_read, finished) = self.data_chunk(
                                (self.chunk_cursor - 6) as u32,
                                self.length_16 as u32,
                                &data[data_cursor..],
                            )?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.chunk_reset()?;
                            }
                        }
                    };
                }
                ChunkType::FullChunk => {
                    match self.chunk_cursor {
                        0 => {
                            if !(self.subchannels[self.chunk_subchannel as usize]
                                .transmission_initiated)
                            {
                                bail!("Chunk received without transmission initiation");
                            }
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        _ => {
                            // read data
                            let max_read_n = self.subchannels[self.chunk_subchannel as usize]
                                .max_chunk_size as u32;
                            let (n_read, finished) = self.data_chunk(
                                (self.chunk_cursor - 1) as u32,
                                max_read_n,
                                &data[data_cursor..],
                            )?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.chunk_reset()?;
                            }
                        }
                    };
                }
                ChunkType::PartialChunk => {
                    match self.chunk_cursor {
                        0 => {
                            if !(self.subchannels[self.chunk_subchannel as usize]
                                .transmission_initiated)
                            {
                                bail!("Chunk received without transmission initiation");
                            }
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        1..=2 => {
                            // big endian 16bit length
                            let (n_read, finished) =
                                self.read_u16_be(self.chunk_cursor - 1, &data[data_cursor..])?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished && self.length_16 == 0 {
                                bail!("Received chunk of size zero");
                            }
                        }
                        _ => {
                            // read data
                            let (n_read, finished) = self.data_chunk(
                                (self.chunk_cursor - 3) as u32,
                                self.length_16 as u32,
                                &data[data_cursor..],
                            )?;
                            self.chunk_cursor += n_read;
                            data_cursor += n_read;
                            if finished {
                                self.chunk_reset()?;
                            }
                        }
                    };
                }
                ChunkType::Stop => {
                    match self.chunk_cursor {
                        0 => {
                            if !(self.subchannels[self.chunk_subchannel as usize]
                                .transmission_initiated)
                            {
                                bail!("Stop received without transmission initiation");
                            }
                            self.chunk_cursor += 1;
                            data_cursor += 1;
                        }
                        1 => {
                            self.data_stop()?;
                            self.chunk_reset()?;
                        }
                        _ => {
                            bail!("Unexpected position of reading cursor");
                        }
                    };
                }
            }
        }

        // move subchannel events and return them, clear their original vecs
        let mut return_events: [Vec<MultiplexerEvent>; 32] = Default::default();
        for chan_i in 0..32 {
            return_events[chan_i].extend(self.events[chan_i].drain(..));
        }
        Ok(return_events)
    }

    fn read_u16_be(&mut self, offset: usize, data: &[u8]) -> Result<(usize, bool), Error> {
        const N_BYTES: usize = 2;
        if offset >= N_BYTES {
            bail!("Attempting to read beyond expected number of bytes");
        }
        if offset == 0 {
            self.length_16 = 0;
        }
        let read_length = cmp::min(N_BYTES - offset, data.len());
        for cursor in 0..read_length {
            self.length_16 |= (data[cursor] as u16) << ((8 * (offset + cursor)) as usize);
        }
        let finished = (offset + read_length) == N_BYTES;
        Ok((read_length, finished))
    }

    fn read_u24_be(&mut self, offset: usize, data: &[u8]) -> Result<(usize, bool), Error> {
        const N_BYTES: usize = 3;
        if offset >= N_BYTES {
            bail!("Attempting to read beyond expected number of bytes");
        }
        if offset == 0 {
            self.length_24 = 0;
        }
        let read_length = cmp::min(N_BYTES - offset, data.len());
        for cursor in 0..read_length {
            self.length_24 |= (data[cursor] as u32) << ((8 * (offset + cursor)) as usize);
        }
        let finished = (offset + read_length) == N_BYTES;
        Ok((read_length, finished))
    }

    fn data_start(&mut self, length: u32) -> Result<(), Error> {
        let subchan = &mut self.subchannels[self.chunk_subchannel as usize];
        if subchan.transmission_initiated {
            bail!("Attempting to start transmission without closing the previous one");
        }
        if length == 0 {
            bail!("Attempting to start transmission with zero total length");
        }
        subchan.transmission_initiated = true;
        subchan.transmisison_length = length;
        subchan.transmission_cursor = 0;
        self.events[self.chunk_subchannel as usize].push(MultiplexerEvent::Start {
            length: length as usize,
        });
        Ok(())
    }

    fn data_chunk(&mut self, offset: u32, max_n: u32, data: &[u8]) -> Result<(usize, bool), Error> {
        let subchan = &mut self.subchannels[self.chunk_subchannel as usize];
        if !(subchan.transmission_initiated) {
            bail!("Atempting to write chunk to non-initiated transmission");
        }
        let read_length: usize = cmp::min((max_n - offset) as usize, data.len());
        let chunk_finished = (offset as usize + read_length) >= (max_n as usize);
        if read_length == 0 {
            return Ok((read_length, chunk_finished));
        }
        let events = &mut self.events[self.chunk_subchannel as usize];

        // append to last data event if any. Otherwise create a new one
        match events.len() {
            0 => {
                events.push(MultiplexerEvent::Data {
                    data: data[..read_length].to_vec(),
                });
            }
            n => {
                if let MultiplexerEvent::Data { data: data_vec } = &mut events[n - 1] {
                    data_vec.extend(data[..read_length].iter());
                } else {
                    events.push(MultiplexerEvent::Data {
                        data: data[..read_length].to_vec(),
                    });
                }
            }
        }

        subchan.transmission_cursor += read_length as u32;
        if subchan.transmission_cursor >= subchan.transmisison_length {
            subchan.transmission_initiated = false;
            subchan.transmisison_length = 0;
            subchan.transmission_cursor = 0;
            events.push(MultiplexerEvent::Finish {});
        }

        Ok((read_length, chunk_finished))
    }

    fn data_stop(&mut self) -> Result<(), Error> {
        let subchan = &mut self.subchannels[self.chunk_subchannel as usize];
        if !(subchan.transmission_initiated) {
            bail!("Atempting to stop non-initiated transmission");
        }

        subchan.transmission_initiated = false;
        subchan.transmisison_length = 0;
        subchan.transmission_cursor = 0;

        // add a STOP event, or collapse a previous START event if no data in between
        let events = &mut self.events[self.chunk_subchannel as usize];
        match events.len() {
            0 => {
                events.push(MultiplexerEvent::Stop {});
            }
            n => {
                if let MultiplexerEvent::Start { .. } = &events[n - 1] {
                    events.pop();
                } else {
                    events.push(MultiplexerEvent::Stop {});
                }
            }
        }

        Ok(())
    }

    fn chunk_reset(&mut self) -> Result<(), Error> {
        self.chunk_cursor = 0;
        self.chunk_subchannel = 0;
        self.chunk_type = ChunkType::default();
        self.length_24 = 0;
        self.length_16 = 0;
        Ok(())
    }
}

pub struct MultiplexedWriter {
    //TODO
}
