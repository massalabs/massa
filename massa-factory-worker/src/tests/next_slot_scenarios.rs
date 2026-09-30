use crate::block_factory::compute_next_block_slot;
use crate::endorsement_factory::compute_next_endorsement_slot;
use massa_factory_exports::FactoryConfig;
use massa_models::{slot::Slot, timeslots::get_block_slot_timestamp};
use massa_time::MassaTime;

fn test_config() -> FactoryConfig {
    FactoryConfig {
        thread_count: 32,
        genesis_timestamp: MassaTime::from_millis(1_000_000),
        t0: MassaTime::from_millis(16_000),
        initial_delay: MassaTime::from_millis(100),
        last_start_period: 0,
        ..FactoryConfig::default()
    }
}

fn slot_timestamp(cfg: &FactoryConfig, slot: Slot) -> MassaTime {
    get_block_slot_timestamp(cfg.thread_count, cfg.t0, cfg.genesis_timestamp, slot).unwrap()
}

fn endorsement_instant(cfg: &FactoryConfig, slot: Slot) -> MassaTime {
    slot_timestamp(cfg, slot)
        .checked_sub(cfg.t0.checked_div_u64(2).unwrap())
        .unwrap()
}

fn previous_slot(cfg: &FactoryConfig, slot: Slot) -> Slot {
    slot.get_prev_slot(cfg.thread_count).unwrap()
}

/// Restart timeline of the issue: the previous instance endorsed (6, 15) at its endorsement
/// instant (slot time - t0/2), and the new instance starts between that instant and the slot time.
/// The new instance must not endorse (6, 15), nor any other slot whose endorsement instant is past.
#[test]
fn first_endorsement_slot_skips_past_endorsement_instants() {
    let cfg = test_config();
    let already_endorsed = Slot::new(6, 15);
    let now = slot_timestamp(&cfg, already_endorsed)
        .checked_sub(MassaTime::from_millis(235))
        .unwrap();
    assert!(now > endorsement_instant(&cfg, already_endorsed));

    let slot = compute_next_endorsement_slot(&cfg, None, now);
    assert!(slot > already_endorsed);
    assert_eq!(slot, Slot::new(6, 31));
    assert!(endorsement_instant(&cfg, slot) >= now.saturating_add(cfg.initial_delay));
}

/// Whatever the start time within a slot, the first endorsement slot is the first one whose
/// endorsement instant is at least `initial_delay` ahead.
#[test]
fn first_endorsement_slot_is_first_one_ahead_of_initial_delay() {
    let cfg = test_config();
    let start = slot_timestamp(&cfg, Slot::new(3, 0)).as_millis();
    let end = slot_timestamp(&cfg, Slot::new(5, 0)).as_millis();
    for now in (start..end).step_by(7).map(MassaTime::from_millis) {
        let earliest = now.saturating_add(cfg.initial_delay);
        let slot = compute_next_endorsement_slot(&cfg, None, now);
        assert!(
            endorsement_instant(&cfg, slot) >= earliest,
            "now {}: slot {} endorsed too early",
            now,
            slot
        );
        assert!(
            endorsement_instant(&cfg, previous_slot(&cfg, slot)) < earliest,
            "now {}: slot {} skips more than needed",
            now,
            slot
        );
    }
}

/// Slots are not skipped once the factory is running.
#[test]
fn next_endorsement_slot_follows_previous_slot() {
    let cfg = test_config();
    let prev = Slot::new(6, 15);
    let next = Slot::new(6, 16);
    // woken up at the endorsement instant of the previous slot
    let now = endorsement_instant(&cfg, prev);
    assert_eq!(compute_next_endorsement_slot(&cfg, Some(prev), now), next);
}

/// After a network restart, endorsement production starts at the first slot after the last start period.
#[test]
fn first_endorsement_slot_ignores_genesis() {
    let cfg = FactoryConfig {
        last_start_period: 5,
        ..test_config()
    };
    let now = slot_timestamp(&cfg, Slot::new(5, 0));
    assert_eq!(
        compute_next_endorsement_slot(&cfg, None, now),
        Slot::new(6, 0)
    );
}

/// A restart right after a slot time must not produce that block again.
#[test]
fn first_block_slot_skips_past_slots() {
    let cfg = test_config();
    let already_produced = Slot::new(6, 15);
    let now = slot_timestamp(&cfg, already_produced).saturating_add(MassaTime::from_millis(100));

    let slot = compute_next_block_slot(&cfg, None, now);
    assert_eq!(slot, Slot::new(6, 16));
}

/// Whatever the start time within a slot, the first block slot is the first one at least
/// `initial_delay` ahead.
#[test]
fn first_block_slot_is_first_one_ahead_of_initial_delay() {
    let cfg = test_config();
    let start = slot_timestamp(&cfg, Slot::new(3, 0)).as_millis();
    let end = slot_timestamp(&cfg, Slot::new(3, 4)).as_millis();
    for now in (start..end).map(MassaTime::from_millis) {
        let earliest = now.saturating_add(cfg.initial_delay);
        let slot = compute_next_block_slot(&cfg, None, now);
        assert!(
            slot_timestamp(&cfg, slot) >= earliest,
            "now {}: slot {} produced too early",
            now,
            slot
        );
        assert!(
            slot_timestamp(&cfg, previous_slot(&cfg, slot)) < earliest,
            "now {}: slot {} skips more than needed",
            now,
            slot
        );
    }
}

/// Slots are not skipped once the factory is running.
#[test]
fn next_block_slot_follows_previous_slot() {
    let cfg = test_config();
    let prev = Slot::new(6, 15);
    let next = Slot::new(6, 16);
    // woken up at the previous slot time
    let now = slot_timestamp(&cfg, prev);
    assert_eq!(compute_next_block_slot(&cfg, Some(prev), now), next);
}
