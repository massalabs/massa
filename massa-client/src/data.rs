use crate::ReplError;
use bitcoin_hashes;
use chrono::Local;
use chrono::TimeZone;
use crypto::signature::PublicKey;
use crypto::signature::Signature;
use models::operation::Operation;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use time::UTime;

pub const HASH_SIZE_BYTES: usize = 32;
pub static FORMAT_SHORT_HASH: AtomicBool = AtomicBool::new(true); //never set to zero.

pub fn compare_hash(
    (hash1, (slot1, _thread1)): &(Hash, (u64, u8)),
    (hash2, (slot2, _thread2)): &(Hash, (u64, u8)),
) -> std::cmp::Ordering {
    match slot1.cmp(&slot2) {
        std::cmp::Ordering::Equal => hash1.cmp(&hash2),
        val @ _ => val,
    }
}

#[derive(Clone, Debug, Deserialize)]
pub enum DiscardReason {
    Invalid,
    Stale,
    Final,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
    pub signature: Signature,
}
impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let signature = self.signature.to_string();
        let signature = if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            &signature[..4]
        } else {
            &signature
        };
        write!(f, "{} signature:{} operations:....", self.header, signature)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeader {
    pub creator: PublicKey,
    pub thread_number: u8,
    pub period_number: u64,
    pub roll_number: u32,
    pub parents: Vec<Hash>,
    pub endorsements: Vec<Option<Signature>>,
    pub out_ledger_hash: Hash,
    pub operation_merkle_root: Hash, // all operations hash
}
impl std::fmt::Display for BlockHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let pk = self.creator.to_string();
        let pk = if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            &pk[..4]
        } else {
            &pk
        };
        writeln!(
            f,
            "creator: {} periode:{} th:{} roll:{} ledger:{} merkle_root:{} parents:{:?} endorsements:{:?}",
            pk,
            self.period_number,
            self.thread_number,
            self.roll_number,
            self.out_ledger_hash,
            self.operation_merkle_root,
            self.parents,
            self.endorsements
        )
        //        writeln!(f, "  parents:{:?}", self.parents)?;
        //        writeln!(f, "  endorsements:{:?}", self.endorsements)
    }
}

#[derive(Clone, Deserialize)]
pub struct State {
    time: UTime,
    latest_slot: Option<(u64, u8)>,
    our_ip: Option<IpAddr>,
    last_final: Vec<(Hash, u64, u8, UTime)>,
    nb_cliques: usize,
    nb_peers: usize,
}
impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let duration: Duration = self.time.into();

        let date = Local.timestamp(duration.as_secs() as i64, 0);
        write!(
            f,
            "  Time: {:?} Latest:{}",
            date,
            self.latest_slot
                .map(|(sl, th)| format!("Period {} Thread {}", sl, th))
                .unwrap_or("None".to_string())
        )?;
        write!(
            f,
            " Nb peers: {}, our IP: {}",
            self.nb_peers,
            self.our_ip
                .map(|i| i.to_string())
                .unwrap_or("None".to_string())
        )?;
        let mut final_blocks: Vec<&(Hash, u64, u8, UTime)> = self.last_final.iter().collect();
        final_blocks.sort_unstable_by(|a, b| compare_hash(&(a.0, (a.1, a.2)), &(b.0, (b.1, b.2))));

        write!(
            f,
            " Nb cliques: {}, last final blocks:{:?}",
            self.nb_cliques,
            final_blocks
                .iter()
                .map(|(hash, slot, th, date)| format!(
                    " {} per:{} th:{} {:?}",
                    hash,
                    slot,
                    th,
                    Local.timestamp(Into::<Duration>::into(*date).as_secs() as i64, 0)
                ))
                .collect::<Vec<String>>()
        )
    }
}

#[derive(Clone, Deserialize)]
pub struct StakerInfo {
    staker_active_blocks: Vec<(Hash, BlockHeader)>,
    staker_discarded_blocks: Vec<(Hash, DiscardReason, BlockHeader)>,
    staker_next_draws: Vec<(u64, u8)>,
}
impl std::fmt::Display for StakerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "  active blocks:")?;
        let mut blocks: Vec<&(Hash, BlockHeader)> = self.staker_active_blocks.iter().collect();
        blocks.sort_unstable_by(|a, b| {
            compare_hash(
                &(a.0, (a.1.period_number, a.1.thread_number)),
                &(b.0, (b.1.period_number, b.1.thread_number)),
            )
        });
        for (hash, block) in &blocks {
            write!(f, "    block: hash:{} header: {}", hash, block)?;
        }
        writeln!(f, "  discarded blocks:")?;
        let mut blocks: Vec<&(Hash, DiscardReason, BlockHeader)> =
            self.staker_discarded_blocks.iter().collect();
        blocks.sort_unstable_by(|a, b| {
            compare_hash(
                &(a.0, (a.2.period_number, a.2.thread_number)),
                &(b.0, (b.2.period_number, b.2.thread_number)),
            )
        });
        for (hash, reason, block) in &blocks {
            write!(
                f,
                "    block: hash:{} reason:{:?} header: {}",
                hash, reason, block
            )?;
        }
        writeln!(
            f,
            "  staker_next_draws{:?}:",
            self.staker_next_draws
                .iter()
                .map(|(slot, th)| format!("(per:{},th:{})", slot, th))
                .collect::<Vec<String>>()
        )
    }
}

#[derive(Clone, Deserialize)]
pub struct NetworkInfo {
    our_ip: Option<IpAddr>,
    peers: HashMap<IpAddr, PeerInfo>,
}
impl std::fmt::Display for NetworkInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "  Our IP address: {}",
            self.our_ip
                .map(|i| i.to_string())
                .unwrap_or("None".to_string())
        )?;
        writeln!(f, "  Peers:")?;
        for peer in self.peers.values() {
            write!(f, "    {}", peer)?;
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
pub struct PeerInfo {
    pub ip: IpAddr,
    pub banned: bool,
    pub bootstrap: bool,
    pub last_alive: Option<UTime>,
    pub last_failure: Option<UTime>,
    pub advertised: bool,
    pub active_out_connection_attempts: usize,
    pub active_out_connections: usize,
    pub active_in_connections: usize,
}
impl std::fmt::Display for PeerInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "Peer: Ip:{} bootstrap:{} banned:{} last_alive:{} last_failure:{} act_out_attempts:{} act_out:{} act_in:{}"
            ,self.ip
            , self.banned
            , self.bootstrap
            , self.last_alive.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.last_failure.map(|t| format!("{:?}",Local.timestamp(Into::<Duration>::into(t).as_secs() as i64, 0))).unwrap_or("None".to_string())
            , self.active_out_connection_attempts
            , self.active_out_connections
            , self.active_in_connections)
    }
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash)]
pub struct Hash(bitcoin_hashes::sha256::Hash);

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.to_bs58_check()[..4])
        } else {
            write!(f, "{}", &self.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if FORMAT_SHORT_HASH.load(Ordering::Relaxed) {
            write!(f, "{}", &self.to_bs58_check()[..4])
        } else {
            write!(f, "{}", &self.to_bs58_check())
        }
    }
}

impl Hash {
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }
    pub fn from_bs58_check(data: &str) -> Result<Hash, ReplError> {
        bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| ReplError::HashParseError(format!("{:?}", err)))
            .and_then(|s| Hash::from_bytes(&s))
    }
    pub fn to_bytes(&self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        *self.0.as_inner()
    }
    pub fn from_bytes(data: &[u8]) -> Result<Hash, ReplError> {
        use bitcoin_hashes::Hash;
        use std::convert::TryInto;
        let res_inner: Result<<bitcoin_hashes::sha256::Hash as Hash>::Inner, _> = data.try_into();
        res_inner
            .map(|inner| Hash(bitcoin_hashes::sha256::Hash::from_inner(inner)))
            .map_err(|err| ReplError::HashParseError(format!("{:?}", err)))
    }
}
impl<'de> ::serde::Deserialize<'de> for Hash {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Hash::from_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bs58_check(&v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}
