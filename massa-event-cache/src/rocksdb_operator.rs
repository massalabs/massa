use rocksdb::MergeOperands;

pub fn counter_merge(
    _key: &[u8],
    existing_val: Option<&[u8]>,
    operands: &MergeOperands,
) -> Option<Vec<u8>> {
    let counter_current_value = if let Some(existing_val) = existing_val {
        u64::from_be_bytes(existing_val.try_into().unwrap())
    } else {
        0
    };

    let counter_value = operands.iter().fold(counter_current_value, |mut acc, x| {
        let incr_value = i64::from_be_bytes(x.try_into().unwrap());
        acc = acc.saturating_add_signed(incr_value);
        acc
    });

    Some(counter_value.to_be_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    // std
    // third-party
    use rocksdb::{Options, DB};
    use serial_test::serial;
    use tempfile::TempDir;

    #[test]
    #[serial]
    fn test_operator() {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        let options = {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.set_merge_operator_associative("counter merge operator", counter_merge);
            opts
        };
        let db = DB::open(&options, tmp_path).unwrap();
        let key_1 = "foo1";
        let key_2 = "baz42";
        db.put(key_1, 0u64.to_be_bytes()).unwrap();
        db.put(key_2, 0u64.to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 0);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 0);

        // key_1 counter += 1
        db.merge(key_1, 1i64.to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 1);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 0);

        // key_2 counter += 9
        db.merge(key_2, 9i64.to_be_bytes()).unwrap();
        // key_2 counter += 1
        db.merge(key_2, 1i64.to_be_bytes()).unwrap();
        // key_2 counter += 32
        db.merge(key_2, 32i64.to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 1);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 42);
    }

    #[test]
    #[serial]
    fn test_operator_2() {
        let tmp_path = TempDir::new().unwrap().path().to_path_buf();
        let options = {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.set_merge_operator_associative("counter merge operator", counter_merge);
            opts
        };
        let db = DB::open(&options, tmp_path).unwrap();
        let key_1 = "foo1";
        let key_2 = "baz42";
        db.put(key_1, 0u64.to_be_bytes()).unwrap();
        db.put(key_2, 0u64.to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 0);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 0);

        // key_1 counter += 1
        db.merge(key_1, 1i64.to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 1);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 0);

        db.merge(key_1, (-3i64).to_be_bytes()).unwrap();

        let value = db.get(key_1).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value.try_into().unwrap()), 0);
        let value2 = db.get(key_2).unwrap().unwrap();
        assert_eq!(u64::from_be_bytes(value2.try_into().unwrap()), 0);
    }
}
