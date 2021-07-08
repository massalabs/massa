//RUST_BACKTRACE=1 cargo test test_block_validity -- --nocapture

use super::super::consensus_controller::{ConsensusController, ConsensusControllerInterface};
use super::super::default_consensus_controller::DefaultConsensusController;
use super::mock_protocol_controller::{self};
use super::tools;
use crypto::{hash::Hash, signature::SignatureEngine};
use time::UTime;

//use time::UTime;

#[tokio::test]
async fn test_block_validity() {
    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    //set time longuer than the test.
    cfg.t0 = 25000.into();
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let cnss_cmd = cnss.get_interface();

    let genesis_hashes = cnss_cmd
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create valid block
    let valid_block_p2t0 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        2,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create block with wrong thread (2)
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        2,
        1,
        genesis_hashes.clone(),
        false,
        false,
    )
    .await;

    //create block with wrong slot number 0 (2)
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        0,
        genesis_hashes.clone(),
        false,
        false,
    )
    .await;

    //create block with a parent in the future
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        vec![valid_block_p2t0, genesis_hashes[1]],
        false,
        false,
    )
    .await;

    //create block with wrong signature
    let (invalid_hash, mut invalid_block, block_private_key) =
        tools::create_block(&cfg, 0, 15, genesis_hashes.clone());
    let signature_engine = SignatureEngine::new();
    //use other hash
    invalid_block.signature = signature_engine
        .sign(&genesis_hashes[1], &block_private_key)
        .unwrap();
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &invalid_block)
        .await;
    //see if the block is not propagated.
    assert!(
        !tools::validate_notpropagate_block(&mut protocol_controler_interface, invalid_hash, 500)
            .await
    );

    //create block with wrong public key (creator)
    let (invalid_hash, mut invalid_block, block_private_key) =
        tools::create_block(&cfg, 0, 15, genesis_hashes.clone());
    //change creator and rehash
    invalid_block.header.creator = signature_engine.derive_public_key(&cfg.genesis_key);
    let signature_engine = SignatureEngine::new();
    let header_hash = invalid_block.header.compute_hash().unwrap();
    invalid_block.signature = signature_engine
        .sign(&header_hash, &block_private_key)
        .unwrap();
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &invalid_block)
        .await;
    //see if the block is not propagated.
    assert!(
        !tools::validate_notpropagate_block(&mut protocol_controler_interface, invalid_hash, 500)
            .await
    );

    //create a valids block for thread 0
    let valid_hash1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let valid_hash2 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //Create other clique
    let (block_clique_hash, valid_block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::hash("Other hash!".as_bytes()),
        0,
        1,
        genesis_hashes.clone(),
    );
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &valid_block)
        .await;
    tools::validate_propagate_block(&mut protocol_controler_interface, block_clique_hash, 1000)
        .await;

    //validate block with parent in same slot same thread
    let (child_clique_hash2, valid_block, _) =
        tools::create_block(&cfg, 0, 3, vec![valid_hash1, block_clique_hash]);
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &valid_block)
        .await;
    //see if the block is propagated.
    assert!(
        !tools::validate_notpropagate_block(
            &mut protocol_controler_interface,
            child_clique_hash2,
            1000
        )
        .await
    );

    //Test parent in different clique
    //create a child for clique1.
    let child_clique_hash1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        2,
        vec![valid_hash1, valid_hash2],
        true,
        false,
    )
    .await;

    //create a child for clique2.
    let child_clique_hash2 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        2,
        vec![block_clique_hash, valid_hash2],
        true,
        false,
    )
    .await;

    //create a block with parent on the different clique.
    let (child_clique_hash2, valid_block, _) =
        tools::create_block(&cfg, 0, 3, vec![child_clique_hash1, child_clique_hash2]);
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &valid_block)
        .await;
    //see if the block is propagated.
    assert!(
        !tools::validate_notpropagate_block(
            &mut protocol_controler_interface,
            child_clique_hash2,
            1000
        )
        .await
    );

    //validate block with parent in different slot
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        3,
        vec![child_clique_hash1, valid_hash2],
        true,
        false,
    )
    .await;

    //validate block arrive several slot after the parent where other parent childs exist before.
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        4,
        vec![valid_hash1, valid_hash2],
        true,
        false,
    )
    .await;

    /*println!(
        "time {:?}",
        UTime::now().unwrap().saturating_sub(cfg.genesis_timestamp)
    );*/
}

#[tokio::test]
async fn test_ti() {
    /*    stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap(); */

    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;
    //to avoir timing pb for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let cnss_cmd = cnss.get_interface();
    let genesis_hashes = cnss_cmd
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    //create a valids block for thread 0
    let valid_hasht0s1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let valid_hasht1s1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //one click with 2 block compatible
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let block1_clic = tools::get_cliques(&block_graph, valid_hasht0s1);
    let block2_clic = tools::get_cliques(&block_graph, valid_hasht1s1);
    assert_eq!(1, block1_clic.len());
    assert_eq!(1, block2_clic.len());
    assert_eq!(block1_clic, block2_clic);

    //Create other clique bock T0S2
    let (fork_block_hash, block, _) = tools::create_block_with_merkle_root(
        &cfg,
        Hash::hash("Other hash!".as_bytes()),
        0,
        2,
        genesis_hashes.clone(),
    );

    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &block)
        .await;
    tools::validate_propagate_block(&mut protocol_controler_interface, fork_block_hash, 1000).await;
    //two clique with valid_hasht0s1 and valid_hasht1s1 in one and fork_block_hash, valid_hasht1s1 in the other
    //test the first clique hasn't changed.
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let block1_clic = tools::get_cliques(&block_graph, valid_hasht0s1);
    let block2_clic = tools::get_cliques(&block_graph, valid_hasht1s1);
    assert_eq!(1, block1_clic.len());
    assert_eq!(2, block2_clic.len());
    assert!(block2_clic.intersection(&block1_clic).next().is_some());
    //test the new click
    let fork_clic = tools::get_cliques(&block_graph, fork_block_hash);
    assert_eq!(1, fork_clic.len());
    assert!(fork_clic.intersection(&block1_clic).next().is_none());
    assert!(fork_clic.intersection(&block2_clic).next().is_some());

    //extend first clique
    let mut parentt0sn_hash = valid_hasht0s1;
    for slot in 3..=35 {
        let block_hash = tools::create_and_test_block(
            &mut protocol_controler_interface,
            &cfg,
            node_ids[0].1.clone(),
            0,
            slot,
            vec![parentt0sn_hash, valid_hasht1s1],
            true,
            false,
        )
        .await;
        //validate the added block isn't in the forked block click.
        let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
        let block_clic = tools::get_cliques(&block_graph, block_hash);
        let fork_clic = tools::get_cliques(&block_graph, fork_block_hash);
        assert!(fork_clic.intersection(&block_clic).next().is_none());

        parentt0sn_hash = block_hash;
    }

    //create new block in other clique
    let (invalid_block_hasht1s2, block, _) =
        tools::create_block(&cfg, 1, 2, vec![fork_block_hash, valid_hasht1s1]);
    protocol_controler_interface
        .receive_block(node_ids[0].1.clone(), &block)
        .await;
    assert!(
        !tools::validate_notpropagate_block(
            &mut protocol_controler_interface,
            invalid_block_hasht1s2,
            1000,
        )
        .await
    );
    //verify that the clique has been pruned.
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let fork_clic = tools::get_cliques(&block_graph, fork_block_hash);
    assert_eq!(0, fork_clic.len());
}

#[tokio::test]
async fn test_gpi() {
    // // setup logging
    /*stderrlog::new()
    .verbosity(4)
    .timestamp(stderrlog::Timestamp::Millisecond)
    .init()
    .unwrap();*/

    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;
    //to avoir timing pb for block in the futur
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let cnss_cmd = cnss.get_interface();
    let genesis_hashes = cnss_cmd
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 1 normal block in each thread (t0s1 and t1s1) with genesis parents
    //create a valids block for thread 0
    let valid_hasht0s1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let valid_hasht1s1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //one click with 2 block compatible
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let block1_clic = tools::get_cliques(&block_graph, valid_hasht0s1);
    let block2_clic = tools::get_cliques(&block_graph, valid_hasht1s1);
    assert_eq!(1, block1_clic.len());
    assert_eq!(1, block2_clic.len());
    assert_eq!(block1_clic, block2_clic);

    //create 2 clique
    // * create 1 block in t0s2 with parents of slots (t0s1, t1s0)
    let valid_hasht0s2 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        2,
        vec![valid_hasht0s1, genesis_hashes[1]],
        true,
        false,
    )
    .await;
    // * create 1 block in t1s2 with parents of slots (t0s0, t1s1)
    let valid_hasht1s2 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        2,
        vec![genesis_hashes[0], valid_hasht1s1],
        true,
        false,
    )
    .await;

    // * after processing the block in t1s2, the block of t0s2 is incompatible with block of t1s2 (link in gi)
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let blockt1s2_clic = tools::get_cliques(&block_graph, valid_hasht1s2);
    let blockt0s2_clic = tools::get_cliques(&block_graph, valid_hasht0s2);
    assert!(blockt1s2_clic
        .intersection(&blockt0s2_clic)
        .next()
        .is_none());
    // * after processing the block in t1s2, there are 2 cliques, one with block of t0s2 and one with block of t1s2, and the parent vector uses the clique of minimum hash sum so the block of minimum hash between t0s2 and t1s2
    assert_eq!(1, blockt1s2_clic.len());
    assert_eq!(1, blockt0s2_clic.len());
    let parents = block_graph.best_parents.clone();
    if valid_hasht1s2 > valid_hasht0s2 {
        assert_eq!(parents[0], valid_hasht0s2)
    } else {
        assert_eq!(parents[1], valid_hasht1s2)
    }

    // * continue with 33 additional blocks in thread 0, that extend the clique of the block in t0s2:
    //    - a block in slot t0sX has parents (t0sX-1, t1s1), for X from 3 to 35
    let mut parentt0sn_hash = valid_hasht0s2;
    for slot in 3..=35 {
        let block_hash = tools::create_and_test_block(
            &mut protocol_controler_interface,
            &cfg,
            node_ids[0].1.clone(),
            0,
            slot,
            vec![parentt0sn_hash, valid_hasht1s1],
            true,
            false,
        )
        .await;
        parentt0sn_hash = block_hash;
    }
    // * create 1 block in t1s2 with the genesis blocks as parents
    tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        3,
        vec![valid_hasht0s1, valid_hasht1s2],
        false,
        false,
    )
    .await;

    // * after processing the 33 blocks, one clique is removed (too late),
    //   the block of minimum hash becomes final, the one of maximum hash becomes stale
    //verify that the clique has been pruned.
    let block_graph = cnss_cmd.get_block_graph_status().await.unwrap();
    let fork_clic = tools::get_cliques(&block_graph, valid_hasht1s2);
    assert_eq!(0, fork_clic.len());
    assert!(block_graph.discarded_blocks.set.contains(&valid_hasht1s2));
    assert!(block_graph.active_blocks.contains_key(&valid_hasht0s2));
    assert!(!block_graph.active_blocks.contains_key(&valid_hasht1s2));
}

#[tokio::test]
async fn test_old_stale() {
    // // setup logging
    // stderrlog::new()
    //     .verbosity(4)
    //     .timestamp(stderrlog::Timestamp::Millisecond)
    //     .init()
    //     .unwrap();

    let node_ids = tools::create_node_ids(1);

    let (protocol_controller, mut protocol_controler_interface) = mock_protocol_controller::new();
    let mut cfg = tools::default_consensus_config(&node_ids);
    cfg.t0 = 32000.into();
    cfg.delta_f0 = 32;

    //to avoid timing pb for block in the future
    cfg.genesis_timestamp = UTime::now()
        .unwrap()
        .saturating_sub(cfg.t0.checked_mul(1000).unwrap());

    let cnss = DefaultConsensusController::new(&cfg, protocol_controller)
        .await
        .expect("Could not create consensus controller");
    let cnss_cmd = cnss.get_interface();
    let genesis_hashes = cnss_cmd
        .get_block_graph_status()
        .await
        .expect("could not get block graph status")
        .genesis_blocks;

    // * create 40 normal blocks in each thread: in slot 1 they have genesis parents, in slot 2 they have slot 1 parents
    //create a valid block for slot 1
    let mut valid_hasht0 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    //create a valid block on the other thread.
    let mut valid_hasht1 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        1,
        1,
        genesis_hashes.clone(),
        true,
        false,
    )
    .await;

    // and loop for the 39 other blocks
    for i in 0..39 {
        valid_hasht0 = tools::create_and_test_block(
            &mut protocol_controler_interface,
            &cfg,
            node_ids[0].1.clone(),
            0,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;

        //create a valid block on the other thread.
        valid_hasht1 = tools::create_and_test_block(
            &mut protocol_controler_interface,
            &cfg,
            node_ids[0].1.clone(),
            1,
            i + 2,
            vec![valid_hasht0, valid_hasht1],
            true,
            false,
        )
        .await;
    }

    //create 1 block in thread 0 slot 1 with genesis parents
    let _valid_hasht0s2 = tools::create_and_test_block(
        &mut protocol_controler_interface,
        &cfg,
        node_ids[0].1.clone(),
        0,
        1,
        genesis_hashes.clone(),
        false,
        false,
    )
    .await;
}
