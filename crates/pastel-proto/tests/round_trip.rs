use pastel_proto::*;
use proptest::collection::{vec, SizeRange};
use proptest::prelude::*;

fn arb_room_code() -> impl Strategy<Value = RoomCode> {
    "[0-9A-HJKMNP-TV-Z]{6}".prop_map(|s| RoomCode::parse(&s).unwrap())
}

fn arb_name() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_]{0,32}".prop_map(String::from)
}

fn arb_text(max: usize) -> impl Strategy<Value = String> {
    proptest::string::string_regex(&format!(".{{0,{max}}}"))
        .unwrap()
        .prop_map(move |s| {
            let mut s = s;
            while s.len() > max {
                s.pop();
            }
            s
        })
}

fn arb_point() -> impl Strategy<Value = Point> {
    (any::<i8>(), any::<i8>(), any::<u8>(), any::<u8>()).prop_map(|(dx, dy, dt, pressure)| Point {
        dx,
        dy,
        dt,
        pressure,
    })
}

fn arb_points(max: usize) -> impl Strategy<Value = Vec<Point>> {
    vec(arb_point(), SizeRange::from(0..=max))
}

fn arb_player() -> impl Strategy<Value = Player> {
    (any::<u32>(), arb_name()).prop_map(|(id, name)| Player { id, name })
}

fn arb_completed_stroke() -> impl Strategy<Value = CompletedStroke> {
    (
        any::<u32>(),
        any::<u32>(),
        (any::<u16>(), any::<u16>()),
        arb_points(MAX_POINTS_PER_BATCH * 4),
    )
        .prop_map(|(player, stroke_id, origin, points)| CompletedStroke {
            player,
            stroke_id,
            origin,
            points,
        })
}

fn arb_snapshot() -> impl Strategy<Value = RoomSnapshot> {
    (
        vec(arb_player(), 0..=MAX_PLAYERS_PER_ROOM),
        vec(arb_completed_stroke(), 0..=32),
        any::<u64>(),
    )
        .prop_map(|(players, completed, seq)| RoomSnapshot {
            players,
            completed,
            seq,
        })
}

fn arb_hello() -> impl Strategy<Value = Hello> {
    (
        arb_room_code(),
        arb_name(),
        proptest::option::of(any::<u64>()),
    )
        .prop_map(|(room, name, resume_from)| Hello {
            room,
            name,
            resume_from,
        })
}

fn arb_game_action() -> impl Strategy<Value = GameAction> {
    prop_oneof![
        Just(GameAction::Start),
        any::<u8>().prop_map(GameAction::PickWord),
        any::<u32>().prop_map(GameAction::Kick),
    ]
}

fn arb_game_event() -> impl Strategy<Value = GameEvent> {
    prop_oneof![
        (any::<u32>(), arb_text(MAX_WORD_LEN), any::<u32>()).prop_map(
            |(drawer, word_mask, duration_ms)| GameEvent::RoundStart {
                drawer,
                word_mask,
                duration_ms,
            }
        ),
        (
            arb_text(MAX_WORD_LEN),
            vec((any::<u32>(), any::<u32>()), 0..=MAX_PLAYERS_PER_ROOM),
        )
            .prop_map(|(word, scores)| GameEvent::RoundEnd { word, scores }),
        vec((any::<u32>(), any::<u32>()), 0..=MAX_PLAYERS_PER_ROOM)
            .prop_map(|final_scores| GameEvent::GameOver { final_scores }),
    ]
}

fn arb_guess_kind() -> impl Strategy<Value = GuessKind> {
    prop_oneof![Just(GuessKind::Correct), Just(GuessKind::Close)]
}

fn arb_bye_reason() -> impl Strategy<Value = ByeReason> {
    prop_oneof![
        Just(ByeReason::Reconnect),
        Just(ByeReason::Kicked),
        Just(ByeReason::RoomClosed),
        Just(ByeReason::RoomFull),
        Just(ByeReason::BadFrame),
    ]
}

fn arb_client_msg() -> impl Strategy<Value = ClientMsg> {
    prop_oneof![
        arb_hello().prop_map(ClientMsg::Hello),
        (
            any::<u32>(),
            (any::<u16>(), any::<u16>()),
            arb_points(MAX_POINTS_PER_BATCH),
            any::<bool>(),
        )
            .prop_map(|(stroke_id, origin, points, finished)| ClientMsg::Stroke {
                stroke_id,
                origin,
                points,
                finished,
            }),
        arb_text(MAX_CHAT_LEN).prop_map(|text| ClientMsg::Chat { text }),
        arb_text(MAX_GUESS_LEN).prop_map(|text| ClientMsg::Guess { text }),
        arb_game_action().prop_map(ClientMsg::Game),
        any::<u32>().prop_map(|nonce| ClientMsg::Pong { nonce }),
    ]
}

fn arb_server_leaf_msg() -> impl Strategy<Value = ServerMsg> {
    prop_oneof![
        (
            any::<u32>(),
            arb_snapshot(),
            any::<u64>(),
            arb_text(MAX_LK_TOKEN_LEN)
        )
            .prop_map(|(you, snapshot, seq, lk_token)| ServerMsg::Welcome {
                you,
                snapshot,
                seq,
                lk_token,
            }),
        (
            any::<u64>(),
            any::<u32>(),
            any::<u32>(),
            (any::<u16>(), any::<u16>()),
            arb_points(MAX_POINTS_PER_BATCH),
            any::<bool>(),
        )
            .prop_map(|(seq, player, stroke_id, origin, points, finished)| {
                ServerMsg::Stroke {
                    seq,
                    player,
                    stroke_id,
                    origin,
                    points,
                    finished,
                }
            }),
        (any::<u64>(), any::<u32>(), arb_text(MAX_CHAT_LEN))
            .prop_map(|(seq, player, text)| ServerMsg::Chat { seq, player, text }),
        (any::<u64>(), any::<u32>(), arb_guess_kind())
            .prop_map(|(seq, player, kind)| ServerMsg::Guess { seq, player, kind }),
        (
            any::<u64>(),
            vec(arb_player(), 0..=MAX_PLAYERS_PER_ROOM),
            vec(any::<u32>(), 0..=MAX_PLAYERS_PER_ROOM),
        )
            .prop_map(|(seq, joined, left)| ServerMsg::Presence { seq, joined, left }),
        (any::<u64>(), arb_game_event()).prop_map(|(seq, event)| ServerMsg::Game { seq, event }),
        any::<u32>().prop_map(|nonce| ServerMsg::Ping { nonce }),
        arb_bye_reason().prop_map(|reason| ServerMsg::Bye { reason }),
    ]
}

fn arb_server_msg() -> impl Strategy<Value = ServerMsg> {
    prop_oneof![
        arb_server_leaf_msg(),
        vec(arb_server_leaf_msg(), 0..=16).prop_map(|events| ServerMsg::Resume { events }),
    ]
}

proptest! {
    #[test]
    fn round_trip_client_msg(msg in arb_client_msg()) {
        let bytes = encode(&msg).unwrap();
        let back: ClientMsg = decode(&bytes).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn round_trip_server_msg(msg in arb_server_msg()) {
        let bytes = encode(&msg).unwrap();
        let back: ServerMsg = decode(&bytes).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn validated_decode_accepts_well_formed_client(msg in arb_client_msg()) {
        let bytes = encode(&msg).unwrap();
        let back = decode_client_validated(&bytes).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn validated_decode_accepts_well_formed_server(msg in arb_server_msg()) {
        let bytes = encode(&msg).unwrap();
        let back = decode_server_validated(&bytes).unwrap();
        prop_assert_eq!(msg, back);
    }

    #[test]
    fn room_code_round_trip(code in arb_room_code()) {
        let s = code.to_string();
        let back = RoomCode::parse(&s).unwrap();
        prop_assert_eq!(code, back);
    }
}

#[test]
fn validation_rejects_oversize_chat() {
    let too_long = "a".repeat(MAX_CHAT_LEN + 1);
    let msg = ClientMsg::Chat { text: too_long };
    let bytes = encode(&msg).unwrap();
    let err = decode_client_validated(&bytes).unwrap_err();
    assert!(matches!(err, CodecError::FieldTooLong { field, .. } if field == "chat.text"));
}

#[test]
fn validation_rejects_oversize_stroke_batch() {
    let msg = ClientMsg::Stroke {
        stroke_id: 1,
        origin: (0, 0),
        points: vec![
            Point {
                dx: 0,
                dy: 0,
                dt: 0,
                pressure: 0
            };
            MAX_POINTS_PER_BATCH + 1
        ],
        finished: false,
    };
    let bytes = encode(&msg).unwrap();
    let err = decode_client_validated(&bytes).unwrap_err();
    assert!(matches!(err, CodecError::FieldTooLong { field, .. } if field == "stroke.points"));
}

#[test]
fn validation_rejects_oversize_name() {
    let msg = ClientMsg::Hello(Hello {
        room: RoomCode::parse("ABC234").unwrap(),
        name: "a".repeat(MAX_NAME_LEN + 1),
        resume_from: None,
    });
    let bytes = encode(&msg).unwrap();
    let err = decode_client_validated(&bytes).unwrap_err();
    assert!(matches!(err, CodecError::FieldTooLong { field, .. } if field == "hello.name"));
}

#[test]
fn validation_rejects_oversize_frame() {
    let huge = vec![0u8; MAX_FRAME_BYTES + 1];
    let err = decode::<ClientMsg>(&huge).unwrap_err();
    assert!(matches!(err, CodecError::FrameTooLarge(_)));
}

#[test]
fn validation_rejects_nested_resume() {
    let inner = ServerMsg::Resume {
        events: vec![ServerMsg::Ping { nonce: 1 }],
    };
    let outer = ServerMsg::Resume {
        events: vec![inner],
    };
    let bytes = encode(&outer).unwrap();
    let err = decode_server_validated(&bytes).unwrap_err();
    assert!(matches!(err, CodecError::ResumeTooDeep));
}

#[test]
fn stroke_batch_size_budget() {
    let msg = ClientMsg::Stroke {
        stroke_id: 7,
        origin: (320, 240),
        points: vec![
            Point {
                dx: 1,
                dy: -2,
                dt: 16,
                pressure: 200
            };
            30
        ],
        finished: false,
    };
    let bytes = encode(&msg).unwrap();
    assert!(
        bytes.len() < 140,
        "30-point batch was {} bytes",
        bytes.len()
    );
}

#[test]
fn stroke_batch_worst_case_size_budget() {
    let msg = ClientMsg::Stroke {
        stroke_id: u32::MAX,
        origin: (u16::MAX, u16::MAX),
        points: vec![
            Point {
                dx: 127,
                dy: -128,
                dt: 255,
                pressure: 255
            };
            MAX_POINTS_PER_BATCH
        ],
        finished: true,
    };
    let bytes = encode(&msg).unwrap();
    assert!(
        bytes.len() < 300,
        "{}-point worst-case batch was {} bytes",
        MAX_POINTS_PER_BATCH,
        bytes.len()
    );
}

#[test]
fn chat_minimum_size_budget() {
    let msg = ClientMsg::Chat { text: "hi".into() };
    let bytes = encode(&msg).unwrap();
    assert!(bytes.len() < 8, "chat 'hi' was {} bytes", bytes.len());
}
