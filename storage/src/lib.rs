mod config;
mod error;
mod storage_controller;
mod storage_worker;

/*#[cfg(test)]
mod tests {
    #[test]
    fn test_sled() {
        let tree = sled::open("./tmp/test").unwrap();
        print!("{:?}", tree);
        tree.flush().unwrap();
    }
}*/
