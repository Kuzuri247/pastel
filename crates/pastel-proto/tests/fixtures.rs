//! Cross-codec wire fixtures. The TypeScript codec in
//! `frontend/tests/postcard.test.ts` asserts the same hex strings. Both
//! sides must agree on the bytes or browsers and Rust clients will not talk.

use pastel_proto::*;

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[test]
fn client_pong_fixture() {
    let msg = ClientMsg::Pong { nonce: 7 };
    assert_eq!(hex(&encode(&msg).unwrap()), "0507");
}

#[test]
fn client_chat_fixture() {
    let msg = ClientMsg::Chat { text: "hi".into() };
    assert_eq!(hex(&encode(&msg).unwrap()), "02026869");
}

#[test]
fn client_stroke_single_point_fixture() {
    let msg = ClientMsg::Stroke {
        stroke_id: 7,
        origin: (0, 0),
        color: 0,
        width: 4,
        points: vec![Point {
            dx: 1,
            dy: -2,
            dt: 16,
            pressure: 200,
        }],
        finished: false,
    };
    // variant 1, stroke_id 7, origin 0 0, color 0, width 4, len 1, point
    // [01 fe 10 c8], finished 0.
    assert_eq!(hex(&encode(&msg).unwrap()), "0107000000040101fe10c800");
}

#[test]
fn server_bye_reconnect_fixture() {
    let msg = ServerMsg::Bye {
        reason: ByeReason::Reconnect,
    };
    assert_eq!(hex(&encode(&msg).unwrap()), "0800");
}

#[test]
fn server_welcome_empty_snapshot_fixture() {
    let msg = ServerMsg::Welcome {
        you: 1,
        snapshot: RoomSnapshot {
            players: vec![],
            completed: vec![],
            seq: 0,
        },
        seq: 0,
        lk_token: String::new(),
    };
    assert_eq!(hex(&encode(&msg).unwrap()), "00010000000000");
}
