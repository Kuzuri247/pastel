use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use pastel_proto::*;
use pastel_room::{JoinOutcome, RoomHandle};
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
enum BotDifficulty {
    Easy,
    Medium,
    Hard,
}

impl BotDifficulty {
    fn from_str(s: &str) -> Self {
        match s {
            "easy" => Self::Easy,
            "hard" => Self::Hard,
            _ => Self::Medium,
        }
    }
    fn guess_delay_secs(&self) -> (f64, f64) {
        match self {
            Self::Easy => (25.0, 50.0),
            Self::Medium => (10.0, 25.0),
            Self::Hard => (3.0, 10.0),
        }
    }
    fn wrong_guesses_before(&self) -> u32 {
        match self {
            Self::Easy => rand::thread_rng().gen_range(2..=4),
            Self::Medium => rand::thread_rng().gen_range(1..=2),
            Self::Hard => 0,
        }
    }
    fn label(&self) -> &'static str {
        match self {
            Self::Easy => "chill",
            Self::Medium => "normal",
            Self::Hard => "sweaty",
        }
    }
}

#[derive(Deserialize)]
pub struct BotQuery {
    #[serde(default)]
    difficulty: Option<String>,
}

struct Drawing {
    strokes: Vec<Vec<(u8, u8)>>,
}

fn load_drawings() -> HashMap<String, Drawing> {
    let data = include_bytes!("../../pastel-loadtest/data/drawings.bin");
    let mut pos = 0usize;
    let count = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    let mut map = HashMap::new();
    for _ in 0..count {
        let wlen = data[pos] as usize;
        pos += 1;
        let word = String::from_utf8_lossy(&data[pos..pos + wlen]).to_string();
        pos += wlen;
        let stroke_count = data[pos] as usize;
        pos += 1;
        let mut strokes = Vec::with_capacity(stroke_count);
        for _ in 0..stroke_count {
            let pt_count = u16::from_le_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            let mut pts = Vec::with_capacity(pt_count);
            for _ in 0..pt_count {
                pts.push((data[pos], data[pos + 1]));
                pos += 2;
            }
            strokes.push(pts);
        }
        map.insert(word, Drawing { strokes });
    }
    map
}

static BOT_NAMES: &[&str] = &[
    "Doodlebot",
    "SketchBuddy",
    "InkyPal",
    "ScribbleFriend",
    "PastelPal",
    "DrawBot",
    "ArtBot",
    "PencilPal",
];

static GREETINGS: &[&str] = &[
    "hey everyone!",
    "hiiii",
    "ready to play!",
    "lets gooo",
    "hello hello",
];

static REACT_CORRECT: &[&str] = &[
    "nice one!",
    "gg",
    "wow fast",
    "how??",
    "big brain",
    "too easy",
];

static REACT_ROUND_END: &[&str] = &[
    "ohhh",
    "i see it now",
    "that was tough",
    "should have got that",
    "lol",
    "interesting",
];

static REACT_MY_TURN: &[&str] = &[
    "my turn!",
    "ok here goes",
    "watch this",
    "i got this",
    "easy one",
];

fn random_from(list: &[&str]) -> String {
    list[rand::thread_rng().gen_range(0..list.len())].to_string()
}

async fn bot_chat(room: &RoomHandle, my_id: PlayerId, text: String) {
    let delay = rand::thread_rng().gen_range(300..1200);
    tokio::time::sleep(Duration::from_millis(delay)).await;
    room.send(my_id, ClientMsg::Chat { text }).await;
}

pub async fn add_bot(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Query(query): Query<BotQuery>,
) -> impl IntoResponse {
    let room_code = match RoomCode::parse(&code) {
        Ok(c) => c,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("bad room code: {e}"));
        }
    };
    let diff = BotDifficulty::from_str(query.difficulty.as_deref().unwrap_or("medium"));
    let handle = state.rooms.get_or_create(room_code);
    let name = BOT_NAMES[rand::thread_rng().gen_range(0..BOT_NAMES.len())].to_string();
    let label = diff.label();
    let resp = format!("{name} joined ({label})");
    tokio::spawn(async move {
        if let Err(e) = run_bot(handle, room_code, name, diff).await {
            tracing::debug!("bot exited: {e}");
        }
    });
    (StatusCode::OK, resp)
}

async fn run_bot(
    room: RoomHandle,
    code: RoomCode,
    name: String,
    diff: BotDifficulty,
) -> anyhow::Result<()> {
    let drawings = load_drawings();
    let bot_words: Vec<String> = drawings.keys().cloned().collect();

    let hello = Hello {
        room: code,
        name,
        resume_from: None,
        client_token: None,
        avatar: Avatar {
            skin: rand::thread_rng().gen_range(0..=6),
            hat: 0,
            hair: rand::thread_rng().gen_range(0..=7),
            eyes: rand::thread_rng().gen_range(0..=7),
            mouth: rand::thread_rng().gen_range(0..=6),
            specs: 0,
            earrings: 0,
        },
    };

    let outcome = room.join(hello).await?;
    let join = match outcome {
        JoinOutcome::Joined(j) => j,
        JoinOutcome::Pending { .. } => anyhow::bail!("bot got pending"),
    };

    let my_id = join.you;
    let mut broadcast_rx = join.broadcast_rx;
    let mut unicast_rx = join.unicast_rx;

    let mut is_drawer = false;
    let mut guess_candidates: Vec<String> = Vec::new();
    let mut round_deadline: Option<tokio::time::Instant> = None;
    let mut guess_sent = false;

    // Drain welcome, then greet
    let _ = unicast_rx.recv().await;
    bot_chat(&room, my_id, random_from(GREETINGS)).await;

    loop {
        tokio::select! {
            biased;

            uc = unicast_rx.recv() => {
                let Some(msg) = uc else { break };
                match msg.as_ref() {
                    ServerMsg::WordOptions { words, .. } => {
                        let pick = words.iter().position(|w| {
                            drawings.contains_key(&w.to_lowercase())
                        }).unwrap_or(0);
                        let idx = pick.min(words.len().saturating_sub(1));
                        room.send(my_id, ClientMsg::Game(GameAction::PickWord(idx as u8))).await;
                    }
                    ServerMsg::DrawerWord { word, duration_ms } => {
                        is_drawer = true;
                        round_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(*duration_ms as u64));

                        bot_chat(&room, my_id, random_from(REACT_MY_TURN)).await;
                        let word_lower = word.to_lowercase();
                        if let Some(drawing) = drawings.get(&word_lower) {
                            replay_drawing(&room, my_id, &drawing.strokes).await;
                        }
                    }
                    ServerMsg::Bye { .. } => break,
                    _ => {}
                }
            }

            bc = broadcast_rx.recv() => {
                let Ok(msg) = bc else { break };
                match msg.as_ref() {
                    ServerMsg::Game { event: GameEvent::RoundStart { drawer, duration_ms, word_mask, .. }, .. } => {
                        is_drawer = *drawer == my_id;
                        guess_sent = false;
                        guess_candidates.clear();
                        if !is_drawer {
                            round_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(*duration_ms as u64));
                            let mask_len = word_mask.chars().filter(|c| *c != ' ').count();
                            let mut candidates: Vec<String> = bot_words.iter()
                                .filter(|w| w.len() == mask_len || w.chars().count() == mask_len)
                                .cloned()
                                .collect();
                            use rand::seq::SliceRandom;
                            candidates.shuffle(&mut rand::thread_rng());
                            guess_candidates = candidates;
                        }
                    }
                    ServerMsg::Game { event: GameEvent::HintReveal { mask }, .. } => {
                        if !is_drawer && !guess_sent {
                            let mask_chars: Vec<char> = mask.chars().collect();
                            guess_candidates.retain(|w| {
                                let wc: Vec<char> = w.chars().collect();
                                if wc.len() != mask_chars.len() { return false; }
                                for (mc, wch) in mask_chars.iter().zip(wc.iter()) {
                                    if *mc != '_' && *mc != ' ' && mc.to_lowercase().next() != wch.to_lowercase().next() {
                                        return false;
                                    }
                                }
                                true
                            });
                        }
                    }
                    ServerMsg::Game { event: GameEvent::WordPickStarted { drawer, .. }, .. } => {
                        is_drawer = *drawer == my_id;
                        guess_candidates.clear();
                        guess_sent = false;
                    }
                    ServerMsg::Game { event: GameEvent::RoundEnd { .. }, .. } => {
                        is_drawer = false;
                        guess_candidates.clear();
                        round_deadline = None;
                        guess_sent = false;
                        if rand::thread_rng().gen_bool(0.5) {
                            bot_chat(&room, my_id, random_from(REACT_ROUND_END)).await;
                        }
                    }
                    ServerMsg::Guess { player, kind: GuessKind::Correct, .. } if *player != my_id => {
                        if rand::thread_rng().gen_bool(0.4) {
                            bot_chat(&room, my_id, random_from(REACT_CORRECT)).await;
                        }
                    }
                    ServerMsg::Game { event: GameEvent::GameOver { .. }, .. } => {
                        // Stay in the room for rematch
                    }
                    _ => {}
                }
            }

            // Guessing timer
            _ = async {
                if is_drawer || guess_sent || guess_candidates.is_empty() || round_deadline.is_none() {
                    return std::future::pending::<()>().await;
                }
                let (lo, hi) = diff.guess_delay_secs();
                let wait_secs = rand::thread_rng().gen_range(lo..hi);
                tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
            } => {
                if !guess_candidates.is_empty() {
                    let wrong_first = diff.wrong_guesses_before() as usize;
                    let total = (wrong_first + 1).min(guess_candidates.len());
                    for i in 0..total {
                        let guess = guess_candidates[i].clone();
                        room.send(my_id, ClientMsg::Guess { text: guess }).await;
                        if i < total - 1 {
                            let delay = rand::thread_rng().gen_range(2000..5000);
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                    }
                    guess_sent = true;
                }
            }
        }
    }

    room.leave(my_id).await;
    Ok(())
}

async fn replay_drawing(room: &RoomHandle, my_id: PlayerId, strokes: &[Vec<(u8, u8)>]) {
    let scale_x = 960.0 / 256.0;
    let scale_y = 600.0 / 256.0;

    for (sid, stroke) in strokes.iter().enumerate() {
        if stroke.is_empty() {
            continue;
        }
        let origin_x = (stroke[0].0 as f32 * scale_x) as u16;
        let origin_y = (stroke[0].1 as f32 * scale_y) as u16;

        let mut points: Vec<Point> = Vec::new();
        let mut prev = stroke[0];
        for &(x, y) in &stroke[1..] {
            let dx = (x as i16 - prev.0 as i16).clamp(-128, 127) as i8;
            let dy = (y as i16 - prev.1 as i16).clamp(-128, 127) as i8;
            points.push(Point {
                dx,
                dy,
                dt: 16,
                pressure: 200,
            });
            prev = (x, y);

            if points.len() >= 60 {
                let msg = ClientMsg::Stroke {
                    stroke_id: sid as u32,
                    origin: (origin_x, origin_y),
                    color: 0x2a2a2e,
                    width: 4,
                    points: std::mem::take(&mut points),
                    finished: false,
                };
                room.send(my_id, msg).await;
                let batch_pause = rand::thread_rng().gen_range(60..150);
                tokio::time::sleep(Duration::from_millis(batch_pause)).await;
            }
        }

        let msg = ClientMsg::Stroke {
            stroke_id: sid as u32,
            origin: (origin_x, origin_y),
            color: 0x2a2a2e,
            width: 4,
            points,
            finished: true,
        };
        room.send(my_id, msg).await;
        let stroke_pause = rand::thread_rng().gen_range(300..700);
        tokio::time::sleep(Duration::from_millis(stroke_pause)).await;
    }
}
