use anyhow::Result;
use clap::Parser;
use libp2p::{identity, PeerId};
use libp2p_kad_fastboot::prelude::*;
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncReadExt, task::JoinHandle};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // parse the command line arguments
    let opt = Options::parse();

    // initialize the tracing logger
    Log::init();

    // load the identity and certificate
    let local_key = read_or_create_identity(&opt.local_key_path).await?;

    // load the kademlia state if it exists
    let kademlia_state = read_kademlia_state(&opt.kad_state_file).await?;

    // create the peer, connecting it to the ui
    let mut peer = Peer::new(local_key, kademlia_state).await?;

    // spawn tasks for the peer
    let peer_task: JoinHandle<Result<()>> = tokio::spawn(async move { peer.run().await });

    // wait for the task to finish
    match peer_task.await {
        Ok(Ok(())) => {
            info!("Peer task finished successfully");
        }
        Ok(Err(e)) => {
            info!("Peer task failed: {}", e);
        }
        Err(e) => {
            info!("Peer task panicked: {}", e);
        }
    }

    Ok(())
}

async fn read_or_create_identity(path: &Path) -> Result<identity::Keypair> {
    let mut key_path = PathBuf::from(path);
    let is_key = key_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "key")
        .unwrap_or(false);
    if !is_key {
        key_path.set_extension("key");
    }

    let mut peer_id_path = PathBuf::from(path);
    let is_peer_id = peer_id_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "peerid")
        .unwrap_or(false);
    if !is_peer_id {
        peer_id_path.set_extension("peerid");
    }

    if key_path.exists() {
        let bytes = fs::read(&key_path).await?;
        info!("Using existing identity from {}", key_path.display());
        // This only works for ed25519 but that is what we are using
        return Ok(identity::Keypair::from_protobuf_encoding(&bytes)?);
    }

    let identity = identity::Keypair::generate_ed25519();
    fs::write(&key_path, &identity.to_protobuf_encoding()?).await?;
    let peer_id: PeerId = identity.public().into();
    fs::write(&peer_id_path, peer_id.to_string()).await?;

    info!(
        "Generated new identity and wrote it to {}",
        key_path.display()
    );

    Ok(identity)
}

async fn read_kademlia_state(path: &Path) -> Result<Option<KademliaState>> {
    if path.exists() {
        let mut state_str = String::new();
        fs::File::open(path)
            .await?
            .read_to_string(&mut state_str)
            .await?;
        let kademlia_state = serde_json::from_str::<KademliaState>(&state_str)?;
        Ok(Some(kademlia_state))
    } else {
        Ok(None)
    }
}
