//! Wire-level size caps. Enforced by `decode_validated` and by senders.

pub const ROOM_CODE_LEN: usize = 6;
pub const MAX_PLAYERS_PER_ROOM: usize = 10;

pub const MAX_NAME_LEN: usize = 32;
pub const MAX_CHAT_LEN: usize = 256;
pub const MAX_GUESS_LEN: usize = 64;
pub const MAX_WORD_LEN: usize = 64;

pub const MAX_POINTS_PER_BATCH: usize = 64;
pub const MAX_STROKES_PER_SNAPSHOT: usize = 1024;
pub const MAX_RESUME_EVENTS: usize = 1024;
pub const MAX_LK_TOKEN_LEN: usize = 1024;

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
