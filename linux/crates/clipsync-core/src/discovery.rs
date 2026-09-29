//! Discovery (spec §3) and transport (spec §4) constants, and how to read a browsed service.
//!
//! Hosts do the mDNS I/O themselves; this module only decides what a service means.

use std::net::{IpAddr, SocketAddr};

use crate::identity::DeviceId;

/// DNS-SD service type, without the `.local.` domain.
pub const SERVICE_TYPE: &str = "_clipsync._tcp";

/// The TCP port a device listens on unless configured otherwise.
pub const DEFAULT_PORT: u16 = 47823;

/// ALPN protocol ID of the TLS connection.
pub const ALPN: &str = "clipsync/1";

/// TXT key holding the protocol version.
pub const TXT_VERSION: &str = "v";

/// TXT key holding the device ID.
pub const TXT_ID: &str = "id";

/// Value of [`TXT_VERSION`] this implementation speaks.
pub const TXT_VERSION_VALUE: &str = "1";

/// The device a browsed service belongs to and where to reach it, or `None` if the
/// service is not a clipsync v1 device other than `me`.
pub fn peer_from(
    me: &DeviceId,
    version: Option<&str>,
    id: Option<&str>,
    port: u16,
    ips: &[IpAddr],
) -> Option<(DeviceId, Vec<SocketAddr>)> {
    if version != Some(TXT_VERSION_VALUE) || port == 0 {
        return None;
    }
    let id: DeviceId = id?.parse().ok()?;
    if id == *me {
        return None;
    }
    let addrs: Vec<SocketAddr> =
        ips.iter().filter(|ip| !is_link_local_v6(ip)).map(|&ip| SocketAddr::new(ip, port)).collect();
    (!addrs.is_empty()).then_some((id, addrs))
}

/// DNS-SD instance name: the device name and its short ID, which keeps instances unique.
pub fn instance_name(name: &str, id: &DeviceId) -> String {
    const MAX_LABEL: usize = 63;
    let suffix = format!(" ({})", id.short());
    let mut name = name.to_owned();
    while name.len() + suffix.len() > MAX_LABEL {
        name.pop();
    }
    name + &suffix
}

/// IPv6 link-local addresses need an interface scope that a socket address from mDNS lacks.
pub fn is_link_local_v6(ip: &IpAddr) -> bool {
    matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex::Hex;

    const ME: DeviceId = DeviceId(Hex([0x11; 32]));
    const PEER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|ip| ip.parse().unwrap()).collect()
    }

    #[test]
    fn a_clipsync_device_is_found_at_its_usable_addresses() {
        let found = peer_from(&ME, Some("1"), Some(PEER), 47823, &ips(&["192.168.0.20", "fe80::1", "fd00::2"]));
        let (id, mut addrs) = found.unwrap();
        addrs.sort();
        assert_eq!(id.to_string(), PEER);
        assert_eq!(addrs, vec!["192.168.0.20:47823".parse().unwrap(), "[fd00::2]:47823".parse().unwrap()]);
    }

    #[test]
    fn other_versions_malformed_ids_and_this_device_are_ignored() {
        assert!(peer_from(&ME, Some("2"), Some(PEER), 1, &ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, None, Some(PEER), 1, &ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some("not-an-id"), 1, &ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), None, 1, &ips(&["10.0.0.1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some(&ME.to_string()), 1, &ips(&["10.0.0.1"])).is_none());
    }

    #[test]
    fn a_device_without_usable_addresses_is_ignored() {
        assert!(peer_from(&ME, Some("1"), Some(PEER), 1, &ips(&["fe80::1"])).is_none());
        assert!(peer_from(&ME, Some("1"), Some(PEER), 0, &ips(&["10.0.0.1"])).is_none());
    }

    #[test]
    fn instance_names_are_unique_and_fit_a_dns_label() {
        assert_eq!(instance_name("Meu PC", &ME), "Meu PC (11111111)");
        let long = instance_name(&"é".repeat(32), &ME);
        assert!(long.len() <= 63, "{} bytes", long.len());
        assert!(long.ends_with(" (11111111)"));
    }
}
