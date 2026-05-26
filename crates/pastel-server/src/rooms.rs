use dashmap::DashMap;
use pastel_proto::RoomCode;
use pastel_room::{spawn_room, RoomHandle, WordLists};
use std::sync::Arc;

#[derive(Clone)]
pub struct Rooms {
    inner: Arc<DashMap<RoomCode, RoomHandle>>,
    words: Arc<WordLists>,
}

impl Rooms {
    pub fn new(words: Arc<WordLists>) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            words,
        }
    }

    pub fn get_or_create(&self, code: RoomCode) -> RoomHandle {
        let words = self.words.clone();
        self.inner
            .entry(code)
            .or_insert_with(|| spawn_room(code, words))
            .clone()
    }

    pub fn count(&self) -> usize {
        self.inner.len()
    }
}
