//! Glue between the player state machine, queue, settings and SQLite.
//!
//! The worker owns a `Session`, feeds it libVLC events, and executes the
//! returned effects on the libVLC player. Keeping effects as data makes the
//! auto-advance / resume behaviour unit-testable without a media player.

use std::time::{Duration, Instant};

use crate::error::Result;
use crate::model::MediaKind;
use crate::queue::Queue;
use crate::settings::SettingsStore;
use crate::state::{PlaybackEvent, PlayerState};
use crate::store::Store;

const PROGRESS_SAVE_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SessionEffects {
    pub seek: Option<Duration>,
    pub play: Option<String>,
    pub stop: bool,
    /// `(media id, position ms, completed)`.
    pub progress: Option<(i64, i64, bool)>,
}

impl SessionEffects {
    pub fn is_empty(&self) -> bool {
        self.seek.is_none() && self.play.is_none() && !self.stop && self.progress.is_none()
    }
}

pub struct Session {
    pub state: PlayerState,
    pub queue: Queue,
    pub store: Store,
    pub settings: SettingsStore,
    current_media: Option<i64>,
    last_progress_save: Option<Instant>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state)
            .field("queue_len", &self.queue.len())
            .field("current_media", &self.current_media)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn new(store: Store, settings: SettingsStore) -> Result<Self> {
        let cfg = settings.get();
        let (uris, current) = store.load_queue()?;
        Ok(Self {
            state: PlayerState::from_settings(&cfg),
            queue: Queue::from_saved(uris, current, cfg.shuffle, cfg.repeat),
            store,
            settings,
            current_media: None,
            last_progress_save: None,
        })
    }

    pub fn current_media_id(&self) -> Option<i64> {
        self.current_media
    }

    /// Registers the media about to play, touches `recent`, and selects it in
    /// the queue when present. Returns the stable media id.
    pub fn begin_playback(&mut self, uri: &str, title: &str, kind: MediaKind) -> Result<i64> {
        let id = self.store.record_play(uri, title, kind, 0)?;
        self.current_media = Some(id);
        self.last_progress_save = None;
        self.state.reset_for_media();
        if self.queue.items().iter().any(|u| u == uri) {
            self.queue.select_uri(uri);
            self.persist_queue()?;
        }
        Ok(id)
    }

    /// Applies one event and returns the side effects the worker must perform.
    pub fn apply_event(&mut self, event: PlaybackEvent) -> SessionEffects {
        let mut effects = SessionEffects::default();
        let time = match &event {
            PlaybackEvent::TimeChanged(t) => Some(*t),
            _ => None,
        };
        self.state.apply(event);

        if let Some(t) = time {
            if let Some(seek) = self.state.ab_repeat_seek() {
                effects.seek = Some(seek);
            }
            if self.should_save_progress() {
                effects.progress = self.progress_effect(t, false);
            }
        }

        if matches!(self.state.status, crate::state::PlaybackState::Ended) {
            effects.progress = Some((
                self.current_media.unwrap_or(0),
                self.state.time.as_millis().min(i64::MAX as u128) as i64,
                true,
            ));
            let autoplay = self.settings.get().autoplay;
            match self.queue.on_end_reached(autoplay) {
                Some(uri) => effects.play = Some(uri.to_owned()),
                None => effects.stop = true,
            }
        }
        effects
    }

    /// Persists an effect produced by [`Self::apply_event`].
    pub fn commit_effects(&mut self, effects: &SessionEffects) -> Result<()> {
        if let Some((id, pos, completed)) = effects.progress {
            self.store.save_progress(id, pos, completed)?;
            self.last_progress_save = Some(Instant::now());
        }
        if effects.play.is_some() || effects.stop {
            self.persist_queue()?;
        }
        Ok(())
    }

    /// Resume position for `uri`, or `None` when the item is new / already
    /// watched or the user disabled resume.
    pub fn resume_position(&self, uri: &str) -> Result<Option<Duration>> {
        if !self.settings.get().remember_position {
            return Ok(None);
        }
        let Some(item) = self.store.media_by_uri(uri)? else {
            return Ok(None);
        };
        match self.store.progress(item.id)? {
            Some(p) if !p.completed && p.position_ms > 0 => {
                Ok(Some(Duration::from_millis(p.position_ms.max(0) as u64)))
            }
            _ => Ok(None),
        }
    }

    pub fn set_queue(&mut self, items: Vec<String>, current: usize) -> Result<()> {
        self.queue.set_items(items, current);
        self.persist_queue()
    }

    pub fn append(&mut self, uri: impl Into<String>) -> Result<()> {
        self.queue.push(uri);
        self.persist_queue()
    }

    pub fn remove_at(&mut self, index: usize) -> Result<()> {
        self.queue.remove(index);
        self.persist_queue()
    }

    pub fn move_item(&mut self, from: usize, to: usize) -> Result<()> {
        self.queue.move_item(from, to);
        self.persist_queue()
    }

    pub fn clear_queue(&mut self) -> Result<()> {
        self.queue.clear();
        self.persist_queue()
    }

    pub fn set_repeat(&mut self, mode: crate::model::RepeatMode) -> Result<()> {
        self.queue.set_repeat(mode);
        self.settings.update(|s| s.repeat = mode)
    }

    pub fn set_shuffle(&mut self, enabled: bool) -> Result<()> {
        self.queue.set_shuffle(enabled);
        self.settings.update(|s| s.shuffle = enabled)
    }

    fn persist_queue(&self) -> Result<()> {
        let (uris, current) = self.queue.snapshot();
        self.store.save_queue(&uris, current)
    }

    fn should_save_progress(&self) -> bool {
        self.current_media.is_some()
            && self
                .last_progress_save
                .is_none_or(|t| t.elapsed() >= PROGRESS_SAVE_INTERVAL)
    }

    fn progress_effect(&self, time: Duration, completed: bool) -> Option<(i64, i64, bool)> {
        self.current_media
            .map(|id| (id, time.as_millis().min(i64::MAX as u128) as i64, completed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn session() -> Session {
        Session::new(Store::in_memory().unwrap(), SettingsStore::in_memory()).unwrap()
    }

    #[test]
    fn new_session_restores_saved_queue_and_settings() -> Result<()> {
        let store = Store::in_memory()?;
        store.save_queue(&["a".into(), "b".into()], 1)?;
        let settings = SettingsStore::in_memory();
        settings.update(|s: &mut Settings| {
            s.shuffle = true;
            s.repeat = crate::model::RepeatMode::One;
        })?;
        let s = Session::new(store, settings)?;
        assert_eq!(s.queue.current(), Some("b"));
        assert!(s.queue.shuffle());
        assert_eq!(s.queue.repeat(), crate::model::RepeatMode::One);
        Ok(())
    }

    #[test]
    fn begin_playback_touches_recent_and_resume() -> Result<()> {
        let mut s = session();
        let id = s.begin_playback("a.mp4", "A", MediaKind::Video)?;
        s.apply_event(PlaybackEvent::TimeChanged(Duration::from_secs(42)));
        let effects = SessionEffects {
            progress: Some((id, 42_000, false)),
            ..Default::default()
        };
        s.commit_effects(&effects)?;
        assert_eq!(s.resume_position("a.mp4")?, Some(Duration::from_secs(42)));
        Ok(())
    }

    #[test]
    fn end_of_media_auto_advances_and_saves_completion() -> Result<()> {
        let mut s = session();
        s.set_queue(vec!["a".into(), "b".into()], 0)?;
        s.begin_playback("a", "A", MediaKind::Video)?;
        let effects = s.apply_event(PlaybackEvent::EndReached);
        assert_eq!(effects.play.as_deref(), Some("b"));
        assert_eq!(effects.progress.map(|p| p.2), Some(true));
        assert!(!effects.stop);
        Ok(())
    }

    #[test]
    fn end_of_last_item_stops_when_autoplay_is_off() -> Result<()> {
        let mut s = session();
        s.settings.update(|cfg| cfg.autoplay = false)?;
        s.set_queue(vec!["a".into()], 0)?;
        s.begin_playback("a", "A", MediaKind::Video)?;
        let effects = s.apply_event(PlaybackEvent::EndReached);
        assert!(effects.stop);
        assert!(effects.play.is_none());
        Ok(())
    }

    #[test]
    fn ab_repeat_produces_a_seek_effect() {
        let mut s = session();
        s.apply_event(PlaybackEvent::LengthChanged(Duration::from_secs(100)));
        s.state
            .set_ab_repeat(Some(Duration::from_secs(10)), Some(Duration::from_secs(20)));
        let effects = s.apply_event(PlaybackEvent::TimeChanged(Duration::from_secs(25)));
        assert_eq!(effects.seek, Some(Duration::from_secs(10)));
    }

    #[test]
    fn queue_edits_are_persisted() -> Result<()> {
        let mut s = session();
        s.set_queue(vec!["a".into(), "b".into()], 0)?;
        s.append("c")?;
        s.move_item(2, 0)?;
        s.remove_at(1)?;
        let (uris, current) = s.store.load_queue()?;
        assert_eq!(uris, vec!["c", "b"]);
        assert_eq!(current, 0);
        Ok(())
    }
}
