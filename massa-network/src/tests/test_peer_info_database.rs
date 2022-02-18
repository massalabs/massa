use crate::PeerInfo;

use massa_time::MassaTime;
use std::collections::HashMap;
use std::net::IpAddr;

#[cfg(test)]
mod tests {
    use crate::{
        error::NetworkConnectionErrorType,
        peer_info_database::{cleanup_peers, PeerInfoDatabase},
        NetworkError, NetworkSettings,
    };

    use super::*;
    use serial_test::serial;
    use tokio::sync::watch;

    #[tokio::test]
    #[serial]
    async fn test_try_new_in_connection_in_connection_closed() {
        let network_settings = NetworkSettings {
            target_out_nonbootstrap_connections: 5,
            ..Default::default()
        };
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle =
            tokio::spawn(async move { while let Ok(()) = saver_watch_rx.changed().await {} });

        let mut db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("ToManyConnectionAttempt error not return");
        }

        db.try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)))
            .expect_err("not global ip not detected.");
        db.try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)))
            .expect_err("local ip not detected.");

        db.try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .expect("in connection not accepted.");
        db.try_new_in_connection(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)))
            .expect_err("banned peer not detected.");

        // test with a not connected peer
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            panic!("ToManyConnectionAttempt error not return");
        }

        // test with a not connected peer
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            panic!("PeerInfoNotFoundError error not return");
        }

        db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db.in_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("ToManyConnectionAttempt error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_out_connection_attempt_failed() {
        let network_settings = NetworkSettings {
            target_out_nonbootstrap_connections: 5,
            ..Default::default()
        };
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle =
            tokio::spawn(async move { while let Ok(()) = saver_watch_rx.changed().await {} });

        let mut db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            println!("res: {:?}", res);
            panic!("ToManyConnectionFailure error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();

        // peer not found.
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            println!("res: {:?}", res);
            panic!("PeerInfoNotFoundError error not return");
        }
        // peer with no attempt.
        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            println!("res: {:?}", res);
            panic!("ToManyConnectionFailure error not return");
        }

        // call ok.
        db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .expect("out_connection_attempt_failed failed");

        let res =
            db.out_connection_attempt_failed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionFailure(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("ToManyConnectionFailure error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_try_out_connection_attempt_success() {
        let network_settings = NetworkSettings {
            target_out_nonbootstrap_connections: 5,
            ..Default::default()
        };
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers1.bootstrap = true;
        connected_peers1.banned = true;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());

        let saver_join_handle =
            tokio::spawn(async move { while let Ok(()) = saver_watch_rx.changed().await {} });

        let mut db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no connection attempt before
        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 11,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("ToManyConnectionAttempt error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();

        // peer not found.
        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 13,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)), ip_err);
        } else {
            println!("res: {:?}", res);
            panic!("PeerInfoNotFoundError error not return");
        }

        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 11,
            )))
            .unwrap();
        assert!(res, "try_out_connection_attempt_success failed");

        let res = db.try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
            169, 202, 0, 12,
        )));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            panic!("PeerInfoNotFoundError error not return");
        }

        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)))
            .unwrap();
        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 12,
            )))
            .unwrap();
        assert!(!res, "try_out_connection_attempt_success not banned");
    }

    #[tokio::test]
    #[serial]
    async fn test_new_out_connection_closed() {
        let network_settings = NetworkSettings {
            max_out_nonbootstrap_connection_attempts: 5,
            ..Default::default()
        };
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, mut saver_watch_rx) = watch::channel(peers.clone());
        let saver_join_handle =
            tokio::spawn(async move { while let Ok(()) = saver_watch_rx.changed().await {} });

        let mut db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        //
        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("CloseConnectionWithNoConnectionToClose error not return");
        }

        // add a new connection attempt
        db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db
            .try_out_connection_attempt_success(&IpAddr::V4(std::net::Ipv4Addr::new(
                169, 202, 0, 11,
            )))
            .unwrap();
        assert!(res, "try_out_connection_attempt_success failed");

        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            panic!("PeerInfoNotFoundError error not return");
        }

        db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
            .unwrap();
        let res = db.out_connection_closed(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::CloseConnectionWithNoConnectionToClose(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("CloseConnectionWithNoConnectionToClose error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_new_out_connection_attempt() {
        let network_settings = NetworkSettings {
            max_out_nonbootstrap_connection_attempts: 5,
            ..Default::default()
        };
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let mut db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)));
        if let Err(NetworkError::InvalidIpError(ip_err)) = res {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 11)), ip_err);
        } else {
            panic!("InvalidIpError not return");
        }

        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::PeerInfoNotFoundError(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)), ip_err);
        } else {
            panic!("PeerInfoNotFoundError error not return");
        }

        (0..5).for_each(|_| {
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)))
                .unwrap()
        });
        let res =
            db.new_out_connection_attempt(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        if let Err(NetworkError::PeerConnectionError(
            NetworkConnectionErrorType::ToManyConnectionAttempt(ip_err),
        )) = res
        {
            assert_eq!(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)), ip_err);
        } else {
            panic!("ToManyConnectionAttempt error not return");
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_get_advertisable_peer_ips() {
        let network_settings = NetworkSettings::default();
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer banned not return.
        let mut banned_host1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = true;
        banned_host1.banned = true;
        banned_host1.last_alive = Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(banned_host1.ip, banned_host1);
        // peer not advertised, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 18)));
        connected_peers1.advertised = false;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer Ok, return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(800.into()).unwrap());
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer Ok, connected return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)));
        connected_peers1.active_out_connections = 1;
        connected_peers1.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(900.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer failure before alive but to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(800.into()).unwrap());
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(2000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);

        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let ip_list = db.get_advertisable_peer_ips();

        assert_eq!(5, ip_list.len());

        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
            ip_list[0]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)),
            ip_list[1]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)),
            ip_list[2]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)),
            ip_list[3]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            ip_list[4]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_get_out_connection_candidate_ips() {
        let network_settings = NetworkSettings::default();
        let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();

        // add peers
        // peer Ok, return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        connected_peers1.bootstrap = true;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer failure to early. not return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(900.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer failure before alive but to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(900.into()).unwrap());
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer alive no failure. return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)));
        connected_peers1.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer banned not return.
        let mut banned_host1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = true;
        banned_host1.banned = true;
        banned_host1.last_alive = Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(banned_host1.ip, banned_host1);
        // peer failure after alive not to early. return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 15)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(12000.into()).unwrap());
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(11000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer failure after alive to early. not return
        let mut connected_peers2 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 16)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(2000.into()).unwrap());
        connected_peers2.last_failure =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers2.ip, connected_peers2);
        // peer Ok, connected, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 17)));
        connected_peers1.active_out_connections = 1;
        peers.insert(connected_peers1.ip, connected_peers1);
        // peer Ok, not advertised, not return
        let mut connected_peers1 =
            default_peer_info_not_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 18)));
        connected_peers1.advertised = false;
        peers.insert(connected_peers1.ip, connected_peers1);

        let wakeup_interval = network_settings.wakeup_interval;
        let (saver_watch_tx, _) = watch::channel(peers.clone());
        let saver_join_handle = tokio::spawn(async move {});

        let db = PeerInfoDatabase {
            network_settings,
            peers,
            saver_join_handle,
            saver_watch_tx,
            active_out_bootstrap_connection_attempts: 0,
            active_bootstrap_connections: 0,
            active_out_nonbootstrap_connection_attempts: 0,
            active_out_nonbootstrap_connections: 0,
            active_in_nonbootstrap_connections: 0,
            wakeup_interval,
            clock_compensation: 0,
        };

        // test with no peers.
        let ip_list = db.get_out_connection_candidate_ips().unwrap();
        assert_eq!(4, ip_list.len());

        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            ip_list[0]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 14)),
            ip_list[1]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 15)),
            ip_list[2]
        );
        assert_eq!(
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 13)),
            ip_list[3]
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_cleanup_peers() {
        let mut network_settings = NetworkSettings {
            max_banned_peers: 1,
            max_idle_peers: 1,
            ..Default::default()
        };
        let mut peers = HashMap::new();

        // Call with empty db.
        cleanup_peers(
            &network_settings,
            &mut peers,
            None,
            0,
            network_settings.ban_timeout,
        )
        .unwrap();
        assert!(peers.is_empty());

        let now = MassaTime::now().unwrap();

        let mut connected_peers1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)));
        connected_peers1.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        peers.insert(connected_peers1.ip, connected_peers1);

        let mut connected_peers2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12)));
        connected_peers2.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(900.into()).unwrap());
        let same_connected_peer = connected_peers2;

        let non_global =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 10)));
        let same_host =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));

        let mut banned_host1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23)));
        banned_host1.bootstrap = false;
        banned_host1.banned = true;
        banned_host1.active_out_connections = 0;
        banned_host1.last_alive = Some(now.checked_sub(1000.into()).unwrap());
        banned_host1.last_failure = Some(now.checked_sub(2000.into()).unwrap());
        let mut banned_host2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 24)));
        banned_host2.bootstrap = false;
        banned_host2.banned = true;
        banned_host2.active_out_connections = 0;
        banned_host2.last_alive = Some(now.checked_sub(900.into()).unwrap());
        banned_host2.last_failure = Some(now.checked_sub(2000.into()).unwrap());
        let mut banned_host3 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 25)));
        banned_host3.bootstrap = false;
        banned_host3.banned = true;
        banned_host3.last_alive = Some(now.checked_sub(900.into()).unwrap());
        banned_host3.last_failure = Some(now.checked_sub(2000.into()).unwrap());

        let mut advertised_host1 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 35)));
        advertised_host1.bootstrap = false;
        advertised_host1.advertised = true;
        advertised_host1.active_out_connections = 0;
        advertised_host1.last_alive =
            Some(MassaTime::now().unwrap().checked_sub(1000.into()).unwrap());
        let mut advertised_host2 =
            default_peer_info_connected(IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 36)));
        advertised_host2.bootstrap = false;
        advertised_host2.advertised = true;
        advertised_host2.active_out_connections = 0;
        advertised_host2.last_alive = Some(now.checked_sub(900.into()).unwrap());

        peers.insert(advertised_host1.ip, advertised_host1);
        peers.insert(banned_host1.ip, banned_host1);
        peers.insert(non_global.ip, non_global);
        peers.insert(same_connected_peer.ip, same_connected_peer);
        peers.insert(connected_peers2.ip, connected_peers2);
        peers.insert(connected_peers1.ip, connected_peers1);
        peers.insert(advertised_host2.ip, advertised_host2);
        peers.insert(same_host.ip, same_host);
        peers.insert(banned_host3.ip, banned_host3);
        peers.insert(banned_host2.ip, banned_host2);

        cleanup_peers(
            &network_settings,
            &mut peers,
            None,
            0,
            network_settings.ban_timeout,
        )
        .unwrap();

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 12))));

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 23))));
        assert!(!peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 24))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 25))));

        assert!(!peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 35))));
        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 36))));

        // test with advertised peers
        let advertised = vec![
            IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 0, 10)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 43)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 11)),
            IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 44)),
            IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
        ];

        network_settings.max_idle_peers = 5;

        cleanup_peers(
            &network_settings,
            &mut peers,
            Some(&advertised),
            0,
            network_settings.ban_timeout,
        )
        .unwrap();

        assert!(peers.contains_key(&IpAddr::V4(std::net::Ipv4Addr::new(169, 202, 0, 43))));
    }

    #[tokio::test]
    #[serial]
    async fn test() {
        let peer_db = PeerInfoDatabase::from(5);
        let p = peer_db.peers.values().next().unwrap();
        assert!(!p.is_active());
    }

    fn default_peer_info_connected(ip: IpAddr) -> PeerInfo {
        PeerInfo {
            ip,
            banned: false,
            bootstrap: false,
            last_alive: None,
            last_failure: None,
            advertised: false,
            active_out_connection_attempts: 0,
            active_out_connections: 1,
            active_in_connections: 0,
        }
    }

    fn default_peer_info_not_connected(ip: IpAddr) -> PeerInfo {
        PeerInfo {
            ip,
            banned: false,
            bootstrap: false,
            last_alive: None,
            last_failure: None,
            advertised: true,
            active_out_connection_attempts: 0,
            active_out_connections: 0,
            active_in_connections: 0,
        }
    }

    impl From<u32> for PeerInfoDatabase {
        fn from(peers_number: u32) -> Self {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let mut peers: HashMap<IpAddr, PeerInfo> = HashMap::new();
            for i in 0..peers_number {
                let ip: [u8; 4] = [rng.gen(), rng.gen(), rng.gen(), rng.gen()];
                let peer = PeerInfo {
                    ip: IpAddr::from(ip),
                    banned: (ip[0] % 5) == 0,
                    bootstrap: (ip[1] % 2) == 0,
                    last_alive: match i % 4 {
                        0 => None,
                        _ => Some(MassaTime::now().unwrap().checked_sub(50000.into()).unwrap()),
                    },
                    last_failure: match i % 5 {
                        0 => None,
                        _ => Some(MassaTime::now().unwrap().checked_sub(60000.into()).unwrap()),
                    },
                    advertised: (ip[2] % 2) == 0,
                    active_out_connection_attempts: 0,
                    active_out_connections: 0,
                    active_in_connections: 0,
                };
                peers.insert(peer.ip, peer);
            }
            let network_settings = NetworkSettings::default();
            let wakeup_interval = network_settings.wakeup_interval;
            let (saver_watch_tx, _) = watch::channel(peers.clone());
            let saver_join_handle = tokio::spawn(async move {});
            PeerInfoDatabase {
                network_settings,
                peers,
                saver_join_handle,
                saver_watch_tx,
                active_out_bootstrap_connection_attempts: 0,
                active_bootstrap_connections: 0,
                active_out_nonbootstrap_connection_attempts: 0,
                active_out_nonbootstrap_connections: 0,
                active_in_nonbootstrap_connections: 0,
                wakeup_interval,
                clock_compensation: 0,
            }
        }
    }
}
