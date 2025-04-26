use clap::Parser;
use std::{net::IpAddr, path::PathBuf};

const LISTEN_ADDR: [&str; 1] = ["0.0.0.0"];
const LOCAL_KEY_PATH: &str = "./local";
/// The default kad state file path
pub const KAD_STATE_PATH: &str = "./kademlia_state.json";

/// The rust peer command line options
#[derive(Debug, Parser)]
#[clap(name = "libp2p kademlia fastboot")]
pub struct Options {
    /// Address to listen on.
    #[clap(long, env, action = clap::ArgAction::Append, value_delimiter = ',', default_values = LISTEN_ADDR)]
    pub listen_addresses: Vec<IpAddr>,

    /// If known, the external address of this node. Will be used to correctly advertise our external address across all transports.
    #[clap(long, env, action = clap::ArgAction::Append, value_delimiter = ',')]
    pub external_addresses: Vec<IpAddr>,

    /// If set, the path to the local key file.
    #[clap(long, env, default_value = LOCAL_KEY_PATH)]
    pub local_key_path: PathBuf,

    /// If set, the path to the kademlia_state file
    #[clap(long, env, default_value = KAD_STATE_PATH)]
    pub kad_state_file: PathBuf,
}
