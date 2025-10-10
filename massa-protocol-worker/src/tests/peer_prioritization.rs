// In these tests, prepare some hand crafted metadata, assign them a number of priority.
// After sorting the vec of samples, the priority number has to go from increasingly
// If numbers doesn't increase for all vec, the test fails

use massa_time::MassaTime;
use rand::seq::SliceRandom;
use rand::thread_rng;

use crate::handlers::peer_handler::models::ConnectionMetadata;

fn get_md(md: &ConnectionMetadata, mdidx: usize) -> Option<MassaTime> {
    match mdidx {
        0 => md.last_failure,
        1 => md.last_success,
        2 => md.last_test_failure,
        3 => md.last_test_success,
        _ => unreachable!(),
    }
}

fn test_prio(mut vec: Vec<(u64, ConnectionMetadata)>, mdidx: usize, none_first: bool) {
    vec.shuffle(&mut thread_rng());
    vec.sort_by(|a, b| a.1.cmp(&b.1));
    println!(
        "First: {:?}, Last: {:?}",
        get_md(&vec.first().unwrap().1, mdidx),
        get_md(&vec.last().unwrap().1, mdidx),
    );
    let mut trigger = false;
    for (n, (p, md)) in vec.iter().enumerate() {
        let data = get_md(md, mdidx);
        if data.is_none() {
            if none_first {
                assert!(!trigger);
            } else {
                trigger = true;
            }
        } else {
            if none_first {
                trigger = true;
            } else {
                assert!(!trigger);
            }
            assert!(
                *p >= n as u64,
                "Prio {n} failed:\n\t{:?}",
                vec.iter().map(|(p, _)| *p).collect::<Vec<u64>>()
            );
        }
    }
}

//    Failure more ancient (nb milli < ) -> More prio
//    If None, more prio than any failure
#[test]
fn test_last_failure_prio() {
    let test_vec = (1..500)
        .map(|n| {
            (n, {
                ConnectionMetadata::default().edit(
                    0,
                    if n < 50 {
                        None
                    } else {
                        Some(MassaTime::from_millis(n))
                    },
                )
            })
        })
        .collect();
    test_prio(test_vec, 0, true);
}

//    Success more recent (nb milli > ) -> More prio
//    If None, less prio than any success
#[test]
fn test_last_success_prio() {
    let test_vec = (1..500)
        .map(|n| {
            (n, {
                ConnectionMetadata::default().edit(
                    1,
                    if n > 450 {
                        None
                    } else {
                        Some(MassaTime::from_millis(1000 - n))
                    },
                )
            })
        })
        .collect();
    test_prio(test_vec, 1, false);
}

//    Test failure more recent (nb milli > ) -> Less prio
//    If None, more prio than any failure
#[test]
fn test_last_test_failure_prio() {
    let test_vec = (1..500)
        .map(|n| {
            (n, {
                ConnectionMetadata::default().edit(
                    2,
                    if n < 50 {
                        None
                    } else {
                        Some(MassaTime::from_millis(n))
                    },
                )
            })
        })
        .collect();
    test_prio(test_vec, 2, true);
}

//    Test success more recent (nb milli > ) -> More prio
//    If None, less prio than any success
#[test]
fn test_last_test_success_prio() {
    let test_vec = (1..500)
        .map(|n| {
            (n, {
                ConnectionMetadata::default().edit(
                    3,
                    if n > 450 {
                        None
                    } else {
                        Some(MassaTime::from_millis(1000 - n))
                    },
                )
            })
        })
        .collect();
    test_prio(test_vec, 3, false);
}
