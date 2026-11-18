use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::RwLockCondvar;

/// `notify_all` must wait for the mutex, which a waiter holds from the moment it
/// releases its read guard until it parks on the condvar. Otherwise a
/// notification sent in that window is lost.
#[test]
fn test_notify_all_waits_for_waiter_critical_section() {
    let cv = Arc::new(RwLockCondvar::default());

    // stand in for a waiter between releasing its read guard and parking
    let waiter_guard = cv.mutex.lock();

    let (tx, rx) = mpsc::channel();
    let notifier = {
        let cv = cv.clone();
        std::thread::spawn(move || {
            cv.notify_all();
            tx.send(()).unwrap();
        })
    };

    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "notify_all returned while a waiter was still in its critical section"
    );

    drop(waiter_guard);
    rx.recv_timeout(Duration::from_secs(5))
        .expect("notify_all did not complete once the mutex was released");
    notifier.join().unwrap();
}
