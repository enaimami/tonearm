//! The playback anchor — the single representation of state (D-015).
//!
//! The consumer (TUI, GUI, mobile, a room in Phase 4) **computes the position
//! itself**:
//!
//! ```text
//! pos = position_ms + (now - wall_time) * rate
//! ```
//!
//! So the core does not send hundreds of notifications per second; the
//! consumer reads the anchor as often as it wants to draw and fills in the
//! time in between itself.
//!
//! **Phase 4 note:** the rooms' sync primitive is exactly this type (PLAN
//! 4.1). Today it is written for local playback, tomorrow it will be broadcast
//! over the network — it was designed the same way from the start so there
//! are not two separate state models.

use serde::{Deserialize, Serialize};

use crate::ids::CanonicalId;

/// The player's coarse state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    /// Nothing is loaded.
    Stopped,
    /// Loaded and advancing.
    Playing,
    /// Loaded but paused; the position is frozen.
    Paused,
    /// Loaded, not advancing, waiting for data (network/disk).
    ///
    /// Separate from `Paused`: the user did not ask for it, the pipeline is
    /// waiting. The consumer should show this differently — silently saying
    /// "paused" misleads the user.
    Buffering,
}

impl PlayState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Buffering => "buffering",
        }
    }

    /// Is time advancing? Only in `Playing`.
    #[must_use]
    pub const fn advances(self) -> bool {
        matches!(self, Self::Playing)
    }
}

impl std::fmt::Display for PlayState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The single representation of playback state: a time anchor.
///
/// A plain record for `uniffi` — no trait objects, lifetimes or closures
/// (K7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackAnchor {
    /// The canonical identity of the playing track. `None` while `Stopped`.
    pub track: Option<CanonicalId>,
    /// The wall-clock time the anchor was taken at.
    pub wall_time: jiff::Timestamp,
    /// The position within the track, at `wall_time`.
    pub position_ms: u64,
    /// The playback rate. `1.0` is normal; `0.0` means not advancing.
    ///
    /// In Phase 4 drift correction will pull this to values like `1.001` (PLAN
    /// 4.4) — the field exists from today so the surface does not change that
    /// day.
    pub rate: f64,
    pub state: PlayState,
    /// The track's total duration (if known). For the progress bar.
    pub duration_ms: Option<u64>,
}

impl PlaybackAnchor {
    /// Nothing is playing.
    #[must_use]
    pub fn stopped() -> Self {
        Self {
            track: None,
            wall_time: jiff::Timestamp::now(),
            position_ms: 0,
            rate: 0.0,
            state: PlayState::Stopped,
            duration_ms: None,
        }
    }

    /// Computes the position for the given moment.
    ///
    /// The core's counterpart of the calculation the consumer does — it lives
    /// here so the GUI, the TUI and mobile do not write the same formula three
    /// times (the Golden Rule). If `duration_ms` is known the position does not
    /// go past it.
    #[must_use]
    pub fn position_at(&self, now: jiff::Timestamp) -> u64 {
        if !self.state.advances() || self.rate <= 0.0 {
            return self.clamp_to_duration(self.position_ms);
        }
        let elapsed = now
            .as_millisecond()
            .saturating_sub(self.wall_time.as_millisecond());
        if elapsed <= 0 {
            return self.clamp_to_duration(self.position_ms);
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "shown at millisecond scale; the f64 loss is inaudible"
        )]
        let advanced = (elapsed as f64 * self.rate) as u64;
        self.clamp_to_duration(self.position_ms.saturating_add(advanced))
    }

    /// The position right now.
    #[must_use]
    pub fn position_now(&self) -> u64 {
        self.position_at(jiff::Timestamp::now())
    }

    fn clamp_to_duration(&self, position: u64) -> u64 {
        match self.duration_ms {
            Some(duration) => position.min(duration),
            None => position,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> jiff::Timestamp {
        jiff::Timestamp::from_second(seconds).expect("test timestamp")
    }

    fn anchor(state: PlayState, position_ms: u64, rate: f64) -> PlaybackAnchor {
        PlaybackAnchor {
            track: Some(CanonicalId::from_local_key("test")),
            wall_time: at(1000),
            position_ms,
            rate,
            state,
            duration_ms: Some(240_000),
        }
    }

    #[test]
    fn playing_advances_with_wall_clock() {
        let a = anchor(PlayState::Playing, 30_000, 1.0);
        // 10 seconds after the anchor: 30 s + 10 s.
        assert_eq!(a.position_at(at(1010)), 40_000);
    }

    #[test]
    fn paused_position_is_frozen() {
        let a = anchor(PlayState::Paused, 30_000, 0.0);
        assert_eq!(a.position_at(at(1010)), 30_000, "paused must not advance");
    }

    #[test]
    fn buffering_does_not_advance_either() {
        // No sound comes out while buffering; an advancing position would mislead
        // the user.
        let a = anchor(PlayState::Buffering, 30_000, 1.0);
        assert_eq!(a.position_at(at(1010)), 30_000);
    }

    #[test]
    fn rate_scales_the_elapsed_time() {
        // Phase 4 drift correction: playing 10% fast, it advances 11 s in 10 s.
        let a = anchor(PlayState::Playing, 0, 1.1);
        assert_eq!(a.position_at(at(1010)), 11_000);
    }

    #[test]
    fn position_never_exceeds_the_known_duration() {
        let a = anchor(PlayState::Playing, 230_000, 1.0);
        // After 60 seconds it would be 290 s, but the track is 240 s.
        assert_eq!(a.position_at(at(1060)), 240_000);
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_rewind() {
        let a = anchor(PlayState::Playing, 30_000, 1.0);
        assert_eq!(
            a.position_at(at(900)),
            30_000,
            "a clock going backwards must not rewind the position"
        );
    }

    #[test]
    fn stopped_anchor_is_inert() {
        let a = PlaybackAnchor::stopped();
        assert_eq!(a.state, PlayState::Stopped);
        assert_eq!(a.track, None);
        assert_eq!(a.position_now(), 0);
    }
}
