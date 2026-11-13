use massa_serialization::{
    DeserializeError, Deserializer, U128VarIntDeserializer, U16VarIntDeserializer,
    U32VarIntDeserializer, U64VarIntDeserializer,
};
use std::ops::Bound::{Excluded, Included, Unbounded};

macro_rules! cases {
    ($module:ident, $type:ident, $decoder:ident) => {
        mod $module {
            use super::*;

            const GROUPS: usize = ($type::BITS as usize).div_ceil(7);
            const LAST_BITS: u32 = $type::BITS - (GROUPS as u32 - 1) * 7;
            const LAST_MAX: u8 = (1 << LAST_BITS) - 1;

            fn maximum() -> Vec<u8> {
                let mut bytes = vec![0xff; GROUPS - 1];
                bytes.push(LAST_MAX);
                bytes
            }

            fn first_overflow() -> Vec<u8> {
                let mut bytes = vec![0x80; GROUPS - 1];
                bytes.push(1 << LAST_BITS);
                bytes
            }

            fn decoder() -> $decoder {
                $decoder::new(Unbounded, Unbounded)
            }

            #[test]
            fn varint_maximum_valid() {
                let mut bytes = maximum();
                assert_eq!(
                    decoder().deserialize::<DeserializeError>(&bytes).unwrap(),
                    (&[][..], $type::MAX)
                );
                bytes.extend([0x42, 0x80]);
                assert_eq!(
                    decoder().deserialize::<DeserializeError>(&bytes).unwrap(),
                    (&bytes[GROUPS..], $type::MAX)
                );
            }

            #[test]
            fn varint_first_overflow_rejected() {
                let bytes = first_overflow();
                assert!(decoder().deserialize::<DeserializeError>(&bytes).is_err());
            }

            #[test]
            fn varint_terminal_payload_range() {
                for terminal in 1..=0x7f {
                    let mut bytes = vec![0xff; GROUPS - 1];
                    bytes.push(terminal);
                    bytes.push(0x42);
                    let result = decoder().deserialize::<DeserializeError>(&bytes);
                    if terminal <= LAST_MAX {
                        let expected = ($type::from(terminal) << (7 * (GROUPS - 1)))
                            | ($type::MAX >> LAST_BITS);
                        assert_eq!(result.unwrap(), (&bytes[GROUPS..], expected));
                    } else {
                        assert!(
                            result.is_err(),
                            "terminal {terminal:#x} accepted: {result:?}"
                        );
                    }
                }
            }

            #[test]
            fn varint_truncated_rejected() {
                let bytes = maximum();
                for end in 0..GROUPS {
                    assert!(decoder()
                        .deserialize::<DeserializeError>(&bytes[..end])
                        .is_err());
                }
                let mut continuing = bytes;
                continuing[GROUPS - 1] |= 0x80;
                assert!(decoder()
                    .deserialize::<DeserializeError>(&continuing)
                    .is_err());
                continuing.push(1);
                assert!(decoder()
                    .deserialize::<DeserializeError>(&continuing)
                    .is_err());
            }

            #[test]
            fn varint_nonminimal_rejected() {
                for bytes in [vec![0x80, 0], vec![0x81, 0], vec![0x80, 0x80, 0]] {
                    assert!(decoder().deserialize::<DeserializeError>(&bytes).is_err());
                }
                let mut bytes = maximum();
                bytes[GROUPS - 1] |= 0x80;
                bytes.push(0);
                assert!(decoder().deserialize::<DeserializeError>(&bytes).is_err());
            }

            #[test]
            fn varint_configured_bounds() {
                for (min, max, accepted) in [
                    (Included(3), Included(7), vec![3, 4, 5, 6, 7]),
                    (Excluded(3), Excluded(7), vec![4, 5, 6]),
                    (Included(3), Excluded(7), vec![3, 4, 5, 6]),
                    (Excluded(3), Included(7), vec![4, 5, 6, 7]),
                    (Unbounded, Included(2), vec![0, 1, 2]),
                    (Included(7), Unbounded, vec![7, 8, 9]),
                    (Unbounded, Unbounded, (0..=9).collect()),
                    (Included(3), Included(3), vec![3]),
                    (Included(3), Excluded(3), vec![]),
                    (Included(7), Included(3), vec![]),
                    (Excluded(0), Included(0), vec![]),
                ] {
                    for value in 0..=9u8 {
                        let bytes = [value, 0x42];
                        let result =
                            $decoder::new(min, max).deserialize::<DeserializeError>(&bytes);
                        assert_eq!(result.is_ok(), accepted.contains(&value));
                        if let Ok((rest, decoded)) = result {
                            assert_eq!((rest, decoded), (&bytes[1..], $type::from(value)));
                        }
                    }
                }
                let bytes = maximum();
                assert!($decoder::new(Included($type::MAX), Included($type::MAX))
                    .deserialize::<DeserializeError>(&bytes)
                    .is_ok());
                for bounds in [
                    (Unbounded, Excluded($type::MAX)),
                    (Excluded($type::MAX), Unbounded),
                ] {
                    assert!($decoder::new(bounds.0, bounds.1)
                        .deserialize::<DeserializeError>(&bytes)
                        .is_err());
                }
            }

            #[test]
            fn varint_overflow_configured_bounds_rejected() {
                let bytes = first_overflow();
                assert!($decoder::new(Included(0), Included(0))
                    .deserialize::<DeserializeError>(&bytes)
                    .is_err());
                let mut aliases_three = bytes;
                aliases_three[0] |= 3;
                assert!($decoder::new(Included(3), Included(7))
                    .deserialize::<DeserializeError>(&aliases_three)
                    .is_err());
            }
        }
    };
}

cases!(u16, u16, U16VarIntDeserializer);
cases!(u32, u32, U32VarIntDeserializer);
cases!(u64, u64, U64VarIntDeserializer);
cases!(u128, u128, U128VarIntDeserializer);
