use crate::bucket::TokenBucket;
use crate::game::{
    build_mask, drawer_bonus, guess_score, max_hints, pick_hint_index, ranked_scores, reveal_at,
    DRAW_WINDOW, HINT_REMAINING_SECS, PICK_WINDOW, ROUND_REVEAL,
};
use crate::words::{Difficulty, SharedWords};
use crate::{
    BROADCAST_CAPACITY, CHAT_RING, COMMAND_INBOX_CAPACITY, COMPLETED_STROKES_RING, UNICAST_CAPACITY,
};
use ahash::AHashMap;
use pastel_proto::*;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;

const CHAT_BUCKET_CAPACITY: f32 = 5.0;
const CHAT_REFILL_PER_SEC: f32 = 5.0 / 3.0;
const GUESS_BUCKET_CAPACITY: f32 = 10.0;
const GUESS_REFILL_PER_SEC: f32 = 10.0 / 3.0;

pub enum RoomCmd {
    Join {
        hello: Hello,
        reply: oneshot::Sender<Result<JoinResult, JoinError>>,
    },
    Leave {
        player: PlayerId,
    },
    FromClient {
        player: PlayerId,
        msg: ClientMsg,
    },
    /// Test-only: inject a Drawing phase with the given drawer + word so guess
    /// tests can run without driving a full game start.
    SetSecret {
        drawer: PlayerId,
        word: String,
    },
}

pub struct JoinResult {
    pub you: PlayerId,
    pub unicast_rx: mpsc::Receiver<Arc<ServerMsg>>,
    pub broadcast_rx: broadcast::Receiver<Arc<ServerMsg>>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JoinError {
    #[error("room is full")]
    RoomFull,
    #[error("room is closed")]
    RoomClosed,
}

#[derive(Clone)]
pub struct RoomHandle {
    cmd_tx: mpsc::Sender<RoomCmd>,
}

impl RoomHandle {
    pub async fn join(&self, hello: Hello) -> Result<JoinResult, JoinError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(RoomCmd::Join { hello, reply: tx })
            .await
            .map_err(|_| JoinError::RoomClosed)?;
        rx.await.map_err(|_| JoinError::RoomClosed)?
    }

    pub async fn send(&self, player: PlayerId, msg: ClientMsg) {
        let _ = self.cmd_tx.send(RoomCmd::FromClient { player, msg }).await;
    }

    pub async fn leave(&self, player: PlayerId) {
        let _ = self.cmd_tx.send(RoomCmd::Leave { player }).await;
    }

    pub async fn set_secret(&self, drawer: PlayerId, word: impl Into<String>) {
        let _ = self
            .cmd_tx
            .send(RoomCmd::SetSecret {
                drawer,
                word: word.into(),
            })
            .await;
    }
}

pub fn spawn_room(code: RoomCode, words: SharedWords) -> RoomHandle {
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_INBOX_CAPACITY);
    let (bc_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
    let room = Room::new(code, bc_tx, words);
    tokio::spawn(room.run(cmd_rx));
    RoomHandle { cmd_tx }
}

struct PlayerSlot {
    name: String,
    unicast_tx: mpsc::Sender<Arc<ServerMsg>>,
    chat_bucket: TokenBucket,
    guess_bucket: TokenBucket,
}

#[derive(Default)]
struct InProgressStroke {
    origin: (u16, u16),
    color: u32,
    width: u8,
    points: Vec<Point>,
}

enum GamePhase {
    Lobby,
    ChoosingWord {
        drawer: PlayerId,
        options: Vec<String>,
        deadline: Instant,
        round_index: u8,
    },
    Drawing {
        drawer: PlayerId,
        word: String,
        mask: String,
        deadline: Instant,
        started_at: Instant,
        round_index: u8,
        revealed_indices: HashSet<usize>,
        scores_this_round: AHashMap<PlayerId, u32>,
        correct_guessers: Vec<PlayerId>,
        hint_schedule: Vec<Instant>,
    },
    RoundEnd {
        #[allow(dead_code)]
        word: String,
        deadline: Instant,
        round_index: u8,
    },
    GameOver,
}

struct GameState {
    mode: GameMode,
    rounds: u8,
    rotation: Vec<PlayerId>,
    scores: AHashMap<PlayerId, u32>,
    phase: GamePhase,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            mode: GameMode::Standard,
            rounds: GameMode::Standard.rounds(),
            rotation: Vec::new(),
            scores: AHashMap::new(),
            phase: GamePhase::Lobby,
        }
    }
}

enum DeadlineAction {
    None,
    AutoPickWord,
    RevealHint,
    EndRoundTimedOut,
    NextRoundOrFinish,
}

struct Room {
    #[allow(dead_code)]
    code: RoomCode,
    seq: Seq,
    next_player_id: PlayerId,
    players: AHashMap<PlayerId, PlayerSlot>,
    completed: VecDeque<CompletedStroke>,
    in_progress: AHashMap<(PlayerId, u32), InProgressStroke>,
    chat: VecDeque<(Seq, PlayerId, String)>,
    game: GameState,
    words: SharedWords,
    broadcast_tx: broadcast::Sender<Arc<ServerMsg>>,
}

impl Room {
    fn new(
        code: RoomCode,
        broadcast_tx: broadcast::Sender<Arc<ServerMsg>>,
        words: SharedWords,
    ) -> Self {
        Self {
            code,
            seq: 0,
            next_player_id: 1,
            players: AHashMap::new(),
            completed: VecDeque::with_capacity(COMPLETED_STROKES_RING),
            in_progress: AHashMap::new(),
            chat: VecDeque::with_capacity(CHAT_RING),
            game: GameState::default(),
            words,
            broadcast_tx,
        }
    }

    async fn run(mut self, mut inbox: mpsc::Receiver<RoomCmd>) {
        loop {
            let next = self.next_deadline();
            tokio::select! {
                biased;
                cmd = inbox.recv() => match cmd {
                    Some(cmd) => self.handle_cmd(cmd),
                    None => break,
                },
                _ = sleep_until_or_pending(next) => {
                    self.handle_deadline();
                }
            }
        }
    }

    fn handle_cmd(&mut self, cmd: RoomCmd) {
        match cmd {
            RoomCmd::Join { hello, reply } => self.handle_join(hello, reply),
            RoomCmd::Leave { player } => self.handle_leave(player),
            RoomCmd::FromClient { player, msg } => self.handle_client_msg(player, msg),
            RoomCmd::SetSecret { drawer, word } => self.handle_set_secret(drawer, word),
        }
    }

    fn next_seq(&mut self) -> Seq {
        self.seq += 1;
        self.seq
    }

    fn broadcast(&self, msg: ServerMsg) {
        let _ = self.broadcast_tx.send(Arc::new(msg));
    }

    fn unicast(&self, player: PlayerId, msg: ServerMsg) {
        if let Some(slot) = self.players.get(&player) {
            let _ = slot.unicast_tx.try_send(Arc::new(msg));
        }
    }

    fn snapshot(&self) -> RoomSnapshot {
        RoomSnapshot {
            players: self
                .players
                .iter()
                .map(|(id, slot)| Player {
                    id: *id,
                    name: slot.name.clone(),
                })
                .collect(),
            completed: self.completed.iter().cloned().collect(),
            seq: self.seq,
            chat: self
                .chat
                .iter()
                .map(|(seq, player, text)| ChatLine {
                    seq: *seq,
                    player: *player,
                    text: text.clone(),
                })
                .collect(),
        }
    }

    // ---- join / leave / presence -----------------------------------------

    fn handle_join(&mut self, hello: Hello, reply: oneshot::Sender<Result<JoinResult, JoinError>>) {
        if self.players.len() >= MAX_PLAYERS_PER_ROOM {
            let _ = reply.send(Err(JoinError::RoomFull));
            return;
        }

        let id = self.next_player_id;
        self.next_player_id = self.next_player_id.wrapping_add(1);

        let (uc_tx, uc_rx) = mpsc::channel(UNICAST_CAPACITY);
        let bc_rx = self.broadcast_tx.subscribe();

        let welcome = ServerMsg::Welcome {
            you: id,
            snapshot: self.snapshot(),
            seq: self.seq,
            lk_token: String::new(),
        };
        let _ = uc_tx.try_send(Arc::new(welcome));

        self.players.insert(
            id,
            PlayerSlot {
                name: hello.name.clone(),
                unicast_tx: uc_tx,
                chat_bucket: TokenBucket::new(CHAT_BUCKET_CAPACITY, CHAT_REFILL_PER_SEC),
                guess_bucket: TokenBucket::new(GUESS_BUCKET_CAPACITY, GUESS_REFILL_PER_SEC),
            },
        );

        let _ = reply.send(Ok(JoinResult {
            you: id,
            unicast_rx: uc_rx,
            broadcast_rx: bc_rx,
        }));

        let seq = self.next_seq();
        self.broadcast(ServerMsg::Presence {
            seq,
            joined: vec![Player {
                id,
                name: hello.name,
            }],
            left: vec![],
        });
    }

    fn handle_leave(&mut self, player: PlayerId) {
        if self.players.remove(&player).is_none() {
            return;
        }
        self.in_progress.retain(|(p, _), _| *p != player);

        // If the leaver was the current drawer, end the round early.
        let was_current_drawer = match &self.game.phase {
            GamePhase::ChoosingWord { drawer, .. } | GamePhase::Drawing { drawer, .. } => {
                *drawer == player
            }
            _ => false,
        };

        let seq = self.next_seq();
        self.broadcast(ServerMsg::Presence {
            seq,
            joined: vec![],
            left: vec![player],
        });

        if was_current_drawer {
            self.end_round_abort();
        }
    }

    // ---- client messages -------------------------------------------------

    fn handle_client_msg(&mut self, player: PlayerId, msg: ClientMsg) {
        if !self.players.contains_key(&player) {
            return;
        }
        match msg {
            ClientMsg::Stroke {
                stroke_id,
                origin,
                color,
                width,
                points,
                finished,
            } => self.handle_stroke(player, stroke_id, origin, color, width, points, finished),
            ClientMsg::Chat { text } => self.handle_chat(player, text),
            ClientMsg::Guess { text } => self.handle_guess(player, text),
            ClientMsg::Game(GameAction::Start { mode }) => self.handle_start_game(mode),
            ClientMsg::Game(GameAction::PickWord(idx)) => self.handle_pick_word(player, idx),
            ClientMsg::Game(GameAction::Clear) => self.handle_clear(player),
            ClientMsg::Game(GameAction::Kick(_)) | ClientMsg::Hello(_) | ClientMsg::Pong { .. } => {
                // Hello is connection setup; Kick is later phase work.
            }
        }
    }

    fn handle_clear(&mut self, player: PlayerId) {
        self.completed.clear();
        self.in_progress.clear();
        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::Cleared { by: player },
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_stroke(
        &mut self,
        player: PlayerId,
        stroke_id: u32,
        origin: (u16, u16),
        color: u32,
        width: u8,
        points: Vec<Point>,
        finished: bool,
    ) {
        // While a game is in progress, only the drawer's strokes are accepted.
        if matches!(self.game.phase, GamePhase::Drawing { drawer, .. } if drawer != player) {
            return;
        }
        if matches!(
            self.game.phase,
            GamePhase::ChoosingWord { .. } | GamePhase::RoundEnd { .. }
        ) {
            return;
        }

        let key = (player, stroke_id);
        let entry = self
            .in_progress
            .entry(key)
            .or_insert_with(|| InProgressStroke {
                origin,
                color,
                width,
                points: Vec::new(),
            });
        entry.points.extend_from_slice(&points);

        if finished {
            if let Some(done) = self.in_progress.remove(&key) {
                self.completed.push_back(CompletedStroke {
                    player,
                    stroke_id,
                    origin: done.origin,
                    color: done.color,
                    width: done.width,
                    points: done.points,
                });
                while self.completed.len() > COMPLETED_STROKES_RING {
                    self.completed.pop_front();
                }
            }
        }

        let seq = self.next_seq();
        self.broadcast(ServerMsg::Stroke {
            seq,
            player,
            stroke_id,
            origin,
            color,
            width,
            points,
            finished,
        });
    }

    fn handle_chat(&mut self, player: PlayerId, text: String) {
        let Some(slot) = self.players.get_mut(&player) else {
            return;
        };
        if !slot.chat_bucket.try_take() {
            return;
        }
        let seq = self.next_seq();
        self.chat.push_back((seq, player, text.clone()));
        while self.chat.len() > CHAT_RING {
            self.chat.pop_front();
        }
        self.broadcast(ServerMsg::Chat { seq, player, text });
    }

    fn handle_guess(&mut self, player: PlayerId, text: String) {
        let Some(slot) = self.players.get_mut(&player) else {
            return;
        };
        if !slot.guess_bucket.try_take() {
            return;
        }

        match self.evaluate_guess(player, &text) {
            GuessOutcome::Correct { everyone_guessed } => {
                let seq = self.next_seq();
                self.broadcast(ServerMsg::Guess {
                    seq,
                    player,
                    kind: GuessKind::Correct,
                });
                if everyone_guessed {
                    self.end_round_completed();
                }
            }
            GuessOutcome::Wrong | GuessOutcome::NotInGame => {
                let seq = self.next_seq();
                self.broadcast(ServerMsg::Chat { seq, player, text });
            }
            GuessOutcome::DrawerSelfGuess | GuessOutcome::AlreadyCorrect => {
                // Silently dropped; drawer can't spoil and re-guessers can't spam.
            }
        }
    }

    fn evaluate_guess(&mut self, player: PlayerId, text: &str) -> GuessOutcome {
        match &mut self.game.phase {
            GamePhase::Drawing {
                drawer,
                word,
                deadline,
                started_at,
                scores_this_round,
                correct_guessers,
                ..
            } => {
                if *drawer == player {
                    return GuessOutcome::DrawerSelfGuess;
                }
                if correct_guessers.contains(&player) {
                    return GuessOutcome::AlreadyCorrect;
                }
                if !text.trim().eq_ignore_ascii_case(word.trim()) {
                    return GuessOutcome::Wrong;
                }

                let now = Instant::now();
                let remaining_ms = deadline
                    .checked_duration_since(now)
                    .unwrap_or_default()
                    .as_millis() as u32;
                let window_ms = deadline
                    .checked_duration_since(*started_at)
                    .unwrap_or(DRAW_WINDOW)
                    .as_millis() as u32;
                let rank = correct_guessers.len();
                let score = guess_score(remaining_ms, window_ms, rank);

                scores_this_round.insert(player, score);
                correct_guessers.push(player);

                let guessers_remaining = self
                    .players
                    .keys()
                    .filter(|p| **p != *drawer)
                    .filter(|p| !correct_guessers.contains(p))
                    .count();

                GuessOutcome::Correct {
                    everyone_guessed: guessers_remaining == 0,
                }
            }
            _ => GuessOutcome::NotInGame,
        }
    }

    fn handle_set_secret(&mut self, drawer: PlayerId, word: String) {
        // Test-only fast path: jump into a Drawing phase with the given word.
        // No timers, no hint schedule; deadline is far in the future.
        let mask = build_mask(&word);
        let started_at = Instant::now();
        let deadline = started_at + Duration::from_secs(60 * 60);
        self.game.phase = GamePhase::Drawing {
            drawer,
            word,
            mask,
            deadline,
            started_at,
            round_index: 0,
            revealed_indices: HashSet::new(),
            scores_this_round: AHashMap::new(),
            correct_guessers: Vec::new(),
            hint_schedule: Vec::new(),
        };
    }

    // ---- game state machine ---------------------------------------------

    fn handle_start_game(&mut self, mode: GameMode) {
        if !matches!(self.game.phase, GamePhase::Lobby | GamePhase::GameOver) {
            return;
        }
        if self.players.len() < 2 {
            tracing::debug!("ignoring Start: need at least 2 players");
            return;
        }
        if self.words.is_empty() {
            tracing::warn!("ignoring Start: word lists are empty");
            return;
        }
        let mut rotation: Vec<PlayerId> = self.players.keys().copied().collect();
        rotation.sort();
        self.game = GameState {
            mode,
            rounds: mode.rounds(),
            rotation,
            scores: AHashMap::from_iter(self.players.keys().copied().map(|p| (p, 0u32))),
            phase: GamePhase::Lobby, // will be overwritten by start_choosing_round
        };
        self.start_choosing_round(0);
    }

    fn start_choosing_round(&mut self, round_index: u8) {
        // Find next alive drawer.
        let mut drawer = None;
        let total = self.game.rotation.len();
        if total == 0 {
            self.end_game();
            return;
        }
        for offset in 0..total {
            let candidate = self.game.rotation[(round_index as usize + offset) % total];
            if self.players.contains_key(&candidate) {
                drawer = Some(candidate);
                break;
            }
        }
        let Some(drawer) = drawer else {
            self.end_game();
            return;
        };

        let diff = Difficulty::for_round(round_index);
        let count = self.game.mode.word_options() as usize;
        let options = self.words.sample(diff, count);
        if options.is_empty() {
            tracing::warn!("no words available for difficulty {diff:?}; ending game");
            self.end_game();
            return;
        }

        let deadline = Instant::now() + PICK_WINDOW;
        self.game.phase = GamePhase::ChoosingWord {
            drawer,
            options: options.clone(),
            deadline,
            round_index,
        };

        let total_rounds = self.game.rounds;
        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::WordPickStarted {
                drawer,
                deadline_ms: PICK_WINDOW.as_millis() as u32,
                round_index,
                total_rounds,
            },
        });
        self.unicast(
            drawer,
            ServerMsg::WordOptions {
                words: options,
                deadline_ms: PICK_WINDOW.as_millis() as u32,
            },
        );
    }

    fn handle_pick_word(&mut self, player: PlayerId, idx: u8) {
        let (chosen_word, round_index) = match &self.game.phase {
            GamePhase::ChoosingWord {
                drawer,
                options,
                round_index,
                ..
            } if *drawer == player => {
                let i = (idx as usize).min(options.len().saturating_sub(1));
                if options.is_empty() {
                    return;
                }
                (options[i].clone(), *round_index)
            }
            _ => return,
        };
        self.start_drawing(player, chosen_word, round_index);
    }

    fn auto_pick_word(&mut self) {
        let (drawer, chosen_word, round_index) = match &self.game.phase {
            GamePhase::ChoosingWord {
                drawer,
                options,
                round_index,
                ..
            } if !options.is_empty() => (*drawer, options[0].clone(), *round_index),
            _ => return,
        };
        self.start_drawing(drawer, chosen_word, round_index);
    }

    fn start_drawing(&mut self, drawer: PlayerId, word: String, round_index: u8) {
        // Fresh canvas per round.
        self.completed.clear();
        self.in_progress.clear();
        let clear_seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq: clear_seq,
            event: GameEvent::Cleared { by: drawer },
        });

        let mask = build_mask(&word);
        let started_at = Instant::now();
        let deadline = started_at + DRAW_WINDOW;

        // Pre-reveal non-alpha positions (spaces, hyphens, digits).
        let mut revealed_indices = HashSet::new();
        for (i, c) in word.chars().enumerate() {
            if !c.is_alphabetic() {
                revealed_indices.insert(i);
            }
        }

        let hint_count = max_hints(&word);
        let hint_schedule: Vec<Instant> = HINT_REMAINING_SECS
            .iter()
            .take(hint_count)
            .map(|secs| deadline - Duration::from_secs(*secs))
            .collect();

        let total_rounds = self.game.rounds;
        self.game.phase = GamePhase::Drawing {
            drawer,
            word: word.clone(),
            mask: mask.clone(),
            deadline,
            started_at,
            round_index,
            revealed_indices,
            scores_this_round: AHashMap::new(),
            correct_guessers: Vec::new(),
            hint_schedule,
        };

        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::RoundStart {
                drawer,
                word_mask: mask,
                duration_ms: DRAW_WINDOW.as_millis() as u32,
                round_index,
                total_rounds,
            },
        });
        self.unicast(
            drawer,
            ServerMsg::DrawerWord {
                word,
                duration_ms: DRAW_WINDOW.as_millis() as u32,
            },
        );
    }

    fn reveal_one_hint(&mut self) {
        let GamePhase::Drawing {
            word,
            mask,
            revealed_indices,
            hint_schedule,
            ..
        } = &mut self.game.phase
        else {
            return;
        };
        if !hint_schedule.is_empty() {
            hint_schedule.remove(0);
        }
        let Some(idx) = pick_hint_index(word, revealed_indices) else {
            return;
        };
        if !reveal_at(mask, word, idx) {
            return;
        }
        revealed_indices.insert(idx);
        let mask_clone = mask.clone();
        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::HintReveal { mask: mask_clone },
        });
    }

    fn end_round_completed(&mut self) {
        self.end_round(EndRoundReason::AllGuessed);
    }

    fn end_round_timed_out(&mut self) {
        self.end_round(EndRoundReason::TimedOut);
    }

    fn end_round_abort(&mut self) {
        self.end_round(EndRoundReason::Aborted);
    }

    fn end_round(&mut self, reason: EndRoundReason) {
        let (word, drawer, scores_this_round, correct_guessers, round_index) =
            match std::mem::replace(&mut self.game.phase, GamePhase::Lobby) {
                GamePhase::Drawing {
                    word,
                    drawer,
                    scores_this_round,
                    correct_guessers,
                    round_index,
                    ..
                } => (
                    word,
                    drawer,
                    scores_this_round,
                    correct_guessers,
                    round_index,
                ),
                other => {
                    // Wasn't Drawing; restore and move on.
                    self.game.phase = other;
                    return;
                }
            };

        // Award drawer bonus only if the round wasn't aborted.
        let mut total = 0u32;
        for v in scores_this_round.values() {
            total = total.saturating_add(*v);
        }
        let bonus = if matches!(reason, EndRoundReason::Aborted) {
            0
        } else {
            drawer_bonus(total)
        };

        for (pid, delta) in &scores_this_round {
            let entry = self.game.scores.entry(*pid).or_insert(0);
            *entry = entry.saturating_add(*delta);
        }
        if bonus > 0 {
            let entry = self.game.scores.entry(drawer).or_insert(0);
            *entry = entry.saturating_add(bonus);
        }

        let _ = correct_guessers; // already tracked in scores_this_round
        let cumulative = ranked_scores(&self.game.scores);
        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::RoundEnd {
                word: word.clone(),
                scores: cumulative,
            },
        });

        let deadline = Instant::now() + ROUND_REVEAL;
        self.game.phase = GamePhase::RoundEnd {
            word,
            deadline,
            round_index,
        };
    }

    fn advance_round(&mut self) {
        let GamePhase::RoundEnd { round_index, .. } = self.game.phase else {
            return;
        };
        let next = round_index as u16 + 1;
        if next >= self.game.rounds as u16 {
            self.end_game();
        } else {
            self.start_choosing_round(next as u8);
        }
    }

    fn end_game(&mut self) {
        let final_scores = ranked_scores(&self.game.scores);
        let seq = self.next_seq();
        self.broadcast(ServerMsg::Game {
            seq,
            event: GameEvent::GameOver { final_scores },
        });
        self.game.phase = GamePhase::GameOver;
    }

    // ---- deadlines -------------------------------------------------------

    fn next_deadline(&self) -> Option<Instant> {
        match &self.game.phase {
            GamePhase::ChoosingWord { deadline, .. } => Some(*deadline),
            GamePhase::RoundEnd { deadline, .. } => Some(*deadline),
            GamePhase::Drawing {
                deadline,
                hint_schedule,
                ..
            } => {
                let next_hint = hint_schedule.first().copied();
                match next_hint {
                    Some(h) if h < *deadline => Some(h),
                    _ => Some(*deadline),
                }
            }
            GamePhase::Lobby | GamePhase::GameOver => None,
        }
    }

    fn handle_deadline(&mut self) {
        let now = Instant::now();
        let action = match &self.game.phase {
            GamePhase::ChoosingWord { deadline, .. } if now >= *deadline => {
                DeadlineAction::AutoPickWord
            }
            GamePhase::Drawing {
                deadline,
                hint_schedule,
                ..
            } => {
                if hint_schedule.first().is_some_and(|h| *h <= now) {
                    DeadlineAction::RevealHint
                } else if now >= *deadline {
                    DeadlineAction::EndRoundTimedOut
                } else {
                    DeadlineAction::None
                }
            }
            GamePhase::RoundEnd { deadline, .. } if now >= *deadline => {
                DeadlineAction::NextRoundOrFinish
            }
            _ => DeadlineAction::None,
        };
        match action {
            DeadlineAction::None => {}
            DeadlineAction::AutoPickWord => self.auto_pick_word(),
            DeadlineAction::RevealHint => self.reveal_one_hint(),
            DeadlineAction::EndRoundTimedOut => self.end_round_timed_out(),
            DeadlineAction::NextRoundOrFinish => self.advance_round(),
        }
    }
}

enum GuessOutcome {
    Correct { everyone_guessed: bool },
    Wrong,
    NotInGame,
    DrawerSelfGuess,
    AlreadyCorrect,
}

enum EndRoundReason {
    AllGuessed,
    TimedOut,
    Aborted,
}

async fn sleep_until_or_pending(when: Option<Instant>) {
    match when {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending::<()>().await,
    }
}
