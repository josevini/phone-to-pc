//! The exported helpers the app calls outside a session: pairing URIs, discovery and constants.

use std::collections::HashMap;

use clipsync_ffi::{
    CoreError, DiscoveredPeer, PairUri, SocketAddress, alpn, default_port, format_pair_uri, instance_name,
    is_valid_name, pairing_window_ms, parse_pair_uri, peer_from_service, service_type, short_id, txt_properties,
};

const ID: &str = "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027";
const ME: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn a_pairing_uri_is_parsed_into_plain_values() {
    let uri = format!(
        "clipsync://pair?v=1&id={ID}&name=Meu+PC&addr=192.168.0.10:47823&addr=%5Bfd00::2%5D:5000\
         &token=000102030405060708090a0b0c0d0e0f"
    );
    let expected = PairUri {
        id: ID.into(),
        name: "Meu PC".into(),
        addrs: vec![
            SocketAddress { ip: "192.168.0.10".into(), port: 47823 },
            SocketAddress { ip: "fd00::2".into(), port: 5000 },
        ],
        token: "000102030405060708090a0b0c0d0e0f".into(),
    };
    assert_eq!(parse_pair_uri(uri), Ok(expected));
}

#[test]
fn a_bad_pairing_uri_says_why() {
    let Err(CoreError::InvalidUri { reason }) = parse_pair_uri("https://example.com".into()) else {
        panic!("expected InvalidUri");
    };
    assert!(reason.contains("clipsync://pair"), "{reason}");
    let uri = format!("clipsync://pair?v=2&id={ID}&name=x&addr=10.0.0.1:1&token=000102030405060708090a0b0c0d0e0f");
    assert_eq!(parse_pair_uri(uri), Err(CoreError::UnsupportedUriVersion { version: "2".into() }));
}

fn invite() -> PairUri {
    PairUri {
        id: ID.into(),
        name: "Meu celular".into(),
        addrs: vec![
            SocketAddress { ip: "192.168.0.20".into(), port: 47823 },
            SocketAddress { ip: "fd00::2".into(), port: 47823 },
        ],
        token: "000102030405060708090a0b0c0d0e0f".into(),
    }
}

#[test]
fn a_pairing_uri_is_formatted_from_plain_values() {
    let uri = format_pair_uri(invite()).unwrap();
    assert_eq!(
        uri,
        format!(
            "clipsync://pair?v=1&id={ID}&name=Meu+celular&addr=192.168.0.20%3A47823&addr=%5Bfd00%3A%3A2%5D%3A47823\
             &token=000102030405060708090a0b0c0d0e0f"
        )
    );
    assert_eq!(parse_pair_uri(uri), Ok(invite()));
}

#[test]
fn a_pairing_uri_is_not_formatted_from_values_it_cannot_carry() {
    let bad_id = PairUri { id: "x".into(), ..invite() };
    assert_eq!(format_pair_uri(bad_id), Err(CoreError::InvalidDeviceId { id: "x".into() }));
    let bad_token = PairUri { token: "0001".into(), ..invite() };
    assert_eq!(format_pair_uri(bad_token), Err(CoreError::InvalidToken));
    let bad_ip = PairUri { addrs: vec![SocketAddress { ip: "phone.local".into(), port: 1 }], ..invite() };
    assert_eq!(
        format_pair_uri(bad_ip),
        Err(CoreError::InvalidUri { reason: "`phone.local` is not an IP address".into() })
    );
    // The spec requires at least one address and a 1–64 byte name.
    assert!(matches!(format_pair_uri(PairUri { addrs: vec![], ..invite() }), Err(CoreError::InvalidUri { .. })));
    assert!(matches!(format_pair_uri(PairUri { name: String::new(), ..invite() }), Err(CoreError::InvalidUri { .. })));
}

fn txt(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn a_device_advertises_the_protocol_version_and_its_id() {
    assert_eq!(txt_properties(ID.into()), Ok(txt(&[("v", "1"), ("id", ID)])));
    assert_eq!(txt_properties("x".into()), Err(CoreError::InvalidDeviceId { id: "x".into() }));
}

#[test]
fn a_browsed_service_is_read_as_the_core_reads_it() {
    let ips = vec!["192.168.0.20".into(), "fe80::1".into(), "not an ip".into()];
    let found = peer_from_service(ME.into(), txt(&[("v", "1"), ("id", ID), ("other", "x")]), 47823, ips).unwrap();
    let addrs = vec![SocketAddress { ip: "192.168.0.20".into(), port: 47823 }];
    assert_eq!(found, Some(DiscoveredPeer { id: ID.into(), addrs }));
    assert_eq!(peer_from_service(ME.into(), txt(&[("v", "2"), ("id", ID)]), 1, vec!["10.0.0.1".into()]), Ok(None));
    assert_eq!(
        peer_from_service("nope".into(), HashMap::new(), 1, vec![]),
        Err(CoreError::InvalidDeviceId { id: "nope".into() })
    );
}

#[test]
fn names_ids_and_constants() {
    assert_eq!(instance_name("Pixel 8".into(), ID.into()), Ok("Pixel 8 (86224755)".into()));
    assert_eq!(short_id(ID.into()), Ok("86224755".into()));
    assert_eq!(short_id("x".into()), Err(CoreError::InvalidDeviceId { id: "x".into() }));
    assert_eq!(instance_name("x".into(), "x".into()), Err(CoreError::InvalidDeviceId { id: "x".into() }));
    assert!(is_valid_name("Pixel 8".into()));
    assert!(!is_valid_name(String::new()));
    assert_eq!(default_port(), 47823);
    assert_eq!(pairing_window_ms(), 120_000);
    assert_eq!(alpn(), "clipsync/1");
    assert_eq!(service_type(), "_clipsync._tcp");
}
