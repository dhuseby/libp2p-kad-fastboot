use crate::{ipaddr_to_multiaddr, is_private_ip, split_peer_id, Options, KAD_STATE_PATH};
use clap::Parser;
use futures::StreamExt;
use libp2p::{
    connection_limits::{self, Behaviour as ConnectionLimits},
    identify::{Behaviour as Identify, Config as IdentifyConfig, Event as IdentifyEvent},
    identity,
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig,
        Event as KademliaEvent, NodeStatus, QueryId, QueryResult, RoutingUpdate,
    },
    memory_connection_limits::Behaviour as MemoryConnectionLimits,
    multiaddr::Protocol,
    swarm::{NetworkBehaviour, Swarm, SwarmEvent},
    Multiaddr, PeerId, StreamProtocol, SwarmBuilder,
};
use signal_hook::consts::SIGTERM;
use signal_hook_tokio::Signals;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use tokio::fs;
use tracing::{debug, info, warn};

// Universal connectivity agent string
const KAD_FASTBOOT_AGENT: &str = "kad-fastboot/0.0.1";

// Protocol Names
const IPFS_KADEMLIA_PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");
const IPFS_IDENTIFY_PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/id/1.0.0");

// Listen Ports
const PORT_QUIC: u16 = 9091; // UDP

// Kademlia bootstrap interval
const KADEMLIA_BOOTSTRAP_INTERVAL: u64 = 300;
const IPFS_BOOTSTRAP_NODES: [&str; 4] = [
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "/dnsaddr/bootstrap.libp2p.io/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];

/// The Kademlia State
pub type KademliaState = Vec<(PeerId, Vec<Multiaddr>)>;

/// The Peer Behaviour
#[derive(NetworkBehaviour)]
struct Behaviour {
    connection_limits: ConnectionLimits,
    identify: Identify,
    kademlia: Kademlia<MemoryStore>,
    memory_connection_limits: MemoryConnectionLimits,
}

// The rust-peer implementation is full featured and supports a number of protocols and transports
// to make it maximally compatible will all other universal connectivity peers
//
// This swarm supports:
// - QUIC + Noise on UDP port 9091

/// The Peer state
pub struct Peer {
    /// The local peer id
    local_peer_id: PeerId,
    /// The addresses we're listening on
    listen_addresses: HashSet<Multiaddr>,
    /// The external addresses that others see, given on command line
    external_addresses: HashSet<Multiaddr>,
    /// The swarm itself
    swarm: Swarm<Behaviour>,
    /// The kademlia state, if given one
    kademlia_state: Option<KademliaState>,
    /// The query id for the kademlia bootstrap
    bootstrap_query_id: Option<QueryId>,
    /// The start time of the test
    start_time: Option<Instant>,
}

impl Peer {
    /// Create a new Peer instance by initializing the swarm and peer state
    pub async fn new(
        keypair: identity::Keypair,
        kademlia_state: Option<KademliaState>,
    ) -> anyhow::Result<Self> {
        // parse the command line arguments
        let opt = Options::parse();

        // get the local peer id
        let local_peer_id = PeerId::from(keypair.public());
        info!(" Local peer id: {local_peer_id}");

        // get the listen addresses with the QUIC port
        let mut listen_addresses = HashSet::new();
        for addr in opt.listen_addresses.iter() {
            // add the QUIC address
            listen_addresses.insert(
                ipaddr_to_multiaddr(addr)
                    .with(Protocol::Udp(PORT_QUIC))
                    .with(Protocol::QuicV1),
            );
        }

        // get any external addresses if given
        let mut external_addresses = HashSet::new();
        for addr in opt.external_addresses.iter() {
            external_addresses.insert(ipaddr_to_multiaddr(addr));
        }

        // initialize the swarm
        let swarm = {
            // Create the ConnectionLimits behaviour
            let connection_limits = {
                let cfg = connection_limits::ConnectionLimits::default()
                    .with_max_pending_incoming(Some(100))
                    .with_max_pending_outgoing(Some(100))
                    .with_max_established_per_peer(Some(10))
                    .with_max_established(Some(1000));
                ConnectionLimits::new(cfg)
            };

            // Create an Identify behaviour
            let identify = {
                let cfg = IdentifyConfig::new(
                    IPFS_IDENTIFY_PROTOCOL_NAME.to_string(), // bug: https://github.com/libp2p/rust-libp2p/issues/5940
                    keypair.public(),
                )
                .with_agent_version(KAD_FASTBOOT_AGENT.to_string());
                Identify::new(cfg)
            };

            // Create a Kademlia behaviour
            let kademlia: Kademlia<MemoryStore> = {
                let mut cfg = KademliaConfig::new(IPFS_KADEMLIA_PROTOCOL_NAME);
                cfg.set_query_timeout(Duration::from_secs(60));
                cfg.set_periodic_bootstrap_interval(Some(Duration::from_secs(
                    KADEMLIA_BOOTSTRAP_INTERVAL,
                )));
                let store = MemoryStore::new(local_peer_id);
                Kademlia::with_config(local_peer_id, store, cfg)
            };

            // Create the MemoryConnectionLimits behaviour
            let memory_connection_limits = MemoryConnectionLimits::with_max_percentage(0.9);

            // Initialize the overall peer behaviour
            let behaviour = Behaviour {
                connection_limits,
                identify,
                kademlia,
                memory_connection_limits,
            };

            // Build the swarm
            SwarmBuilder::with_existing_identity(keypair.clone())
                .with_tokio()
                .with_quic()
                .with_dns()?
                .with_behaviour(|_key| behaviour)?
                .build()
        };

        Ok(Self {
            local_peer_id,
            listen_addresses,
            external_addresses,
            swarm,
            kademlia_state,
            bootstrap_query_id: None,
            start_time: None,
        })
    }

    /// Saves to disk the kademlia state
    pub async fn save_kademlia_state(&mut self) -> anyhow::Result<()> {
        let kad = &mut self.swarm.behaviour_mut().kademlia;
        let kademlia_state: Vec<(PeerId, Vec<Multiaddr>)> = kad
            .kbuckets()
            .flat_map(|bucket| {
                bucket
                    .iter()
                    .filter(|erv| matches!(erv.status, NodeStatus::Connected))
                    .map(|erv| {
                        (
                            (*erv.node.key).into_preimage(),
                            erv.node.value.clone().into_vec(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let state_str = serde_json::to_string_pretty(&kademlia_state)?;
        fs::write(KAD_STATE_PATH, state_str).await?;
        info!(" Kademlia state saved to {KAD_STATE_PATH}");
        Ok(())
    }

    /// Outputs the test run line
    pub async fn output_metric(&mut self) -> anyhow::Result<()> {
        info!(" Kademlia bootstrapped");
        let status = match self.kademlia_state {
            Some(_) => "fastboot".to_string(),
            None => "bootstrap".to_string(),
        };
        let timing = match self.start_time {
            Some(start) => {
                let elapsed = start.elapsed();
                format!(
                    "{:02}:{:02}:{:02}",
                    elapsed.as_secs() / 3600,
                    elapsed.as_secs() / 60,
                    elapsed.as_secs() % 60,
                )
            }
            None => "00:00:00".to_string(),
        };
        info!("] {status},{},{timing}", self.local_peer_id);
        Ok(())
    }

    /// Update our external address if needed
    pub async fn update_external_address(&mut self, address: &Multiaddr) -> anyhow::Result<bool> {
        if !is_private_ip(address) && self.external_addresses.insert(address.clone()) {
            info!(" Adding external address: {address}");
            self.swarm.add_external_address(address.clone());
            return Ok(true);
        }
        Ok(false)
    }

    /// Run the Peer
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // register the shutdown signal
        let mut signals = Signals::new([SIGTERM])?;

        // Listen on the given addresses
        let addrs: Vec<Multiaddr> = self.listen_addresses.iter().cloned().collect();
        for addr in addrs.iter() {
            if let Err(e) = self.swarm.listen_on(addr.clone()) {
                warn!(" Failed to listen on {addr}: {e}");
            }
        }

        // Set the external address if passed in
        let addrs: Vec<Multiaddr> = self.external_addresses.drain().collect();
        for addr in addrs.iter() {
            self.update_external_address(addr).await?;
        }

        // initialize the kademlia state
        {
            let kad = &mut self.swarm.behaviour_mut().kademlia;
            let mut initialized = false;

            if let Some(kademlia_state) = self.kademlia_state.take() {
                // load the kademlia state from disk
                info!(" Loading kademlia state from disk");
                kademlia_state.iter().for_each(|(peerid, addrs)| {
                    addrs.iter().for_each(|multiaddr| {
                        match kad.add_address(peerid, multiaddr.clone()) {
                            RoutingUpdate::Success => {
                                info!(" {peerid} {multiaddr}");
                                initialized = true;
                            }
                            RoutingUpdate::Pending => {
                                info!(" Pending {peerid} {multiaddr}");
                            }
                            RoutingUpdate::Failed => {
                                warn!(" Failed {peerid} {multiaddr}");
                            }
                        }
                    })
                });
            }

            if !initialized {
                // parse the bootstrap multiaddrs
                let bootstrappers: Vec<Multiaddr> = IPFS_BOOTSTRAP_NODES
                    .iter()
                    .filter_map(|s| s.parse().ok())
                    .collect();

                // add the bootstrap nodes to the kademlia
                info!(" Initializing kademlia state from bootstrapper list");
                for addr in bootstrappers.iter() {
                    if let Some((multiaddr, peerid)) = split_peer_id(addr.clone()) {
                        match kad.add_address(&peerid, multiaddr.clone()) {
                            RoutingUpdate::Success => {
                                info!(" {peerid} {multiaddr}");
                            }
                            RoutingUpdate::Pending => {
                                info!(" Pending {peerid} {multiaddr}");
                            }
                            RoutingUpdate::Failed => {
                                warn!(" Failed {peerid} {multiaddr}");
                            }
                        }
                    }
                }
            }

            // start the bootstrap process
            match kad.bootstrap() {
                Ok(query_id) => {
                    self.start_time = Some(Instant::now());
                    self.bootstrap_query_id = Some(query_id);
                    info!(" Bootstrapping Kademlia");
                }
                Err(e) => {
                    warn!(" Failed to bootstrap Kademlia: {e}");
                    info!(
                        "Don't worry, it will try again in {KADEMLIA_BOOTSTRAP_INTERVAL} seconds"
                    );
                }
            }
        }

        // Create our loop ticker
        let mut tick = tokio::time::interval(Duration::from_secs(60));

        // Run the main loop
        'runloop: loop {
            tokio::select! {

                Some(SIGTERM) = signals.next() => {
                    info!(" Shutting down the peer");
                    break;
                }

                _ = tick.tick() => {
                    self.save_kademlia_state().await?;
                }

                Some(event) = self.swarm.next() => match event {

                    // When the swarm in initiates a dial
                    SwarmEvent::Dialing { peer_id, .. } => {
                        let peer_id = peer_id.map_or("Unknown".to_string(), |peer_id| peer_id.to_string());
                        debug!("Dialing {peer_id}");
                    }

                    // When we successfully connect to a peer
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        debug!("Connected to {peer_id}");
                    }

                    // When we fail to connect to a peer
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        warn!("Failed to dial {peer_id:?}: {error}");
                    }

                    // When we fail to accept a connection from a peer
                    SwarmEvent::IncomingConnectionError { error, .. } => {
                        warn!("{:#}", anyhow::Error::from(error))
                    }

                    // When a connection to a peer is closed
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        warn!("Connection to {peer_id} closed: {cause:?}");
                        self.swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
                        info!("Removed {peer_id} from the routing table (if it was in there).");
                    }

                    // When we receive an identify event
                    SwarmEvent::Behaviour(BehaviourEvent::Identify(event)) => match event {
                        IdentifyEvent::Received { info, .. } => {
                            //self.update_external_address(&info.observed_addr).await?;
                            if info.agent_version == KAD_FASTBOOT_AGENT {
                                let peer_id: PeerId = info.public_key.into();
                                let agent = format!("{} version: {}", info.agent_version, info.protocol_version);
                                let protocols = info.protocols.iter().map(|p| format!("\n\t\t{p}") ).collect::<Vec<String>>().join("");
                                info!("Identify {peer_id}:\n\tagent: {agent}\n\tprotocols: {protocols}");
                                for addr in info.listen_addrs.iter() {
                                    if !is_private_ip(addr) {
                                        if let Err(e) = self.swarm.dial(addr.clone()) {
                                            warn!("Failed to dial {addr}: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        IdentifyEvent::Sent { .. } => {
                            debug!("identify::Event::Sent");
                        }
                        IdentifyEvent::Pushed { .. } => {
                            debug!("identify::Event::Pushed");
                        }
                        IdentifyEvent::Error { error, .. } => {
                            warn!("{error}");
                        }
                    }

                    // When we receive a kademlia event
                    SwarmEvent::Behaviour(BehaviourEvent::Kademlia(event)) => match event {
                        KademliaEvent::OutboundQueryProgressed { id, result: QueryResult::Bootstrap(result), step, .. } => {
                            if let Some(query_id) = self.bootstrap_query_id {
                                if id == query_id {
                                    match result {
                                        Ok(bootstrap) => {
                                            if step.last {
                                                self.bootstrap_query_id = None;
                                                self.save_kademlia_state().await?;
                                                self.output_metric().await?;
                                                break 'runloop;
                                            } else {
                                                info!(" Peer found {}, remaining: {}", bootstrap.peer, bootstrap.num_remaining);
                                            }
                                        }
                                        Err(e) => {
                                            warn!(" Failed to bootstrap Kademlia: {e}");
                                            self.bootstrap_query_id = None;
                                        }
                                    }
                                }
                            }
                        }

                        KademliaEvent::RoutingUpdated { peer, is_new_peer, addresses, .. } => {
                            info!(" Added to routing table: {peer} {is_new_peer} {addresses:?}");
                        }

                        ref _other => {}
                    }

                    ref _other => {}
                }
            }
        }

        Ok(())
    }
}
