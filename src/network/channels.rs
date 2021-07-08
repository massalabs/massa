use failure::{bail, Error};
use rand::rngs::OsRng;
use rand::Rng;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::time::{timeout, Duration};

use crate::config::NetworkConfig;
use crate::crypto::*;

#[derive(Debug)]
pub struct CommunicationChannel {
    pub public_key: PublicKey,
    pub socket: TcpStream,
}

pub async fn establish_channel(
    network_config: &NetworkConfig,
    secret_key: &SecretKey,
    public_key: &PublicKey,
    mut socket: TcpStream,
) -> Result<CommunicationChannel, Error> {
    const HANDSHAKE_RANDOM_BYTES: usize = 32;
    // split socket for full duplex operation
    let (mut reader, mut writer) = socket.split();

    // prepare local handshake message (randomnes + pubkey) + signature
    let local_randomnes;
    let local_handshake_data;
    {
        let mut local_randomnes_mut = [0u8; HANDSHAKE_RANDOM_BYTES];
        let mut rng = OsRng::default();
        rng.try_fill(&mut local_randomnes_mut)?;
        local_randomnes = local_randomnes_mut;
        let local_pubkey = public_key.serialize_compressed();
        let data_to_sign = [&local_randomnes[..], &local_pubkey[..]].concat();
        let signature = secret_key.generate_signature(&data_to_sign)?;
        local_handshake_data = [&data_to_sign[..], &signature[..]].concat();
    }

    // send local handshake and read remote handshake
    let mut remote_handshake_data =
        [0u8; HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE + SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::try_join!(
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                writer.write_all(&local_handshake_data)
            ),
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                reader.read_exact(&mut remote_handshake_data)
            )
        )?;
        res1?;
        res2?;
        //TODO stop when one fails (not just when it times out)
    }

    // parse and verify remote handshake, prepare local response
    let remote_pubkey;
    let local_repsonse_data;
    {
        let mut remote_randomnes = [0u8; HANDSHAKE_RANDOM_BYTES];
        remote_randomnes.copy_from_slice(&remote_handshake_data[..HANDSHAKE_RANDOM_BYTES]);
        remote_pubkey = PublicKey::parse_slice(
            &remote_handshake_data
                [(HANDSHAKE_RANDOM_BYTES)..(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)],
            Some(PublicKeyFormat::Compressed),
        )?;
        if remote_pubkey == *public_key {
            bail!("Remote public key same as local public key");
        }
        let mut sig = [0u8; SIGNATURE_SIZE];
        sig.copy_from_slice(
            &remote_handshake_data[(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)..],
        );
        remote_pubkey.verify_signature(
            &remote_handshake_data[..(HANDSHAKE_RANDOM_BYTES + COMPRESSED_PUBLIC_KEY_SIZE)],
            &sig,
        )?;
        local_repsonse_data = secret_key.generate_signature(&remote_randomnes)?;
    }

    // send local response and read remote response
    let mut remote_response_data = [0u8; SIGNATURE_SIZE];
    {
        let (res1, res2) = tokio::try_join!(
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                writer.write_all(&local_repsonse_data)
            ),
            timeout(
                Duration::from_secs_f32(network_config.timeout),
                reader.read_exact(&mut remote_response_data)
            )
        )?;
        res1?;
        res2?;
        //TODO stop when one fails (not just when it times out)
    }

    // check their signature of our randomnes
    remote_pubkey.verify_signature(&local_randomnes, &remote_response_data)?;

    // return remote public key
    Ok(CommunicationChannel {
        public_key: remote_pubkey,
        socket: socket,
    })
}
