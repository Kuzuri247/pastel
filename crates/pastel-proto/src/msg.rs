use crate::types::{Player, PlayerId, Point, RoomCode, RoomSnapshot, Seq};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Hello {
    pub room: RoomCode,
    pub name: String,
    pub resume_from: Option<Seq>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GameAction {
    Start,
    PickWord(u8),
    Kick(PlayerId),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GameEvent {
    RoundStart {
        drawer: PlayerId,
        word_mask: String,
        duration_ms: u32,
    },
    RoundEnd {
        word: String,
        scores: Vec<(PlayerId, u32)>,
    },
    GameOver {
        final_scores: Vec<(PlayerId, u32)>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GuessKind {
    Correct,
    Close,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ByeReason {
    Reconnect,
    Kicked,
    RoomClosed,
    RoomFull,
    BadFrame,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClientMsg {
    Hello(Hello),
    Stroke {
        stroke_id: u32,
        origin: (u16, u16),
        points: Vec<Point>,
        finished: bool,
    },
    Chat {
        text: String,
    },
    Guess {
        text: String,
    },
    Game(GameAction),
    Pong {
        nonce: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ServerMsg {
    Welcome {
        you: PlayerId,
        snapshot: RoomSnapshot,
        seq: Seq,
        lk_token: String,
    },
    Resume {
        events: Vec<ServerMsg>,
    },
    Stroke {
        seq: Seq,
        player: PlayerId,
        stroke_id: u32,
        origin: (u16, u16),
        points: Vec<Point>,
        finished: bool,
    },
    Chat {
        seq: Seq,
        player: PlayerId,
        text: String,
    },
    Guess {
        seq: Seq,
        player: PlayerId,
        kind: GuessKind,
    },
    Presence {
        seq: Seq,
        joined: Vec<Player>,
        left: Vec<PlayerId>,
    },
    Game {
        seq: Seq,
        event: GameEvent,
    },
    Ping {
        nonce: u32,
    },
    Bye {
        reason: ByeReason,
    },
}
