//! libp2p-kad-fastboot crate
#![warn(missing_docs)]
#![deny(
    trivial_casts,
    trivial_numeric_casts,
    unused_import_braces,
    unused_qualifications
)]

/// The logging module
pub mod log;
pub use log::Log;

/// The command line options module
pub mod options;
pub use options::{Options, KAD_STATE_PATH};

/// The peer module
pub mod peer;
pub use peer::{KademliaState, Peer};

/// The utility functions
pub mod util;
pub use util::{extract_ip_multiaddr, ipaddr_to_multiaddr, is_private_ip, split_peer_id};

/// Prelude module
pub mod prelude {
    pub use super::*;
}
