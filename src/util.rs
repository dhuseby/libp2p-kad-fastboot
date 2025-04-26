use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use std::net::IpAddr;

/// Split the PeerId from a Multiaddr
pub fn split_peer_id(multiaddr: Multiaddr) -> Option<(Multiaddr, PeerId)> {
    let mut base_addr = Multiaddr::empty();
    let mut peer_id = None;

    // Iterate over the protocols in the Multiaddr
    for protocol in multiaddr.into_iter() {
        if let Protocol::P2p(id) = protocol {
            peer_id = Some(id);
            break; // Stop once we find the P2p component
        } else {
            base_addr.push(protocol); // Add non-P2p components to the base address
        }
    }

    peer_id.map(|id| (base_addr, id))
}

/// Extract the IP address from a Multiaddr
pub fn extract_ip_multiaddr(multiaddr: &Multiaddr) -> Option<Multiaddr> {
    let mut result = Multiaddr::empty();

    for component in multiaddr.into_iter() {
        match component {
            Protocol::Ip4(addr) => {
                result.push(Protocol::Ip4(addr));
                return Some(result);
            }
            Protocol::Ip6(addr) => {
                result.push(Protocol::Ip6(addr));
                return Some(result);
            }
            _ => continue,
        }
    }

    None
}

/// Check if a Multiaddr contains a private IP address
pub fn is_private_ip(multiaddr: &Multiaddr) -> bool {
    for component in multiaddr.into_iter() {
        match component {
            Protocol::Ip4(addr) => {
                return addr.is_private() ||    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                       addr.is_loopback() ||   // 127.0.0.0/8
                       addr.is_link_local() || // 169.254.0.0/16
                       addr.is_unspecified(); // 0.0.0.0
            }
            Protocol::Ip6(addr) => {
                return addr.is_loopback() ||    // ::1
                       addr.is_unspecified() || // ::
                       // Unique Local Address (fc00::/7 where 8th bit is 1)
                       (addr.segments()[0] & 0xfe00 == 0xfc00) ||
                       // Link-Local unicast (fe80::/10)
                       (addr.segments()[0] & 0xffc0 == 0xfe80);
            }
            _ => continue,
        }
    }
    false
}

/// Convert an IP address to a Multiaddr
pub fn ipaddr_to_multiaddr(ip: &IpAddr) -> Multiaddr {
    let multiaddr = match ip {
        IpAddr::V4(ipv4) => Multiaddr::empty().with(Protocol::Ip4(*ipv4)),
        IpAddr::V6(ipv6) => Multiaddr::empty().with(Protocol::Ip6(*ipv6)),
    };
    multiaddr
}
