use anyhow::{bail, Result};
use assert_cmd::Command;
use serial_test::serial;

const TIMEOUT: u64 = 5;
const TENTATIVES: u64 = 5;

async fn run_client_cmd(cmd: &str) -> Result<String> {
    for _ in 0..TENTATIVES {
        let output = std::str::from_utf8(
            &Command::new("cargo")
                .args(&["run", "--", cmd, "--json"])
                .assert()
                .get_output()
                .stdout,
        )?
        .to_string();
        // TODO: compare with tcp connect error as a JSON object?
        if !output.contains("tcp connect error") {
            return Ok(output);
        }
        tokio::time::sleep(std::time::Duration::from_secs(TIMEOUT)).await;
    }
    bail!("tcp connect error")
}

#[tokio::test]
#[serial]
async fn run_node() {
    // TODO: @adrien-zinger do we have a better way to spawn `massa_node`?
    std::thread::spawn(|| {
        Command::new("cargo")
            .arg("run")
            .current_dir("../massa-node")
            .unwrap()
    });
}

#[tokio::test]
#[serial]
async fn client_exit_gracefully() {
    run_client_cmd("exit").await.unwrap();
}

#[tokio::test]
#[serial]
async fn node_stop_gracefully() {
    run_client_cmd("node_stop").await.unwrap();
    // TODO: @adrien-zinger do we have a better way than `.join().unwrap()` to
    // check that `massa-node` did stop?
}
