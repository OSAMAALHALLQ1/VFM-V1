use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Opening,
    Playing,
    Paused,
    Ended,
}

impl PlaybackState {
    pub fn needs_restart(self) -> bool {
        matches!(self, Self::Stopped | Self::Ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_only_when_stopped_or_ended() {
        assert!(PlaybackState::Stopped.needs_restart());
        assert!(PlaybackState::Ended.needs_restart());
        for state in [
            PlaybackState::Opening,
            PlaybackState::Playing,
            PlaybackState::Paused,
        ] {
            assert!(!state.needs_restart());
        }
    }
}
