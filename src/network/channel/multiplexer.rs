use failure::{bail, Error};
use num_enum::TryFromPrimitive;
use std::cmp;
use std::collections::{HashMap, VecDeque};
use std::convert::TryInto;
use std::{u16, u64};

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Copy, Clone)]
#[repr(u8)]
enum ChunkType {
    // 2 bit
    Start = 0,
    FullChunk = 1,
    PartialChunk = 2,
    Stop = 3,
}

#[derive(Debug, Eq, PartialEq, TryFromPrimitive, Copy, Clone)]
#[repr(u8)]
pub enum ObjectType {
    // 2 bit
    Block = 0,
    Transaction = 1,
    Peer = 2,
    Status = 3,
}

#[derive(Debug, Clone)]
struct RecvTransmissionStatus {
    active: bool,
    length: u32,
    cursor: u32,
    id: Vec<u8>,
}

impl Default for RecvTransmissionStatus {
    fn default() -> Self {
        RecvTransmissionStatus {
            active: false,
            length: 0,
            cursor: 0,
            id: Vec::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MultiplexerEvent {
    Start {
        obj_type: ObjectType,
        id: Vec<u8>,
        length: usize,
    },
    Data {
        obj_type: ObjectType,
        id: Vec<u8>,
        data: Vec<u8>,
    },
    Finish {
        obj_type: ObjectType,
        id: Vec<u8>,
    },
    Stop {
        obj_type: ObjectType,
        id: Vec<u8>,
    },
}

#[derive(Debug, Clone)]
pub struct MultiplexedReader {
    initialized: bool,
    version: u8,
    chunk_max_length: u16,
    chunk_cursor: usize,
    cur_chunk_type: ChunkType,
    cur_object_type: ObjectType,
    cur_object_number: u8,
    tmp_u24: u32,
    tmp_u16: u16,
    tmp_u8: u8,
    tmp_id: Vec<u8>,
    objects: [[RecvTransmissionStatus; 16]; 4],
    events: Vec<MultiplexerEvent>,
}

impl MultiplexedReader {
    pub fn new() -> Result<MultiplexedReader, Error> {
        Ok(MultiplexedReader {
            initialized: false,
            version: 0,
            chunk_max_length: u16::MAX,
            chunk_cursor: 0,
            cur_chunk_type: ChunkType::Stop,
            cur_object_type: ObjectType::Block,
            cur_object_number: 0,
            tmp_u24: 0,
            tmp_u16: 0,
            tmp_u8: 0,
            tmp_id: Vec::default(),
            objects: Default::default(),
            events: Vec::default(),
        })
    }

    fn read_handshake(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        let mut data_cursor = 0;
        if self.chunk_cursor == 0 {
            let (n_read, finished) = self.read_u8(&data[data_cursor..])?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
            if self.version > 0 {
                bail!("Remote has an incompatible multiplexer version");
            }
            self.version = self.tmp_u8;
        }
        if (1..=2).contains(&self.chunk_cursor) {
            let (n_read, finished) =
                self.read_u16_be(self.chunk_cursor - 1, &data[data_cursor..])?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
            if self.tmp_u16 == 0 {
                bail!("Remote attempted to set chunk max size to zero");
            }
            self.chunk_max_length = self.tmp_u16;
        }
        Ok((data_cursor, true))
    }

    fn read_start(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        let mut data_cursor = 0;
        if self.chunk_cursor == 0 {
            if self.objects[self.cur_object_type as usize][self.cur_object_number as usize].active {
                bail!("Cannot start new object if previous one is active on the same channel");
            }
            data_cursor += 1;
            self.chunk_cursor += 1;
        }
        if (1..=3).contains(&self.chunk_cursor) {
            let (n_read, finished) =
                self.read_u24_be(self.chunk_cursor - 1, &data[data_cursor..])?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
        }
        if self.chunk_cursor == 4 {
            let (n_read, finished) = self.read_u8(&data[data_cursor..])?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
        }
        if self.chunk_cursor >= 5 {
            let (n_read, finished) = self.read_id(
                (self.chunk_cursor - 5) as usize,
                self.tmp_u8 as usize,
                &data[data_cursor..],
            )?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
            self.object_start(
                self.cur_object_type,
                self.cur_object_number,
                self.tmp_u24,
                self.tmp_id.clone(),
            )?;
        }
        Ok((data_cursor, true))
    }

    fn read_full_chunk(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        let mut data_cursor = 0;
        if self.chunk_cursor == 0 {
            if !self.objects[self.cur_object_type as usize][self.cur_object_number as usize].active
            {
                bail!("Received chunk without active object");
            }
            data_cursor += 1;
            self.chunk_cursor += 1;
        }
        if self.chunk_cursor >= 1 {
            let (n_read, finished) = self.object_data(
                self.cur_object_type,
                self.cur_object_number,
                (self.chunk_cursor - 4) as u32,
                self.chunk_max_length as u32,
                &data[data_cursor..],
            )?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
        }
        Ok((data_cursor, true))
    }

    fn read_partial_chunk(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        let mut data_cursor = 0;
        if self.chunk_cursor == 0 {
            if !self.objects[self.cur_object_type as usize][self.cur_object_number as usize].active
            {
                bail!("Received chunk without active object");
            }
            data_cursor += 1;
            self.chunk_cursor += 1;
        }
        if (1..=2).contains(&self.chunk_cursor) {
            let (n_read, finished) =
                self.read_u16_be(self.chunk_cursor - 1, &data[data_cursor..])?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
        }
        if self.chunk_cursor >= 3 {
            let (n_read, finished) = self.object_data(
                self.cur_object_type,
                self.cur_object_number,
                (self.chunk_cursor - 6) as u32,
                std::cmp::min(self.chunk_max_length as u32, self.tmp_u16 as u32),
                &data[data_cursor..],
            )?;
            data_cursor += n_read;
            self.chunk_cursor += n_read;
            if !finished {
                return Ok((data_cursor, false));
            }
        }
        Ok((data_cursor, true))
    }

    fn read_stop(&mut self, _data: &[u8]) -> Result<(usize, bool), Error> {
        let mut data_cursor = 0;
        if self.chunk_cursor == 0 {
            if !self.objects[self.cur_object_type as usize][self.cur_object_number as usize].active
            {
                bail!("Received stop without active object");
            }
            data_cursor += 1;
            self.chunk_cursor += 1;
            self.object_stop(self.cur_object_type, self.cur_object_number)?;
        }
        Ok((data_cursor, true))
    }

    pub fn input(&mut self, data: &[u8]) -> Result<Vec<MultiplexerEvent>, Error> {
        let mut data_cursor: usize = 0;
        if !self.initialized {
            let (n_read, chunk_finished) = self.read_handshake(&data[data_cursor..])?;
            data_cursor += n_read;
            if !chunk_finished {
                return Ok(Vec::default());
            }
            self.initialized = true;
        }
        while data_cursor < data.len() {
            if self.chunk_cursor == 0 {
                // read header byte
                let header_byte = data[data_cursor];
                self.cur_chunk_type = ((header_byte & 0b11000000_u8) >> 6).try_into()?;
                self.cur_object_type = ((header_byte & 0b00110000_u8) >> 4).try_into()?;
                self.cur_object_number = header_byte & 0b00001111_u8;
            }
            let (n_read, chunk_finished) = match self.cur_chunk_type {
                ChunkType::Start => self.read_start(&data[data_cursor..])?,
                ChunkType::FullChunk => self.read_full_chunk(&data[data_cursor..])?,
                ChunkType::PartialChunk => self.read_partial_chunk(&data[data_cursor..])?,
                ChunkType::Stop => self.read_stop(&data[data_cursor..])?,
            };
            data_cursor += n_read;
            if chunk_finished {
                self.chunk_cursor = 0;
            }
        }
        Ok(self.events.drain(..).collect()) // return available events
    }

    fn read_u8(&mut self, data: &[u8]) -> Result<(usize, bool), Error> {
        if data.len() == 0 {
            return Ok((0, false));
        }
        self.tmp_u8 = data[0];
        Ok((1, true))
    }

    fn read_u16_be(&mut self, offset: usize, data: &[u8]) -> Result<(usize, bool), Error> {
        const N_BYTES: usize = 2;
        if offset >= N_BYTES {
            bail!("Attempting to read beyond expected number of bytes");
        }
        if offset == 0 {
            self.tmp_u16 = 0;
        }
        let read_length = cmp::min(N_BYTES - offset, data.len());
        for cursor in 0..read_length {
            self.tmp_u16 |= (data[cursor] as u16) << ((8 * (offset + cursor)) as usize);
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
            self.tmp_u24 = 0;
        }
        let read_length = cmp::min(N_BYTES - offset, data.len());
        for cursor in 0..read_length {
            self.tmp_u24 |= (data[cursor] as u32) << ((8 * (offset + cursor)) as usize);
        }
        let finished = (offset + read_length) == N_BYTES;
        Ok((read_length, finished))
    }

    fn read_id(
        &mut self,
        offset: usize,
        length: usize,
        data: &[u8],
    ) -> Result<(usize, bool), Error> {
        if offset == 0 {
            self.tmp_id = vec![0u8; length];
        }
        let read_length = std::cmp::min(length - offset, data.len());
        let finished = offset + read_length >= length;
        if read_length > 0 {
            self.tmp_id[offset..(offset + read_length)].clone_from_slice(&data[..read_length]);
        }
        Ok((read_length, finished))
    }

    fn object_start(
        &mut self,
        object_type: ObjectType,
        object_number: u8,
        length: u32,
        id: Vec<u8>,
    ) -> Result<(), Error> {
        for pobj in self.objects[object_type as usize].iter() {
            if pobj.active && pobj.id == id {
                bail!(
                    "Remote is transmitting simultaneously two objects with the same type and ID"
                );
            }
        }
        let obj = &mut self.objects[object_type as usize][object_number as usize];
        if obj.active {
            bail!("Attempting to start object transmission without closing the previous one");
        }
        obj.active = true;
        obj.length = length;
        obj.cursor = 0;
        obj.id = id;
        self.events.push(MultiplexerEvent::Start {
            obj_type: object_type,
            id: obj.id.clone(),
            length: length as usize,
        });
        Ok(())
    }

    fn object_data(
        &mut self,
        object_type: ObjectType,
        object_number: u8,
        offset: u32,
        max_n: u32,
        data: &[u8],
    ) -> Result<(usize, bool), Error> {
        let obj = &mut self.objects[object_type as usize][object_number as usize];
        if !obj.active {
            bail!("Attempting to write data to object before its transmission started");
        }
        let mut read_length: usize = data.len();
        let mut chunk_finished = false;
        let mut object_finished = false;
        if offset as usize + read_length >= max_n as usize {
            read_length = max_n as usize - offset as usize;
            chunk_finished = true;
        }
        if obj.cursor as usize + read_length >= obj.length as usize {
            read_length = obj.length as usize - obj.cursor as usize;
            chunk_finished = true;
            object_finished = true;
        }
        if read_length > 0 {
            self.events.push(MultiplexerEvent::Data {
                obj_type: object_type,
                id: obj.id.clone(),
                data: data[..read_length].to_vec(),
            });
        }
        if object_finished {
            obj.active = false;
            self.events.push(MultiplexerEvent::Finish {
                obj_type: object_type,
                id: obj.id.clone(),
            });
        }
        Ok((read_length, chunk_finished))
    }

    fn object_stop(&mut self, object_type: ObjectType, object_number: u8) -> Result<(), Error> {
        let obj = &mut self.objects[object_type as usize][object_number as usize];
        if !obj.active {
            bail!("Attempting to write data to object before its transmission started");
        }
        obj.active = false;
        self.events.push(MultiplexerEvent::Stop {
            obj_type: object_type,
            id: obj.id.clone(),
        });
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SendTransmissionStatus {
    last_activity_cycle: u64,
    active: bool,
    length: u32,
    cursor: u32,
    id: Vec<u8>,
    pending_data: VecDeque<u8>,
    starting: bool,
    stopping: bool,
    high_priority: bool,
}

impl Default for SendTransmissionStatus {
    fn default() -> Self {
        SendTransmissionStatus {
            last_activity_cycle: 0,
            active: false,
            length: 0,
            cursor: 0,
            id: Vec::default(),
            pending_data: VecDeque::default(),
            starting: false,
            stopping: false,
            high_priority: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MultiplexerWriterStatus {
    transmissions: [SendTransmissionStatus; 16],
    finished_ids: Vec<Vec<u8>>,
    stopped_ids: Vec<Vec<u8>>,
}

#[derive(Debug)]
pub struct MultiplexedWriter {
    initialized: bool,
    version: u8,
    chunk_max_length: u16,
    statuses: [MultiplexerWriterStatus; 4],
    id_to_num: [HashMap<Vec<u8>, u8>; 4],
    last_activity_cycle: [u64; 4],
    activity_cycle: u64,
}

impl MultiplexedWriter {
    pub fn new() -> Result<MultiplexedWriter, Error> {
        Ok(MultiplexedWriter {
            initialized: false,
            version: 0,
            chunk_max_length: u16::MAX,
            statuses: Default::default(),
            id_to_num: Default::default(),
            last_activity_cycle: Default::default(),
            activity_cycle: 0,
        })
    }

    pub fn object_start(
        &mut self,
        object_type: ObjectType,
        length: u32,
        id: Vec<u8>,
        high_priority: bool,
    ) -> Result<(), Error> {
        if length > 16777215 {
            // maximum value for 24 bits
            bail!("Length does not fit in 24 bits");
        }
        if id.len() > 255 {
            // maximum value for 8 bits
            bail!("ID length does not fit in one byte");
        }

        // check if ID already in use
        if self.id_to_num[object_type as usize].contains_key(&id) {
            bail!("An object with the same ID and type is already being sent")
        }

        // find a free slot
        let mut obj_num: Option<u8> = None;
        for (obj_i, obj) in self.statuses[object_type as usize]
            .transmissions
            .iter()
            .enumerate()
        {
            if !obj.active {
                obj_num = Some(obj_i as u8);
                break;
            }
        }
        let obj_num = match obj_num {
            Some(v) => v,
            None => bail!("All writing channels are occupied for this object type"),
        };

        // register
        self.id_to_num[object_type as usize].insert(id.clone(), obj_num);

        // start sending
        let activity_cycle = self.get_next_activity_cycle()?;
        let obj = &mut self.statuses[object_type as usize].transmissions[obj_num as usize];
        obj.active = true;
        obj.length = length;
        obj.cursor = 0;
        obj.id = id;
        obj.pending_data = VecDeque::default();
        obj.starting = true;
        obj.stopping = false;
        obj.last_activity_cycle = activity_cycle;
        obj.high_priority = high_priority;

        Ok(())
    }

    pub fn object_data(
        &mut self,
        object_type: ObjectType,
        id: &Vec<u8>,
        data: &Vec<u8>,
    ) -> Result<(), Error> {
        let obj_num = match self.id_to_num[object_type as usize].get(id) {
            Some(&v) => v,
            None => bail!("Object is not being sent"),
        };
        let obj = &mut self.statuses[object_type as usize].transmissions[obj_num as usize];
        if obj.stopping {
            bail!("Object transmission is being stopped: cannot feed new data");
        }
        if obj.cursor as usize + obj.pending_data.len() + data.len() > obj.length as usize {
            bail!("Data size exceeds advertized size for object");
        }
        obj.pending_data.extend(data);
        Ok(())
    }

    pub fn object_stop(&mut self, object_type: ObjectType, id: &Vec<u8>) -> Result<(), Error> {
        let obj_num = match self.id_to_num[object_type as usize].get(id) {
            Some(&v) => v,
            None => bail!("Object is not being sent"),
        };
        let obj = &mut self.statuses[object_type as usize].transmissions[obj_num as usize];
        if obj.stopping {
            bail!("Object transmission is already being stopped");
        }
        if obj.starting {
            // object not started yet: cancel starting and delete object
            obj.starting = false;
            obj.stopping = false;
            obj.active = false;
            obj.pending_data = VecDeque::default();
            self.id_to_num[object_type as usize].remove(&obj.id);
            self.statuses[object_type as usize]
                .stopped_ids
                .push(obj.id.drain(..).collect());
            return Ok(());
        }
        obj.stopping = true;
        obj.pending_data = VecDeque::default();
        Ok(())
    }

    pub fn get_chunk(&mut self) -> Result<Vec<u8>, Error> {
        if !self.initialized {
            self.initialized = true;
            let mut handshake: Vec<u8> = vec![self.version];
            handshake.extend(&self.chunk_max_length.to_be_bytes());
            return Ok(handshake);
        }

        // pick the next object to process
        let (obj_type, obj_num) = match self.pick_active()? {
            Some(v) => v,
            None => return Ok(Vec::default()),
        };
        let activity_cycle = self.get_next_activity_cycle()?;
        self.last_activity_cycle[obj_type as usize] = activity_cycle;
        let obj = &mut self.statuses[obj_type as usize].transmissions[obj_num as usize];

        // process object
        obj.last_activity_cycle = activity_cycle;
        if obj.starting {
            let chunk_h: u8 = ((ChunkType::Start as u8) << 6) | ((obj_type as u8) << 4) | obj_num;
            let mut chunk: Vec<u8> = vec![chunk_h];
            chunk.extend(&obj.length.to_be_bytes()[0..=2]);
            chunk.push(obj.id.len() as u8);
            chunk.extend(&obj.id);
            obj.starting = false;
            if obj.length == 0 {
                // object is already finished
                obj.active = false;
                self.id_to_num[obj_type as usize].remove(&obj.id);
                self.statuses[obj_type as usize]
                    .finished_ids
                    .push(obj.id.drain(..).collect());
            }
            return Ok(chunk);
        }
        if obj.stopping {
            let chunk_h: u8 = ((ChunkType::Stop as u8) << 6) | ((obj_type as u8) << 4) | obj_num;
            obj.stopping = false;
            obj.active = false;
            self.id_to_num[obj_type as usize].remove(&obj.id);
            self.statuses[obj_type as usize]
                .stopped_ids
                .push(obj.id.drain(..).collect());
            obj.pending_data = VecDeque::default();
            return Ok(vec![chunk_h]);
        }
        let n_data_bytes = cmp::min(
            cmp::min(
                self.chunk_max_length as usize,
                obj.length as usize - obj.cursor as usize,
            ),
            obj.pending_data.len(),
        );
        let mut chunk: Vec<u8>;
        if n_data_bytes == self.chunk_max_length as usize
            || obj.cursor as usize + n_data_bytes == obj.length as usize
        {
            let chunk_h: u8 =
                ((ChunkType::FullChunk as u8) << 6) | ((obj_type as u8) << 4) | obj_num;
            chunk = vec![chunk_h];
        } else {
            let chunk_h: u8 =
                ((ChunkType::PartialChunk as u8) << 6) | ((obj_type as u8) << 4) | obj_num;
            chunk = vec![chunk_h];
            chunk.extend(&(n_data_bytes as u16).to_be_bytes());
        }
        chunk.extend(obj.pending_data.drain(0..n_data_bytes).collect::<Vec<u8>>());
        obj.cursor += n_data_bytes as u32;
        if obj.cursor == obj.length {
            obj.active = false;
            obj.pending_data = VecDeque::default();
            self.id_to_num[obj_type as usize].remove(&obj.id);
            self.statuses[obj_type as usize]
                .finished_ids
                .push(obj.id.drain(..).collect());
        }
        Ok(chunk)
    }

    fn get_next_activity_cycle(&mut self) -> Result<u64, Error> {
        if self.activity_cycle == u64::MAX {
            let mut value_map: HashMap<u64, u64> = HashMap::new();
            for (type_i, &last_cycle) in self.last_activity_cycle.iter().enumerate() {
                value_map.insert(last_cycle, 0);
                for itm in self.statuses[type_i].transmissions.iter() {
                    value_map.insert(itm.last_activity_cycle, 0);
                }
            }
            let mut sorted_values: Vec<u64> = value_map.keys().cloned().collect();
            sorted_values.sort();
            for (new_val, &ans_val) in sorted_values.iter().enumerate() {
                value_map.insert(ans_val, new_val as u64);
            }
            for (type_i, last_cycle) in self.last_activity_cycle.iter_mut().enumerate() {
                *last_cycle = match value_map.get(last_cycle) {
                    Some(&v) => v,
                    None => bail!("Unexpected error while resetting activity cycle"),
                };
                for itm in self.statuses[type_i].transmissions.iter_mut() {
                    itm.last_activity_cycle = match value_map.get(&itm.last_activity_cycle) {
                        Some(&v) => v,
                        None => bail!("Unexpected error while resetting activity cycle"),
                    };
                }
            }
            self.activity_cycle = sorted_values.len() as u64;
            return Ok(self.activity_cycle);
        }
        self.activity_cycle += 1;
        return Ok(self.activity_cycle);
    }

    pub fn clear_inactive(&mut self, object_type: ObjectType) {
        self.statuses[object_type as usize].finished_ids = Vec::default();
        self.statuses[object_type as usize].stopped_ids = Vec::default();
    }

    fn pick_active(&mut self) -> Result<Option<(ObjectType, u8)>, Error> {
        // cost tuple (!high_priority, type_last_cycle, obj_last_cycle, type_num, obj_num)
        let mut selected_cost: Option<(bool, u64, u64, u8, u8)> = None;
        for (type_i, status) in self.statuses.iter().enumerate() {
            let last_type_cycle = self.last_activity_cycle[type_i];
            for (obj_i, obj) in status.transmissions.iter().enumerate() {
                if !obj.active || (!obj.starting && !obj.stopping && obj.pending_data.is_empty()) {
                    continue;
                }
                let cost = (
                    !obj.high_priority,
                    last_type_cycle,
                    obj.last_activity_cycle,
                    type_i as u8,
                    obj_i as u8,
                );
                selected_cost = match selected_cost {
                    None => Some(cost),
                    Some(cur_cost) if cost < cur_cost => Some(cost),
                    cur_v => cur_v,
                }
            }
        }
        Ok(match selected_cost {
            Some((_, _, _, t_n, o_n)) => Some((t_n.try_into()?, o_n)),
            None => None,
        })
    }

    fn get_status(&self, object_type: ObjectType) -> &MultiplexerWriterStatus {
        &self.statuses[object_type as usize]
    }
}
