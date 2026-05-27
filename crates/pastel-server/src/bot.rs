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
    "lil crayon",
    "soft pencil",
    "chalky",
    "lil pastel",
    "smudge",
    "doodlebug",
    "inkbean",
    "brushie",
    "sketchy",
    "pigment",
    "blotch",
    "scribbles",
    "tintsy",
    "hue",
    "palette",
    "swatch",
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

fn load_all_game_words() -> Vec<String> {
    let easy = include_str!("../data/words-easy.txt");
    let medium = include_str!("../data/words-medium.txt");
    let hard = include_str!("../data/words-hard.txt");
    let bot = include_str!("../data/words-bot.txt");
    let mut words: Vec<String> = easy
        .lines()
        .chain(medium.lines())
        .chain(hard.lines())
        .chain(bot.lines())
        .filter(|l| !l.is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    words.sort();
    words.dedup();
    words
}

async fn run_bot(
    room: RoomHandle,
    code: RoomCode,
    name: String,
    diff: BotDifficulty,
) -> anyhow::Result<()> {
    let drawings = load_drawings();
    let all_words = load_all_game_words();

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
    let mut guess_index: usize = 0;
    let mut guess_sent = false;
    let mut next_guess_at: Option<tokio::time::Instant> = None;

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
                    ServerMsg::DrawerWord { word, .. } => {
                        is_drawer = true;

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
                    ServerMsg::Game { event: GameEvent::RoundStart { drawer, word_mask, .. }, .. } => {
                        is_drawer = *drawer == my_id;
                        guess_sent = false;
                        guess_index = 0;
                        guess_candidates.clear();
                        next_guess_at = None;
                        if !is_drawer {
                            let mask_len = word_mask.chars().filter(|c| *c != ' ').count();
                            let mut candidates: Vec<String> = all_words.iter()
                                .filter(|w| w.len() == mask_len || w.chars().count() == mask_len)
                                .cloned()
                                .collect();
                            use rand::seq::SliceRandom;
                            candidates.shuffle(&mut rand::thread_rng());
                            guess_candidates = candidates;
                            let (lo, hi) = diff.guess_delay_secs();
                            let first_wait = rand::thread_rng().gen_range(lo..hi);
                            next_guess_at = Some(tokio::time::Instant::now() + Duration::from_secs_f64(first_wait));
                        }
                    }
                    ServerMsg::Guess { player, kind: GuessKind::Correct, .. } if *player == my_id => {
                        guess_sent = true;
                        next_guess_at = None;
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
                        guess_index = 0;
                        guess_sent = false;
                        next_guess_at = None;
                    }
                    ServerMsg::Game { event: GameEvent::RoundEnd { .. }, .. } => {
                        is_drawer = false;
                        guess_candidates.clear();
                        guess_index = 0;
                        guess_sent = false;
                        next_guess_at = None;
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

            // Guessing: one candidate per tick, wait for result before next
            _ = async {
                match next_guess_at {
                    Some(t) if !is_drawer && !guess_sent && guess_index < guess_candidates.len() => {
                        tokio::time::sleep_until(t).await;
                    }
                    _ => std::future::pending::<()>().await,
                }
            } => {
                if guess_index < guess_candidates.len() {
                    let guess = guess_candidates[guess_index].clone();
                    guess_index += 1;
                    room.send(my_id, ClientMsg::Guess { text: guess }).await;
                    // Pause grows slightly each attempt so it doesn't feel robotic
                    let attempt = guess_index as u64;
                    let (base_lo, base_hi) = match diff {
                        BotDifficulty::Easy => (5000u64, 9000u64),
                        BotDifficulty::Medium => (3000, 5000),
                        BotDifficulty::Hard => (1200, 2500),
                    };
                    let jitter = rand::thread_rng().gen_range(base_lo..base_hi);
                    let slowdown = (attempt * 300).min(3000);
                    next_guess_at = Some(
                        tokio::time::Instant::now() + Duration::from_millis(jitter + slowdown),
                    );
                } else {
                    // Exhausted all candidates, stop
                    guess_sent = true;
                    next_guess_at = None;
                }
            }
        }
    }

    room.leave(my_id).await;
    Ok(())
}

async fn replay_drawing(room: &RoomHandle, my_id: PlayerId, strokes: &[Vec<(u8, u8)>]) {
    let pad_x = 80.0_f32;
    let pad_y = 50.0_f32;
    let usable_w = 960.0 - pad_x * 2.0;
    let usable_h = 600.0 - pad_y * 2.0;

    let scale = |rx: u8, ry: u8| -> (i32, i32) {
        (
            (pad_x + (rx as f32 / 255.0) * usable_w).round() as i32,
            (pad_y + (ry as f32 / 255.0) * usable_h).round() as i32,
        )
    };

    for (sid, stroke) in strokes.iter().enumerate() {
        if stroke.is_empty() {
            continue;
        }

        let (ox, oy) = scale(stroke[0].0, stroke[0].1);
        let origin_x = ox.clamp(0, 960) as u16;
        let origin_y = oy.clamp(0, 600) as u16;

        let mut points: Vec<Point> = Vec::new();
        let mut cur_x = ox;
        let mut cur_y = oy;

        for &(rx, ry) in &stroke[1..] {
            let (tx, ty) = scale(rx, ry);
            let mut rem_x = tx - cur_x;
            let mut rem_y = ty - cur_y;

            while rem_x != 0 || rem_y != 0 {
                let dx = rem_x.clamp(-120, 120) as i8;
                let dy = rem_y.clamp(-120, 120) as i8;
                points.push(Point {
                    dx,
                    dy,
                    dt: 16,
                    pressure: 200,
                });
                cur_x += dx as i32;
                cur_y += dy as i32;
                rem_x = tx - cur_x;
                rem_y = ty - cur_y;
            }

            if points.len() >= 50 {
                let msg = ClientMsg::Stroke {
                    stroke_id: sid as u32,
                    origin: (origin_x, origin_y),
                    color: 0x2a2a2e,
                    width: 3,
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
            width: 3,
            points,
            finished: true,
        };
        room.send(my_id, msg).await;
        let stroke_pause = rand::thread_rng().gen_range(300..700);
        tokio::time::sleep(Duration::from_millis(stroke_pause)).await;
    }
}
