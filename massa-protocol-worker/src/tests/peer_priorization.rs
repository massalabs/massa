// In these tests, prepare some hand crafted metadata, assign them a number of priority.
// After sorting the vec of samples, the priority number has to go from increasingly
// If numbers doesn't increase for all vec, the test fails

// TODO IMPORTANT    Add unit test to validate ConnectionMetadata sorting algorithm

use massa_time::MassaTime;
use rand::thread_rng;
use rand::seq::SliceRandom;

use crate::handlers::peer_handler::models::ConnectionMetadata;

fn test_prio(mut vec: Vec<(u64, ConnectionMetadata)>) {
    vec.shuffle(&mut thread_rng());
    vec.sort_by(|a, b| a.1.cmp(&b.1));
    println!("First: {:?}, Last: {:?}",
        vec.first().unwrap().1.last_failure,
        vec.last().unwrap().1.last_failure,
    );
    let mut last_number = 0;
    for (n, (p, _)) in vec.iter().enumerate() {
        assert!(*p >= last_number, "Prio {n} failed:\n\t{:?}", vec.iter().map(|(p, _)| *p).collect::<Vec<u64>>());
        last_number = *p;
    }
}

// TODO    Ensure when "None" occurs, it's grouped together where it belongs

#[test]
fn test_last_failure_prio() {
    let test_vec = (1..500).map(|n| (n, {
        ConnectionMetadata::default().edit(0, Some(MassaTime::from_millis(1000 - n)))
    })).collect();
    test_prio(test_vec);
}

#[test]
fn test_last_success_prio() {
    let test_vec = (1..500).map(|n| (n, {
        ConnectionMetadata::default().edit(1, Some(MassaTime::from_millis(n)))
    })).collect();
    test_prio(test_vec);
}

#[test]
fn test_last_try_prio() {
    let test_vec = (1..500).map(|n| (n, {
        ConnectionMetadata::default().edit(2, Some(MassaTime::from_millis(n)))
    })).collect();
    test_prio(test_vec);
}
