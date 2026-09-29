//! mDNS / DNS-SD: advertising this device and finding the others (spec §3).

use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use clipsync_core::DeviceId;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tracing::{debug, warn};

use crate::daemon::DaemonHandle;

pub const SERVICE_TYPE: &str = "_clipsync._tcp.local.";

/// The device a browsed service belongs to and where to reach it, or `None` if the
/// service is not a clipsync v1 device other than `me`.
pub(crate) fn peer_from(
    me: &DeviceId,
    version: Option<&str>,
    id: Option<&str>,
    port: u16,
    ips: impl IntoIterator<Item = IpAddr>,
) -> Option<(DeviceId, Vec<SocketAddr>)> {
    if version != Some("1") || port == 0 {
        return None;
    }
    let id: DeviceId = id?.parse().ok()?;
    if id == *me {
        return None;
    }
    let addrs: Vec<SocketAddr> =
        ips.into_iter().filter(|ip| !is_link_local_v6(ip)).map(|ip| SocketAddr::new(ip, port)).collect();
    (!addrs.is_empty()).then_some((id, addrs))
}

/// DNS-SD instance name: the device name and its short ID, which keeps instances unique.
pub(crate) fn instance_name(name: &str, id: &DeviceId) -> String {
    const MAX_LABEL: usize = 63;
    let suffix = format!(" ({})", id.short());
    let mut name = name.to_owned();
    while name.len() + suffix.len() > MAX_LABEL {
        name.pop();
    }
    name + &suffix
}

/// IPv6 link-local addresses need an interface scope that a socket address from mDNS lacks.
pub(crate) fn is_link_local_v6(ip: &IpAddr) -> bool {
    matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)
}

/// Advertises this device and reports the clipsync devices it finds to the daemon.
/// Dropping it withdraws the advertisement.
pub struct Discovery {
    mdns: ServiceDaemon,
    fullname: String,
}

impl Discovery {
    pub fn start(me: DeviceId, name: &str, port: u16, daemon: DaemonHandle) -> Result<Self> {
        let mdns = ServiceDaemon::new()?;
        let host = format!("clipsync-{}.local.", me.short());
        let id = me.to_string();
        let properties = [("v", "1"), ("id", id.as_str())];
        let info = ServiceInfo::new(SERVICE_TYPE, &instance_name(name, &me), &host, "", port, &properties[..])?
            .enable_addr_auto();
        let fullname = info.get_fullname().to_owned();
        mdns.register(info)?;

        let events = mdns.browse(SERVICE_TYPE)?;
        std::thread::Builder::new().name("mdns-browse".into()).spawn(move || {
            while let Ok(event) = events.recv() {
                let ServiceEvent::ServiceResolved(service) = event else { continue };
                let ips = service.get_addresses().iter().map(|ip| ip.to_ip_addr());
                let props = service.get_properties();
                let (version, id) = (props.get_property_val_str("v"), props.get_property_val_str("id"));
                if let Some((peer, addrs)) = peer_from(&me, version, id, service.get_port(), ips) {
                    debug!(peer = %peer.short(), ?addrs, "found on the network");
                    daemon.discovered(peer, addrs);
                }
            }
        })?;
        Ok(Discovery { mdns, fullname })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        if let Err(e) = self.mdns.unregister(&self.fullname).and_then(|_| self.mdns.shutdown().map(|_| ())) {
            warn!("mDNS shutdown failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clipsync_core::Hex;

    const ME: DeviceId = DeviceId(Hex([0x11; 32]));
    const PEER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    #[test]
    fn a_clipsync_device_is_found_at_its_usable_addresses() {
        let found = peer_from(&ME, Some("1"), Some(PEER), 47823, ips(&["192.168.0.20", "fe80::1", "fd00::2"]));
        let (id, mut addrs) = found.unwrap();
        addrs.sort();
        assert_eq!(id.to_string(), PEER);
        assert_eq!(addrs, vec!["192.168.0.20:47823".parse().unwrap(), "[fd00::2]:47823".parse().unwrap()]);
    }

    #[test]
    fn other_versions_malformed_ids_and_this_device_are_ignored() {
        assert!(peer_from(&ME, Some("2"), Some(PEER), 1, ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, None, Some(PEER), 1, ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some("not-an-id"), 1, ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), None, 1, ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some(&ME.to_string()), 1, ips(&["10.0.0.1"])).is_none());
    }

    #[test]
    fn a_device_without_usable_addresses_is_ignored() {
        assert!(peer_from(&ME, Some("1"), Some(PEER), 1, ips(&["fe80::1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some(PEER), 0, ips(&["10.0.0.1"])).is_none());
    }

    #[test]
    fn instance_names_are_unique_and_fit_a_dns_label() {
        assert_eq!(instance_name("Meu PC", &ME), "Meu PC (11111111)");
        let long = instance_name(&"é".repeat(32), &ME);
        assert!(long.len() <= 63, "{} bytes", long.len());
        assert!(long.ends_with(" (11111111)"));
    }
}
