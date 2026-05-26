use crate::limits::*;
use crate::msg::{ClientMsg, ServerMsg};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    #[error("frame is {0} bytes, exceeds limit {MAX_FRAME_BYTES}")]
    FrameTooLarge(usize),
    #[error("{field} length {len} exceeds limit {max}")]
    FieldTooLong {
        field: &'static str,
        len: usize,
        max: usize,
    },
    #[error("resume events nested too deep")]
    ResumeTooDeep,
}

pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, CodecError> {
    Ok(postcard::to_allocvec(msg)?)
}

pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, CodecError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(CodecError::FrameTooLarge(bytes.len()));
    }
    Ok(postcard::from_bytes(bytes)?)
}

pub fn decode_client_validated(bytes: &[u8]) -> Result<ClientMsg, CodecError> {
    let msg: ClientMsg = decode(bytes)?;
    validate_client(&msg)?;
    Ok(msg)
}

pub fn decode_server_validated(bytes: &[u8]) -> Result<ServerMsg, CodecError> {
    let msg: ServerMsg = decode(bytes)?;
    validate_server(&msg, 0)?;
    Ok(msg)
}

pub fn validate_client(msg: &ClientMsg) -> Result<(), CodecError> {
    match msg {
        ClientMsg::Hello(h) => {
            check_len("hello.name", h.name.len(), MAX_NAME_LEN)?;
        }
        ClientMsg::Stroke { points, .. } => {
            check_len("stroke.points", points.len(), MAX_POINTS_PER_BATCH)?;
        }
        ClientMsg::Chat { text } => {
            check_len("chat.text", text.len(), MAX_CHAT_LEN)?;
        }
        ClientMsg::Guess { text } => {
            check_len("guess.text", text.len(), MAX_GUESS_LEN)?;
        }
        ClientMsg::Game(_) | ClientMsg::Pong { .. } => {}
    }
    Ok(())
}

const MAX_RESUME_NESTING: u8 = 1;

pub fn validate_server(msg: &ServerMsg, depth: u8) -> Result<(), CodecError> {
    match msg {
        ServerMsg::Welcome {
            snapshot, lk_token, ..
        } => {
            check_len(
                "welcome.snapshot.completed",
                snapshot.completed.len(),
                MAX_STROKES_PER_SNAPSHOT,
            )?;
            check_len(
                "welcome.snapshot.players",
                snapshot.players.len(),
                MAX_PLAYERS_PER_ROOM,
            )?;
            check_len("welcome.lk_token", lk_token.len(), MAX_LK_TOKEN_LEN)?;
            for s in &snapshot.completed {
                check_len(
                    "completed_stroke.points",
                    s.points.len(),
                    MAX_POINTS_PER_BATCH * 16,
                )?;
            }
        }
        ServerMsg::Resume { events } => {
            if depth >= MAX_RESUME_NESTING {
                return Err(CodecError::ResumeTooDeep);
            }
            check_len("resume.events", events.len(), MAX_RESUME_EVENTS)?;
            for e in events {
                validate_server(e, depth + 1)?;
            }
        }
        ServerMsg::Stroke { points, .. } => {
            check_len("stroke.points", points.len(), MAX_POINTS_PER_BATCH)?;
        }
        ServerMsg::Chat { text, .. } => {
            check_len("chat.text", text.len(), MAX_CHAT_LEN)?;
        }
        ServerMsg::Presence { joined, left, .. } => {
            check_len("presence.joined", joined.len(), MAX_PLAYERS_PER_ROOM)?;
            check_len("presence.left", left.len(), MAX_PLAYERS_PER_ROOM)?;
        }
        ServerMsg::Game { event, .. } => match event {
            crate::msg::GameEvent::RoundStart { word_mask, .. } => {
                check_len("round_start.word_mask", word_mask.len(), MAX_WORD_LEN)?;
            }
            crate::msg::GameEvent::RoundEnd { word, scores } => {
                check_len("round_end.word", word.len(), MAX_WORD_LEN)?;
                check_len("round_end.scores", scores.len(), MAX_PLAYERS_PER_ROOM)?;
            }
            crate::msg::GameEvent::GameOver { final_scores } => {
                check_len(
                    "game_over.final_scores",
                    final_scores.len(),
                    MAX_PLAYERS_PER_ROOM,
                )?;
            }
            crate::msg::GameEvent::Cleared { .. } => {}
        },
        ServerMsg::Guess { .. } | ServerMsg::Ping { .. } | ServerMsg::Bye { .. } => {}
    }
    Ok(())
}

fn check_len(field: &'static str, len: usize, max: usize) -> Result<(), CodecError> {
    if len > max {
        Err(CodecError::FieldTooLong { field, len, max })
    } else {
        Ok(())
    }
}
