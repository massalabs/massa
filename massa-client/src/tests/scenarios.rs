use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_if_exit_gracefully() {
    use assert_cmd::Command;

    let mut cmd = Command::cargo_bin("massa-client").unwrap();
    cmd.arg("exit").assert().success();
}
