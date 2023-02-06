use crate::multi::{MultiSig, MultiSigMsg};
use crate::*;
use anyhow::Error;
use massa_hash::Hash;
use tokio::join;

pub async fn start_multi_signature_scheme() -> Result<(), Error> {
    let (tx, _) = tokio::sync::broadcast::channel::<MultiSigMsg>(3);

    let keypair_1 = KeyPair::generate(1).unwrap();
    let keypair_2 = KeyPair::generate(1).unwrap();
    let keypair_3 = KeyPair::generate(1).unwrap();

    let pubkey_1 = keypair_1.get_public_key();
    let pubkey_2 = keypair_2.get_public_key();
    let pubkey_3 = keypair_2.get_public_key();

    let hash = Hash::compute_from(b"SomeData");
    let attacker_hash = Hash::compute_from(b"Some_Attacking_Data");

    let handle1 = tokio::spawn({
        let keypair_1 = keypair_1.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_1,
                vec![pubkey_2, pubkey_3],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle2 = tokio::spawn({
        let keypair_2 = keypair_2.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_2,
                vec![pubkey_1, pubkey_3],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle3 = tokio::spawn({
        let keypair_3 = keypair_3.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_3,
                vec![pubkey_1, pubkey_2],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    println!("ALL 3 tasks launched");

    let (res1, res2, res3) = join!(handle1, handle2, handle3);

    assert!(res1.is_ok() && res1.unwrap().is_ok());
    assert!(res2.is_ok() && res2.unwrap().is_ok());
    assert!(res3.is_ok() && res3.unwrap().is_ok());

    println!("ALL 3 tasks joined");

    let handle4 = tokio::spawn({
        let keypair_1 = keypair_1.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_1,
                vec![pubkey_2, pubkey_3],
                attacker_hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle5 = tokio::spawn({
        let keypair_2 = keypair_2.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_2,
                vec![pubkey_1, pubkey_3],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let handle6 = tokio::spawn({
        let keypair_3 = keypair_3.clone();
        let tx_clone = tx.clone();
        let rx_clone = tx_clone.subscribe();
        async move {
            handle_multi_signature_one_node(
                keypair_3,
                vec![pubkey_1, pubkey_2],
                hash,
                tx_clone,
                rx_clone,
            )
            .await?;

            Result::<(), Error>::Ok(())
        }
    });

    let (res4, res5, res6) = join!(handle4, handle5, handle6);

    assert!(res4.is_ok() && res4.unwrap().is_err());
    assert!(res5.is_ok() && res5.unwrap().is_err());
    assert!(res6.is_ok() && res6.unwrap().is_err());

    println!("ALL 3 tasks joined");

    Ok(())
}

pub async fn handle_multi_signature_one_node(
    our_keypair: KeyPair,
    other_pubkeys: Vec<PublicKey>,
    hash: Hash,
    tx: tokio::sync::broadcast::Sender<MultiSigMsg>,
    mut rx: tokio::sync::broadcast::Receiver<MultiSigMsg>,
) -> Result<(), Error> {
    let mut multi_sig = MultiSig::new(our_keypair.clone(), other_pubkeys.clone(), hash).unwrap();

    /* Commit stage */

    println!("KP: {} - Start commit stage", our_keypair.get_public_key());

    // Send our commitment to other keypairs
    tx.send(MultiSigMsg::Commitment(
        our_keypair.get_public_key(),
        multi_sig.get_our_commitment(),
    ))?;

    // Wait for every other commitment
    let mut num_commit = 0;
    while num_commit < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Commitment(pk, c)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_commitment(pk, c)?;
                num_commit += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all commit msg!",
        our_keypair.get_public_key()
    );

    /* Reveal stage */

    println!("KP: {} - Start reveal stage", our_keypair.get_public_key());

    // Send our reveal to other keypairs

    tx.send(MultiSigMsg::Reveal(
        our_keypair.get_public_key(),
        multi_sig.get_our_reveal().clone(),
    ))?;

    // Wait for every other reveal

    let mut num_reveal = 0;
    while num_reveal < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Reveal(pk, r)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_reveal(pk, r)?;
                num_reveal += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all reveal msg!",
        our_keypair.get_public_key()
    );

    /* Cosign stage */

    println!("KP: {} - Start cosign stage", our_keypair.get_public_key());

    // Send our cosignature to other keypairs

    tx.send(MultiSigMsg::Cosignature(
        our_keypair.get_public_key(),
        multi_sig.get_our_cosignature(),
    ))?;

    // Wait for every other reveal

    let mut num_cosignature = 0;
    while num_cosignature < other_pubkeys.len() {
        let res = rx.recv().await;

        match res {
            Ok(MultiSigMsg::Cosignature(pk, s)) if pk != our_keypair.get_public_key() => {
                multi_sig.set_other_cosignature(pk, s)?;
                num_cosignature += 1;
            }
            Ok(_) => {}
            _ => {}
        }
    }

    println!(
        "KP: {} - Received all cosign msg!",
        our_keypair.get_public_key()
    );

    /* EVERYONE SIGNED! */

    let signature = multi_sig.musig_cosig.as_ref().unwrap().sign().unwrap();

    match our_keypair.get_public_key() {
        PublicKey::PublicKeyV0(_) => Err(MassaSignatureError::InvalidVersionError(String::from(
            "Wrong PubKey version",
        ))
        .into()),
        PublicKey::PublicKeyV1(_pk) => {
            let t = schnorrkel::signing_context(b"massa_sig").bytes(hash.to_bytes());

            let aggregate_pk = multi_sig.musig_cosig.as_ref().unwrap().public_key();

            let result = aggregate_pk.verify(t, &signature);

            result.map_err(|_| {
                MassaSignatureError::MultiSignatureError(String::from(
                    "Could not verify the multi-signature",
                ))
                .into()
            })
        }
    }
}

fn original_multi_signature_simulation() {
    let keypairs: Vec<schnorrkel::Keypair> =
        (0..16).map(|_| schnorrkel::Keypair::generate()).collect();

    let t = schnorrkel::signing_context(b"multi-sig").bytes(b"We are legion!");
    let mut commits: Vec<_> = keypairs.iter().map(|k| k.musig(t.clone())).collect();
    for i in 0..commits.len() {
        let r = commits[i].our_commitment();
        for j in commits.iter_mut() {
            assert!(
                j.add_their_commitment(keypairs[i].public, r).is_ok() != (r == j.our_commitment())
            );
        }
    }

    let mut reveal_msgs: Vec<schnorrkel::musig::Reveal> = Vec::with_capacity(commits.len());
    let mut reveals: Vec<_> = commits.drain(..).map(|c| c.reveal_stage()).collect();
    for i in 0..reveals.len() {
        let r = reveals[i].our_reveal().clone();
        for j in reveals.iter_mut() {
            j.add_their_reveal(keypairs[i].public, r.clone()).unwrap();
        }
        reveal_msgs.push(r);
    }
    let pk = reveals[0].public_key();

    let mut cosign_msgs: Vec<schnorrkel::musig::Cosignature> = Vec::with_capacity(reveals.len());
    let mut cosigns: Vec<_> = reveals
        .drain(..)
        .map(|c| {
            assert_eq!(pk, c.public_key());
            c.cosign_stage()
        })
        .collect();
    for i in 0..cosigns.len() {
        assert_eq!(pk, cosigns[i].public_key());
        let r = cosigns[i].our_cosignature();
        for j in cosigns.iter_mut() {
            j.add_their_cosignature(keypairs[i].public, r).unwrap();
        }
        cosign_msgs.push(r);
        assert_eq!(pk, cosigns[i].public_key());
    }

    // let signature = cosigns[0].sign().unwrap();
    let mut c = schnorrkel::musig::collect_cosignatures(t.clone());
    for i in 0..cosigns.len() {
        c.add(keypairs[i].public, reveal_msgs[i].clone(), cosign_msgs[i])
            .unwrap();
    }
    let signature = c.signature();

    assert!(pk.verify(t, &signature).is_ok());
    for cosign in &cosigns {
        assert_eq!(pk, cosign.public_key());
        assert_eq!(signature, cosign.sign().unwrap());
    }
}

#[test]
fn test_multi_signature_simulation() {
    original_multi_signature_simulation();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_multi_signature() -> Result<(), Error> {
    start_multi_signature_scheme().await?;

    Ok(())
}
