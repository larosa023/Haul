//! Network adapters (for binding torrent traffic to a VPN) and NAT-PMP port mapping.

use serde::Serialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::Duration;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    /// What Windows shows ("Ethernet", "ProtonVPN", "Wi-Fi").
    pub name: String,
    pub description: String,
    pub index: u32,
    pub ipv4: Option<Ipv4Addr>,
    pub up: bool,
}

fn display_name(i: &netdev::Interface) -> String {
    i.friendly_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| i.name.clone())
}

pub fn adapters() -> Vec<Adapter> {
    netdev::get_interfaces()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| Adapter {
            name: display_name(&i),
            description: i.description.clone().unwrap_or_default(),
            index: i.index,
            ipv4: i.ipv4.first().map(|n| n.addr()),
            up: i.is_up() && i.is_running() && !i.ipv4.is_empty(),
        })
        .collect()
}

/// The adapter with this name, if it exists and is connected.
pub fn find(name: &str) -> Option<Adapter> {
    adapters().into_iter().find(|a| a.name.eq_ignore_ascii_case(name) || a.description.eq_ignore_ascii_case(name))
}

pub fn default_gateway() -> Option<Ipv4Addr> {
    let i = netdev::get_default_interface().ok()?;
    i.gateway.and_then(|g| g.ipv4.first().copied())
}

/// Asks the router (NAT-PMP, RFC 6886) to forward `port` (TCP and UDP) to this PC.
/// Returns the external port on success.
pub fn natpmp_map(port: u16, lifetime_secs: u32, local: Option<Ipv4Addr>) -> Result<u16, String> {
    let gw = default_gateway().ok_or("no default gateway")?;
    let bind = SocketAddrV4::new(local.unwrap_or(Ipv4Addr::UNSPECIFIED), 0);
    let sock = UdpSocket::bind(bind).map_err(|e| e.to_string())?;
    sock.set_read_timeout(Some(Duration::from_millis(1500))).ok();
    let mut external = 0;
    // opcode 1 = UDP, 2 = TCP
    for op in [2u8, 1u8] {
        let mut req = [0u8; 12];
        req[1] = op;
        req[4..6].copy_from_slice(&port.to_be_bytes());
        req[6..8].copy_from_slice(&port.to_be_bytes());
        req[8..12].copy_from_slice(&lifetime_secs.to_be_bytes());
        let mut ok = false;
        for _ in 0..2 {
            sock.send_to(&req, SocketAddrV4::new(gw, 5351)).map_err(|e| e.to_string())?;
            let mut buf = [0u8; 16];
            if let Ok((n, from)) = sock.recv_from(&mut buf) {
                if n >= 16 && from.ip() == IpAddr::V4(gw) && buf[1] == 128 + op {
                    let code = u16::from_be_bytes([buf[2], buf[3]]);
                    if code != 0 {
                        return Err(format!("router refused (code {code})"));
                    }
                    external = u16::from_be_bytes([buf[10], buf[11]]);
                    ok = true;
                    break;
                }
            }
        }
        if !ok {
            return Err("no NAT-PMP answer from the router".into());
        }
    }
    Ok(external)
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_adapters() {
        // Just make sure enumeration works on this machine.
        let _ = super::adapters();
    }
}
