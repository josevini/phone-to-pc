//! mDNS / DNS-SD: advertising this device and finding the others (spec §3).

use std::net::IpAddr;

use anyhow::Result;
use clipsync_core::DeviceId;
use clipsync_core::discovery::{TXT_ID, TXT_VERSION, TXT_VERSION_VALUE, instance_name, peer_from};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tracing::{debug, warn};

use crate::daemon::DaemonHandle;

/// The DNS-SD service type as `mdns-sd` names it, with its domain.
const MDNS_SERVICE_TYPE: &str = "_clipsync._tcp.local.";

/// Advertises this device and reports the clipsync devices it finds to the daemon.
/// Dropping it withdraws the advertisement.
pub struct Discovery {
    mdns: ServiceDaemon,
    me: DeviceId,
    port: u16,
    fullname: String,
}

/// This device's advertisement: its instance name, its host and port, and its TXT record (spec §3).
fn service_info(me: DeviceId, name: &str, port: u16) -> Result<ServiceInfo> {
    let host = format!("clipsync-{}.local.", me.short());
    let id = me.to_string();
    let properties = [(TXT_VERSION, TXT_VERSION_VALUE), (TXT_ID, id.as_str())];
    let info = ServiceInfo::new(MDNS_SERVICE_TYPE, &instance_name(name, &me), &host, "", port, &properties[..])?;
    Ok(info.enable_addr_auto())
}

impl Discovery {
    pub fn start(me: DeviceId, name: &str, port: u16, daemon: DaemonHandle) -> Result<Self> {
        let mdns = ServiceDaemon::new()?;
        let info = service_info(me, name, port)?;
        let fullname = info.get_fullname().to_owned();
        mdns.register(info)?;

        let events = mdns.browse(MDNS_SERVICE_TYPE)?;
        std::thread::Builder::new().name("mdns-browse".into()).spawn(move || {
            while let Ok(event) = events.recv() {
                let ServiceEvent::ServiceResolved(service) = event else { continue };
                let ips: Vec<IpAddr> = service.get_addresses().iter().map(|ip| ip.to_ip_addr()).collect();
                let props = service.get_properties();
                let (version, id) = (props.get_property_val_str(TXT_VERSION), props.get_property_val_str(TXT_ID));
                if let Some((peer, addrs)) = peer_from(&me, version, id, service.get_port(), &ips) {
                    debug!(peer = %peer.short(), ?addrs, "found on the network");
                    daemon.discovered(peer, addrs);
                }
            }
        })?;
        Ok(Discovery { mdns, me, port, fullname })
    }

    /// Advertises this device under its new `name` instead.
    pub fn rename(&mut self, name: &str) -> Result<()> {
        let info = service_info(self.me, name, self.port)?;
        self.mdns.unregister(&self.fullname)?;
        self.fullname = info.get_fullname().to_owned();
        self.mdns.register(info)?;
        Ok(())
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
    use clipsync_core::discovery::SERVICE_TYPE;

    #[test]
    fn the_mdns_service_type_is_the_protocols_in_the_local_domain() {
        assert_eq!(MDNS_SERVICE_TYPE, format!("{SERVICE_TYPE}.local."));
    }

    #[test]
    fn the_advertisement_carries_the_devices_name_id_and_port() {
        let me = clipsync_core::DeviceId(clipsync_core::Hex([0xab; 32]));
        let info = service_info(me, "book2", 47823).unwrap();
        assert!(info.get_fullname().starts_with(&instance_name("book2", &me)), "{}", info.get_fullname());
        assert_eq!(info.get_port(), 47823);
        assert_eq!(info.get_property_val_str(TXT_ID), Some(me.to_string().as_str()));
        assert_eq!(info.get_property_val_str(TXT_VERSION), Some(TXT_VERSION_VALUE));
    }
}
