use massa_db_exports::DEFERRED_CALLS_SLOT_PREFIX;

pub(crate) const DEFERRED_CALL_TOTAL_GAS: &str = "deferred_call_total_gas";

pub(crate) const CALLS_TAG: u8 = 0u8;
// slot fields
pub(crate) const SLOT_TOTAL_GAS: u8 = 1u8;
pub(crate) const SLOT_BASE_FEE: u8 = 2u8;

// call fields
pub(crate) const CALL_FIELD_SENDER_ADDRESS: u8 = 1u8;
pub(crate) const CALL_FIELD_TARGET_SLOT: u8 = 2u8;
pub(crate) const CALL_FIELD_TARGET_ADDRESS: u8 = 3u8;
pub(crate) const CALL_FIELD_TARGET_FUNCTION: u8 = 4u8;
pub(crate) const CALL_FIELD_PARAMETERS: u8 = 5u8;
pub(crate) const CALL_FIELD_COINS: u8 = 6u8;
pub(crate) const CALL_FIELD_MAX_GAS: u8 = 7u8;
pub(crate) const CALL_FIELD_FEE: u8 = 8u8;
pub(crate) const CALL_FIELD_CANCELED: u8 = 9u8;

#[macro_export]
macro_rules! deferred_call_slot_total_gas_key {
    ($slot:expr) => {
        [
            DEFERRED_CALLS_SLOT_PREFIX.as_bytes(),
            &$slot[..],
            &[$crate::macros::SLOT_TOTAL_GAS],
        ]
        .concat()
    };
}

#[macro_export]
macro_rules! deferred_call_slot_base_fee_key {
    ($slot:expr) => {
        [
            DEFERRED_CALLS_SLOT_PREFIX.as_bytes(),
            &$slot[..],
            &[$crate::macros::SLOT_BASE_FEE],
        ]
        .concat()
    };
}

#[macro_export]
macro_rules! deferred_slot_call_prefix_key {
    ($slot:expr) => {
        [
            DEFERRED_CALLS_SLOT_PREFIX.as_bytes(),
            &$slot[..],
            &[$crate::macros::CALLS_TAG],
        ]
        .concat()
    };
}

#[macro_export]
macro_rules! deferred_call_prefix_key {
    ($id:expr, $slot:expr) => {
        [&deferred_slot_call_prefix_key!($slot), &$id[..]].concat()
    };
}

/// key formatting macro
#[macro_export]
macro_rules! deferred_call_field_key {
    ($id:expr, $slot:expr, $field:expr) => {
        [deferred_call_prefix_key!($id, $slot), vec![$field]].concat()
    };
}

/// sender address key formatting macro
#[macro_export]
macro_rules! sender_address_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_SENDER_ADDRESS)
    };
}

/// target slot key formatting macro
#[macro_export]
macro_rules! target_slot_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_TARGET_SLOT)
    };
}

/// target address key formatting macro
#[macro_export]
macro_rules! target_address_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_TARGET_ADDRESS)
    };
}

/// target function key formatting macro
#[macro_export]
macro_rules! target_function_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_TARGET_FUNCTION)
    };
}

/// parameters key formatting macro
#[macro_export]
macro_rules! parameters_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_PARAMETERS)
    };
}

/// coins key formatting macro
#[macro_export]
macro_rules! coins_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_COINS)
    };
}

/// max gas key formatting macro
#[macro_export]
macro_rules! max_gas_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_MAX_GAS)
    };
}

/// fee key formatting macro
#[macro_export]
macro_rules! fee_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_FEE)
    };
}

/// cancelled key formatting macro
#[macro_export]
macro_rules! cancelled_key {
    ($id:expr, $slot:expr) => {
        deferred_call_field_key!($id, $slot, $crate::macros::CALL_FIELD_CANCELED)
    };
}

#[cfg(test)]
mod tests {
    use massa_db_exports::DEFERRED_CALLS_SLOT_PREFIX;
    use massa_models::{
        deferred_call_id::{DeferredCallId, DeferredCallIdSerializer},
        slot::Slot,
    };
    use massa_serialization::Serializer;

    #[test]
    fn test_deferred_call_prefix_key() {
        let slot_ser = Slot {
            period: 1,
            thread: 5,
        }
        .to_bytes_key();

        let id = DeferredCallId::new(
            0,
            Slot {
                thread: 5,
                period: 1,
            },
            1,
            &[],
        )
        .unwrap();

        let id_ser = DeferredCallIdSerializer::new();
        let mut buf_id = Vec::new();
        id_ser.serialize(&id, &mut buf_id).unwrap();

        let prefix = ["deferred_calls/".as_bytes(), &slot_ser, &[0u8], &buf_id].concat();

        assert_eq!(deferred_call_prefix_key!(buf_id, slot_ser), prefix);

        // let to_check = [prefix[..], &[1u8]].concat();
        // assert_eq!(sender_address_key!(buf_id, slot_ser), to_check);
    }
}
