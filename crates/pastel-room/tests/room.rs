use pastel_proto::*;
use pastel_room::{spawn_room, JoinError, JoinResult, RoomHandle, MAX_PLAYERS_PER_ROOM};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Receiver as BroadcastRx;
use tokio::sync::mpsc::Receiver as UnicastRx;

const ROOM: &str = "ABC234";
const RECV_TIMEOUT: Duration = Duration::from_millis(200);

fn code() -> RoomCode {
    RoomCode::parse(ROOM).unwrap()
}

fn hello(name: &str) -> Hello {
    Hello {
        room: code(),
        name: name.into(),
        resume_from: None,
    }
}

async fn join(handle: &RoomHandle, name: &str) -> JoinResult {
    handle.join(hello(name)).await.unwrap()
}

async fn next(rx: &mut BroadcastRx<Arc<ServerMsg>>) -> Arc<ServerMsg> {
    tokio::time::timeout(RECV_TIMEOUT, rx.recv())
        .await
        .expect("broadcast recv timed out")
        .expect("broadcast channel closed unexpectedly")
}

async fn next_unicast(rx: &mut UnicastRx<Arc<ServerMsg>>) -> Arc<ServerMsg> {
    tokio::time::timeout(RECV_TIMEOUT, rx.recv())
        .await
        .expect("unicast recv timed out")
        .expect("unicast channel closed unexpectedly")
}

async fn expect_no_message(rx: &mut BroadcastRx<Arc<ServerMsg>>) {
    let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(result.is_err(), "expected no message but got one");
}

#[tokio::test]
async fn join_returns_welcome_with_empty_snapshot() {
    let h = spawn_room(code());
    let mut joined = join(&h, "alice").await;

    let welcome = next_unicast(&mut joined.unicast_rx).await;
    match welcome.as_ref() {
        ServerMsg::Welcome { you, snapshot, .. } => {
            assert_eq!(*you, joined.you);
            assert!(
                snapshot.players.is_empty(),
                "first joiner sees empty player list"
            );
            assert!(snapshot.completed.is_empty());
        }
        other => panic!("expected Welcome, got {other:?}"),
    }
}

#[tokio::test]
async fn second_joiner_sees_presence_for_first() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let _welcome_a = next_unicast(&mut a.unicast_rx).await;

    // alice should see her own join broadcast.
    match next(&mut a.broadcast_rx).await.as_ref() {
        ServerMsg::Presence { joined, left, .. } => {
            assert_eq!(joined.len(), 1);
            assert_eq!(joined[0].name, "alice");
            assert!(left.is_empty());
        }
        other => panic!("expected Presence, got {other:?}"),
    }

    let _b = join(&h, "bob").await;

    match next(&mut a.broadcast_rx).await.as_ref() {
        ServerMsg::Presence { joined, left, .. } => {
            assert_eq!(joined.len(), 1);
            assert_eq!(joined[0].name, "bob");
            assert!(left.is_empty());
        }
        other => panic!("expected Presence for bob, got {other:?}"),
    }
}

#[tokio::test]
async fn stroke_broadcasts_with_monotonic_seq() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let _ = next_unicast(&mut a.unicast_rx).await;
    let _ = next(&mut a.broadcast_rx).await; // presence

    h.send(
        a.you,
        ClientMsg::Stroke {
            stroke_id: 1,
            origin: (10, 20),
            color: 0xd62828,
            width: 4,
            points: vec![Point {
                dx: 1,
                dy: 1,
                dt: 16,
                pressure: 200,
            }],
            finished: false,
        },
    )
    .await;

    match next(&mut a.broadcast_rx).await.as_ref() {
        ServerMsg::Stroke {
            seq,
            player,
            stroke_id,
            color,
            width,
            points,
            finished,
            ..
        } => {
            assert_eq!(*player, a.you);
            assert_eq!(*stroke_id, 1);
            assert_eq!(*color, 0xd62828);
            assert_eq!(*width, 4);
            assert_eq!(points.len(), 1);
            assert!(!finished);
            assert!(*seq >= 2, "seq should advance after presence");
        }
        other => panic!("expected Stroke, got {other:?}"),
    }
}

#[tokio::test]
async fn chat_is_broadcast_to_all() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let mut b = join(&h, "bob").await;
    let _ = next_unicast(&mut a.unicast_rx).await;
    let _ = next_unicast(&mut b.unicast_rx).await;
    drain_presence(&mut a.broadcast_rx, 2).await;
    drain_presence(&mut b.broadcast_rx, 1).await;

    h.send(a.you, ClientMsg::Chat { text: "hi".into() }).await;

    for rx in [&mut a.broadcast_rx, &mut b.broadcast_rx] {
        match next(rx).await.as_ref() {
            ServerMsg::Chat { player, text, .. } => {
                assert_eq!(*player, a.you);
                assert_eq!(text, "hi");
            }
            other => panic!("expected Chat, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn correct_guess_broadcasts_guess_event() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await; // drawer
    let mut b = join(&h, "bob").await; // guesser
    let _ = next_unicast(&mut a.unicast_rx).await;
    let _ = next_unicast(&mut b.unicast_rx).await;
    drain_presence(&mut a.broadcast_rx, 2).await;
    drain_presence(&mut b.broadcast_rx, 1).await;

    h.set_secret(a.you, "apple").await;

    h.send(
        b.you,
        ClientMsg::Guess {
            text: "Apple".into(),
        },
    )
    .await;

    for rx in [&mut a.broadcast_rx, &mut b.broadcast_rx] {
        match next(rx).await.as_ref() {
            ServerMsg::Guess { player, kind, .. } => {
                assert_eq!(*player, b.you);
                assert_eq!(*kind, GuessKind::Correct);
            }
            other => panic!("expected Guess, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn wrong_guess_falls_through_to_chat() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let mut b = join(&h, "bob").await;
    let _ = next_unicast(&mut a.unicast_rx).await;
    let _ = next_unicast(&mut b.unicast_rx).await;
    drain_presence(&mut a.broadcast_rx, 2).await;
    drain_presence(&mut b.broadcast_rx, 1).await;

    h.set_secret(a.you, "apple").await;
    h.send(
        b.you,
        ClientMsg::Guess {
            text: "banana".into(),
        },
    )
    .await;

    match next(&mut a.broadcast_rx).await.as_ref() {
        ServerMsg::Chat { player, text, .. } => {
            assert_eq!(*player, b.you);
            assert_eq!(text, "banana");
        }
        other => panic!("expected Chat for wrong guess, got {other:?}"),
    }
}

#[tokio::test]
async fn drawer_guess_is_ignored() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let _ = next_unicast(&mut a.unicast_rx).await;
    drain_presence(&mut a.broadcast_rx, 1).await;

    h.set_secret(a.you, "apple").await;
    h.send(
        a.you,
        ClientMsg::Guess {
            text: "apple".into(),
        },
    )
    .await;

    expect_no_message(&mut a.broadcast_rx).await;
}

#[tokio::test]
async fn eleventh_join_is_rejected() {
    let h = spawn_room(code());
    let mut held = Vec::new();
    for i in 0..MAX_PLAYERS_PER_ROOM {
        held.push(join(&h, &format!("p{i}")).await);
    }
    match h.join(hello("overflow")).await {
        Err(JoinError::RoomFull) => {}
        Err(other) => panic!("expected RoomFull, got {other:?}"),
        Ok(_) => panic!("expected RoomFull, got Ok"),
    }
    drop(held);
}

#[tokio::test]
async fn leave_emits_presence() {
    let h = spawn_room(code());
    let mut a = join(&h, "alice").await;
    let b = join(&h, "bob").await;
    let _ = next_unicast(&mut a.unicast_rx).await;
    drain_presence(&mut a.broadcast_rx, 2).await;

    h.leave(b.you).await;

    match next(&mut a.broadcast_rx).await.as_ref() {
        ServerMsg::Presence { joined, left, .. } => {
            assert!(joined.is_empty());
            assert_eq!(left, &vec![b.you]);
        }
        other => panic!("expected Presence(left), got {other:?}"),
    }
}

#[tokio::test]
async fn ten_players_thousand_strokes_arrive_ordered() {
    let h = spawn_room(code());
    let mut players = Vec::with_capacity(10);
    for i in 0..10 {
        let mut j = join(&h, &format!("p{i}")).await;
        let _ = next_unicast(&mut j.unicast_rx).await;
        players.push(j);
    }
    // Each player subscribes during their own join, so they see one presence
    // for themselves plus one for every player that joins after them.
    let total = players.len();
    for (i, p) in players.iter_mut().enumerate() {
        drain_presence(&mut p.broadcast_rx, total - i).await;
    }

    // Each player sends 100 strokes for a total of 1000.
    for p in &players {
        for s in 0..100u32 {
            h.send(
                p.you,
                ClientMsg::Stroke {
                    stroke_id: s,
                    origin: (0, 0),
                    color: 0,
                    width: 4,
                    points: vec![Point {
                        dx: 0,
                        dy: 0,
                        dt: 0,
                        pressure: 0,
                    }],
                    finished: false,
                },
            )
            .await;
        }
    }

    for p in &mut players {
        let mut last_seq: Seq = 0;
        let mut received = 0;
        while received < 1000 {
            match next(&mut p.broadcast_rx).await.as_ref() {
                ServerMsg::Stroke { seq, .. } => {
                    assert!(*seq > last_seq, "seq went backwards: {last_seq} -> {seq}");
                    last_seq = *seq;
                    received += 1;
                }
                other => panic!("unexpected msg in stroke phase: {other:?}"),
            }
        }
    }
}

async fn drain_presence(rx: &mut BroadcastRx<Arc<ServerMsg>>, count: usize) {
    for _ in 0..count {
        match next(rx).await.as_ref() {
            ServerMsg::Presence { .. } => {}
            other => panic!("expected Presence, got {other:?}"),
        }
    }
}
