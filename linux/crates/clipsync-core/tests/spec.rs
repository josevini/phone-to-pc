//! Known-answer tests for every deterministic rule of `spec/protocol.md`.
//!
//! Expected values (device IDs, digests, SAS codes) were first computed by an
//! independent Python implementation of the spec; keep them as literals.

use clipsync_core::clip::{ClipTracker, LocalOutcome, MAX_TEXT_LEN, RemoteOutcome, TEXT_MIME, text_digest};
use clipsync_core::frame::{Decoded, FrameDecoder, MAX_FRAME_LEN};
use clipsync_core::message::Clip;
use clipsync_core::pairing::{PairUri, sas_code, sas_commit};
use clipsync_core::{DeviceId, Hex, Hex16, Hex32};
use serde_json::{Value, json};

const ID_A: &str = "86224755c0ff3b3b412a5da3ef12466cb12c48326e3454c02687f2cc88771027";
const ID_B: &str = "bdc09de0a1b08196dc73d3ee8a7b905a2b16a5879bd0253d4700aa53e37b591f";

fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// Spec §2: device ID = SHA-256 of the SubjectPublicKeyInfo DER.
#[test]
fn device_id() {
    let cases = [
        (
            "3059301306072a8648ce3d020106082a8648ce3d030107034200041a8666b545ff4f6d74eeaa4c2ab0cb1514e25631499413a079c3\
             ec11b5f8b851b6ceb51c0710f78400664b9129c4b507042231e55fc354607a09820d0aea7186",
            ID_A,
        ),
        (
            "3059301306072a8648ce3d020106082a8648ce3d03010703420004a65d1ad71a7654b2991e908a9ede64a3b54706281e6fd5956166\
             5ecce994016b565568c36054aa040df2c44248b7b628ec6a0b5cab7d854062296dcdc214c9b3",
            ID_B,
        ),
    ];
    for (spki, id) in cases {
        assert_eq!(DeviceId::from_spki_der(&hex_bytes(spki)).to_string(), id);
    }
}

/// Spec §5.2: clip.sha256 = SHA-256 of the UTF-8 bytes of the text.
#[test]
fn clip_hash() {
    let cases = [
        ("hello world", "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"),
        ("Olá, coração! Ação às três.", "2ebf335c03444c62fec493b1d2b767971286429b1201f783638f4c3401c20cdc"),
        ("copiado 👋🏽 do celular", "5594383b3232e5652061e62f5758bc5cdd607235464c2594a041ddfee6f1e950"),
        ("line1\r\nline2\n\tindented", "6eeed436b15492028fa4caa2208e53d3b461024e3825cb8c14431497d24e5393"),
        ("a\u{0}b\u{1f}c", "99fdd2a3794d507f172d5ea24ab112f4abd82687e8441699af24977660d3f84f"),
        ("x", "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"),
    ];
    for (text, sha) in cases {
        assert_eq!(text_digest(text).to_string(), sha, "{text:?}");
    }
}

/// Spec §7.3: SAS commitment and 6-digit code.
#[test]
fn sas() {
    const NONCE_D: &str = "5222459df5b567bd592eb798bb89b48a3b9bf6912fc822f8af33d7dc8e775779";
    const NONCE_A: &str = "c3a4628a99c01c9db84daeb298543ed35110e4126c101b492c44000d54bd839b";
    const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const COMMIT_D: &str = "75780358c3ba8169b1465d070ec01759f5a0d38caa00369cd414c23608ff50e5";
    // (name, dialer, acceptor, nonce_dialer, nonce_acceptor, commit, sas)
    let cases = [
        ("a dials b", ID_A, ID_B, NONCE_D, NONCE_A, COMMIT_D, "264591"),
        ("b dials a, same nonces", ID_B, ID_A, NONCE_D, NONCE_A, COMMIT_D, "686771"),
        (
            "zero nonces",
            ID_A,
            ID_B,
            ZERO,
            ZERO,
            "2add9cb276ce6e53c13141368c0cfc4f4e9d6770405d79f6ded655ff0a95c08f",
            "050602",
        ),
        (
            "leading zero",
            ID_A,
            ID_B,
            NONCE_D,
            "5ccc4f8ac271b9d706a88d331729935894f42319014eaacbef74b45371a4cc06",
            COMMIT_D,
            "037725",
        ),
    ];
    for (name, id_d, id_a, nonce_d, nonce_a, commit, sas) in cases {
        let nonce_d: Hex32 = nonce_d.parse().unwrap();
        let nonce_a: Hex32 = nonce_a.parse().unwrap();
        assert_eq!(sas_commit(&nonce_d).to_string(), commit, "{name}");
        let code = sas_code(&id_d.parse().unwrap(), &id_a.parse().unwrap(), &nonce_d, &nonce_a);
        assert_eq!(format!("{code:06}"), sas, "{name}");
    }
}

/// What decoding one frame must produce (spec §4.1).
enum Expect {
    /// The normalised message: unknown fields dropped, hex lowercased, absent optional fields omitted.
    Message(Value),
    Ignored,
    Error(&'static str),
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(payload);
    out
}

fn json_frame(value: &Value) -> Vec<u8> {
    frame(value.to_string().as_bytes())
}

fn set(mut value: Value, key: &str, new: Value) -> Value {
    value[key] = new;
    value
}

fn unset(mut value: Value, key: &str) -> Value {
    value.as_object_mut().unwrap().remove(key);
    value
}

/// Spec §4.1 and §5: framing, decoding and message validation.
#[test]
fn frames() {
    let digest = |t: &str| text_digest(t).to_string();
    let n32 = digest("n");
    let token = "00112233445566778899aabbccddeeff";
    let hello = json!({
        "type": "hello", "proto": 1, "id": ID_A, "name": "vini-laptop",
        "platform": "linux", "caps": ["text"], "seq": 42,
    });
    let clip = json!({
        "type": "clip", "id": "0123456789abcdef0123456789abcdef", "origin": ID_B, "seq": 7,
        "ts": 1790000000000u64, "mime": TEXT_MIME, "text": "olá 👋", "sha256": digest("olá 👋"),
    });
    let ack = json!({"type": "ack", "id": "0123456789abcdef0123456789abcdef", "applied": true});
    let ok = |v: &Value| (json_frame(v), Expect::Message(v.clone()));
    let bad = |v: &Value| (json_frame(v), Expect::Error("invalid_message"));

    let cases: Vec<(&str, (Vec<u8>, Expect))> = vec![
        ("hello", ok(&hello)),
        (
            "hello, unknown fields ignored",
            (json_frame(&set(hello.clone(), "future", json!({"x": 1}))), Expect::Message(hello.clone())),
        ),
        (
            "hello, uppercase id normalised",
            (json_frame(&set(hello.clone(), "id", json!(ID_A.to_uppercase()))), Expect::Message(hello.clone())),
        ),
        ("hello, other proto still decodes", ok(&set(hello.clone(), "proto", json!(2)))),
        ("hello, empty name", bad(&set(hello.clone(), "name", json!("")))),
        ("hello, name of 66 bytes", bad(&set(hello.clone(), "name", json!("é".repeat(33))))),
        ("hello, name of 64 bytes", ok(&set(hello.clone(), "name", json!("é".repeat(32))))),
        ("hello, short id", bad(&set(hello.clone(), "id", json!(&ID_A[..62])))),
        ("hello, id not hex", bad(&set(hello.clone(), "id", json!("z".repeat(64))))),
        ("hello, missing seq", bad(&unset(hello.clone(), "seq"))),
        ("hello, negative seq", bad(&set(hello.clone(), "seq", json!(-1)))),
        ("clip", ok(&clip)),
        ("clip, unknown mime still decodes", ok(&set(clip.clone(), "mime", json!("image/png")))),
        ("clip, sha256 mismatch", bad(&set(clip.clone(), "sha256", json!(digest("other"))))),
        ("clip, empty text", bad(&set(set(clip.clone(), "text", json!("")), "sha256", json!(digest(""))))),
        ("clip, seq 0", bad(&set(clip.clone(), "seq", json!(0)))),
        ("clip, short id", bad(&set(clip.clone(), "id", json!("0123")))),
        ("ack", ok(&ack)),
        ("ack, applied not bool", bad(&set(ack.clone(), "applied", json!("yes")))),
        ("ping", ok(&json!({"type": "ping"}))),
        ("pong", ok(&json!({"type": "pong"}))),
        ("unpair", ok(&json!({"type": "unpair"}))),
        ("error with message", ok(&json!({"type": "error", "code": "not_paired", "message": "unknown device"}))),
        ("error without message", ok(&json!({"type": "error", "code": "bad_token"}))),
        ("pair_request token", ok(&json!({"type": "pair_request", "method": "token", "token": token}))),
        ("pair_request token, missing token", bad(&json!({"type": "pair_request", "method": "token"}))),
        ("pair_request sas", ok(&json!({"type": "pair_request", "method": "sas", "commit": n32}))),
        ("pair_request sas, missing commit", bad(&json!({"type": "pair_request", "method": "sas", "token": token}))),
        ("pair_request, unknown method", bad(&json!({"type": "pair_request", "method": "nfc"}))),
        ("pair_nonce", ok(&json!({"type": "pair_nonce", "nonce": n32}))),
        ("pair_reveal", ok(&json!({"type": "pair_reveal", "nonce": n32}))),
        ("pair_nonce, short nonce", bad(&json!({"type": "pair_nonce", "nonce": &n32[..32]}))),
        ("pair_result ok", ok(&json!({"type": "pair_result", "ok": true}))),
        ("pair_result rejected", ok(&json!({"type": "pair_result", "ok": false, "reason": "user_rejected"}))),
        ("unknown type", (json_frame(&json!({"type": "file_offer", "size": 3})), Expect::Ignored)),
        ("missing type", bad(&json!({"proto": 1}))),
        ("type not a string", bad(&json!({"type": 5}))),
        ("array payload", bad(&json!([1, 2]))),
        ("invalid json", (frame(b"{\"type\": "), Expect::Error("invalid_json"))),
        ("invalid utf-8", (frame(b"{\"type\":\"ping\",\"x\":\"\xff\"}"), Expect::Error("invalid_json"))),
        ("empty frame", (vec![0, 0, 0, 0], Expect::Error("empty_frame"))),
        (
            "too large, header only",
            (((MAX_FRAME_LEN + 1) as u32).to_be_bytes().to_vec(), Expect::Error("frame_too_large")),
        ),
    ];

    for (name, (bytes, expect)) in cases {
        let mut decoder = FrameDecoder::new();
        decoder.push(&bytes);
        match (decoder.next_frame(), expect) {
            (Ok(Some(Decoded::Message(msg))), Expect::Message(want)) => {
                assert_eq!(serde_json::to_value(&msg).unwrap(), want, "{name}")
            }
            (Ok(Some(Decoded::Ignored { .. })), Expect::Ignored) => {}
            (Err(e), Expect::Error(code)) => assert_eq!(e.code(), code, "{name}: {e}"),
            (got, _) => panic!("{name}: unexpected {got:?}"),
        }
    }
}

/// Spec §8: pairing URI.
#[test]
fn pair_uri() {
    const TOKEN: &str = "00112233445566778899aabbccddeeff";
    let base = format!("clipsync://pair?v=1&id={ID_A}&name=Meu+PC&addr=192.168.0.10:47823&token={TOKEN}");
    let valid = [
        ("basic", base.clone(), "Meu PC", vec!["192.168.0.10:47823"]),
        (
            "multiple addrs, IPv6",
            format!(
                "clipsync://pair?v=1&id={ID_A}&name=Meu%20PC&addr=192.168.0.10:47823\
                 &addr=%5Bfe80::1%5D:47823&addr=[fd00::2]:5000&token={TOKEN}"
            ),
            "Meu PC",
            vec!["192.168.0.10:47823", "[fe80::1]:47823", "[fd00::2]:5000"],
        ),
        (
            "percent-encoded UTF-8 and plus",
            format!("clipsync://pair?v=1&id={ID_A}&name=Cora%C3%A7%C3%A3o%2B1&addr=10.0.0.2:1&token={TOKEN}"),
            "Coração+1",
            vec!["10.0.0.2:1"],
        ),
        (
            "uppercase hex and unknown parameter",
            format!(
                "clipsync://pair?future=1&v=1&id={}&name=Meu+PC&addr=192.168.0.10:47823&token={}",
                ID_A.to_uppercase(),
                TOKEN.to_uppercase()
            ),
            "Meu PC",
            vec!["192.168.0.10:47823"],
        ),
    ];
    for (name, uri, want_name, want_addrs) in valid {
        let parsed = PairUri::parse(&uri).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(parsed.id.to_string(), ID_A, "{name}");
        assert_eq!(parsed.name, want_name, "{name}");
        assert_eq!(parsed.token.to_string(), TOKEN, "{name}");
        let addrs: Vec<String> = parsed.addrs.iter().map(|a| a.to_string()).collect();
        assert_eq!(addrs, want_addrs, "{name}");
    }

    let invalid = [
        ("wrong scheme", base.replace("clipsync://", "https://"), "invalid_uri"),
        ("wrong action", base.replace("//pair?", "//join?"), "invalid_uri"),
        ("not a URI", "not a uri".to_owned(), "invalid_uri"),
        ("extra path", base.replace("//pair?", "//pair/extra?"), "invalid_uri"),
        ("missing id", base.replace(&format!("&id={ID_A}"), ""), "invalid_uri"),
        ("unsupported version", base.replace("v=1", "v=2"), "unsupported_version"),
        ("missing version", base.replace("v=1&", ""), "invalid_uri"),
        ("duplicate token", format!("{base}&token={}", "f".repeat(32)), "invalid_uri"),
        ("port zero", base.replace(":47823", ":0"), "invalid_uri"),
        ("missing token", base.replace(&format!("&token={TOKEN}"), ""), "invalid_uri"),
        ("short token", base.replace(TOKEN, &TOKEN[..30]), "invalid_uri"),
        ("short id", base.replace(ID_A, &ID_A[..60]), "invalid_uri"),
        ("missing addr", base.replace("&addr=192.168.0.10:47823", ""), "invalid_uri"),
        ("hostname addr", base.replace("192.168.0.10", "my-pc.local"), "invalid_uri"),
        ("addr without port", base.replace(":47823", ""), "invalid_uri"),
        ("empty name", base.replace("name=Meu+PC", "name="), "invalid_uri"),
        ("name too long", base.replace("Meu+PC", &"a".repeat(65)), "invalid_uri"),
    ];
    for (name, uri, code) in invalid {
        let err = PairUri::parse(&uri).expect_err(name);
        assert_eq!(err.code(), code, "{name}: {err}");
    }
}

enum Op {
    Local(String),
    Remote { seq: u64, origin: u8, text: &'static str },
    Hello(u64),
}

#[derive(Debug, PartialEq)]
enum Out {
    Emit(u64),
    Unchanged,
    Empty,
    TooLarge,
    Apply,
    SameContent,
    Stale,
    Nothing,
}

fn local(text: &str) -> Op {
    Op::Local(text.to_owned())
}

fn remote(seq: u64, origin: u8, text: &'static str) -> Op {
    Op::Remote { seq, origin, text }
}

/// Every device ID in these scenarios is one byte repeated 32 times.
fn id(byte: u8) -> DeviceId {
    DeviceId(Hex([byte; 32]))
}

/// One ordering step: the operation, its outcome and the Lamport counter afterwards.
type Step = (Op, Out, u64);

/// Spec §6: clip ordering and echo suppression.
#[test]
fn ordering() {
    const ME: u8 = 0x55;
    const LOW: u8 = 0x11;
    const HIGH: u8 = 0xee;
    let scenarios: Vec<(&str, Vec<Step>)> = vec![
        (
            "local changes emit increasing seq",
            vec![
                (local("a"), Out::Emit(1), 1),
                (local("b"), Out::Emit(2), 2),
                (local("b"), Out::Unchanged, 2),
                (local(""), Out::Empty, 2),
                (local("a"), Out::Emit(3), 3),
            ],
        ),
        (
            "text over 1 MiB is not sent",
            vec![
                (Op::Local("a".repeat(MAX_TEXT_LEN)), Out::Emit(1), 1),
                (Op::Local("b".repeat(MAX_TEXT_LEN + 1)), Out::TooLarge, 1),
            ],
        ),
        (
            "remote clip applied, then its echo suppressed",
            vec![
                (local("a"), Out::Emit(1), 1),
                (remote(5, HIGH, "from peer"), Out::Apply, 5),
                (local("from peer"), Out::Unchanged, 5),
                (local("mine again"), Out::Emit(6), 6),
            ],
        ),
        (
            "stale remote clip still advances the counter",
            vec![
                (local("a"), Out::Emit(1), 1),
                (local("b"), Out::Emit(2), 2),
                (local("c"), Out::Emit(3), 3),
                (remote(2, HIGH, "old"), Out::Stale, 3),
                (local("d"), Out::Emit(4), 4),
            ],
        ),
        (
            "concurrent, same seq: higher origin wins",
            vec![
                (local("a"), Out::Emit(1), 1),
                (remote(1, HIGH, "peer wins"), Out::Apply, 1),
                (remote(1, LOW, "peer loses"), Out::Stale, 1),
            ],
        ),
        (
            "concurrent, same seq: lower origin loses",
            vec![(local("a"), Out::Emit(1), 1), (remote(1, LOW, "peer loses"), Out::Stale, 1)],
        ),
        (
            "hello raises the counter",
            vec![
                (Op::Hello(100), Out::Nothing, 100),
                (local("a"), Out::Emit(101), 101),
                (remote(50, HIGH, "late"), Out::Stale, 101),
            ],
        ),
        (
            "remote clip with the same content is not written",
            vec![
                (local("same"), Out::Emit(1), 1),
                (remote(9, LOW, "same"), Out::SameContent, 9),
                (remote(9, LOW, "same"), Out::Stale, 9),
            ],
        ),
        ("first remote clip applies on empty state", vec![(remote(3, LOW, "hi"), Out::Apply, 3)]),
    ];

    for (name, steps) in scenarios {
        let mut tracker = ClipTracker::new(id(ME), 0);
        for (i, (op, want, lamport)) in steps.into_iter().enumerate() {
            let got = match op {
                Op::Local(text) => match tracker.local_change(text, 0) {
                    LocalOutcome::Emit(clip) => Out::Emit(clip.seq),
                    LocalOutcome::Unchanged => Out::Unchanged,
                    LocalOutcome::Empty => Out::Empty,
                    LocalOutcome::TooLarge => Out::TooLarge,
                },
                Op::Remote { seq, origin, text } => {
                    let clip = Clip {
                        id: Hex16::random(),
                        origin: id(origin),
                        seq,
                        ts: 0,
                        mime: TEXT_MIME.into(),
                        text: text.into(),
                        sha256: text_digest(text),
                    };
                    match tracker.receive(&clip) {
                        RemoteOutcome::Apply => Out::Apply,
                        RemoteOutcome::SameContent => Out::SameContent,
                        RemoteOutcome::Stale => Out::Stale,
                    }
                }
                Op::Hello(seq) => {
                    tracker.observe_hello(seq);
                    Out::Nothing
                }
            };
            assert_eq!(got, want, "{name}, step {i}");
            assert_eq!(tracker.lamport(), lamport, "{name}, step {i}: lamport");
        }
    }
}
