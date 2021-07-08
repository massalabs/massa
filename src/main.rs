//use log::{error, warn, info, debug, trace};
mod config;
mod crypto;
mod network;

#[tokio::main]
async fn main() -> Result<(), failure::Error> {
    // parse arguments
    let args = clap::App::new("Massa client")
        .arg(
            clap::Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Config file path")
                .required(true)
                .takes_value(true),
        )
        .get_matches();

    // load config
    let config = config::Config::from_toml(&std::fs::read_to_string(args.value_of("config").unwrap())?)?;

    // setup logging
    stderrlog::new()
        .module(module_path!())
        .verbosity(match config.logging.level.as_str() {
            "ERROR" => 0,
            "WARNING" => 1,
            "INFO" => 2,
            "DEBUG" => 3,
            "TRACE" => 4,
            _ => panic!("Invalid verbosity level"),
        })
        .init()
        .unwrap();

    // run network layer
    network::run(config.network).await?;

    // exit
    Ok(())
}
