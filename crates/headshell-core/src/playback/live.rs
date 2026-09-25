//! A playing session: `Player` + `Session` wired together in one place.
//!
//! **Why in the core (K1):** every shell was doing the same dance — a regular
//! `tick()`, then writing the accumulated listens to the store. The TUI wrote
//! it once, the GUI a second time, mobile would have written it a third. A
//! shell that forgets to write listens to the store **silently loses
//! history** — and nothing makes the loss noticeable.
//!
//! **True to D-015:** no observers/callbacks. The shell drives the loop
//! itself, gets a `TickReport` every round and estimates the position from
//! the anchor.
//! **True to K7:** no closure parameters, generics or leaking lifetimes.

use serde::{Deserialize, Serialize};

use crate::library::WriteSummary;
use crate::model::Listen;
use crate::playback::{PlayState, PlaybackAnchor, Player};
use crate::session::Session;
use crate::{Error, Result};

/// What happened in a `tick`.
///
/// The shell looks at this and redraws. The position is **not** here: it is
/// estimated from the `anchor`, otherwise it would have to be asked dozens of
/// times a second.
///
/// `Serialize`: the GUI sends this to the webview as it is (§3.2). No
/// separate "IPC type" is written — a translating layer would let the two
/// types drift apart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickReport {
    /// Always filled in. The shell computes the position from this
    /// (`PlaybackAnchor::position_at`).
    pub anchor: PlaybackAnchor,
    /// Did the playing track change this round.
    pub track_changed: bool,
    /// The number of listens **written** to the store this round.
    pub listens_recorded: usize,
    /// The number of listens that could not be written and are held back.
    ///
    /// Greater than zero means the store gave a write error; the records **were
    /// not thrown away**, they will be retried next round. The shell should show
    /// this.
    pub listens_pending: usize,
    /// The error chain, if writing to the store failed this round.
    ///
    /// That is why `tick` does not return `Err`: the audio keeps playing, and
    /// dropping the session would increase the data loss. But it does not stay
    /// silent either (K9).
    pub store_error: Option<String>,
    /// The queue ran out and the audio stopped.
    pub finished: bool,
}

/// The playing session.
///
/// `Session` (library, statistics, diagnostics) and `Player` (audio, queue)
/// live here together. Access to both stays open: search and statistics are
/// needed while playing too.
pub struct LiveSession {
    session: Session,
    player: Player,
    /// Listens that could not be written to the store. Since `take_listens`
    /// pulls them out of the player, **if they are not kept here they are lost**
    /// when the write fails.
    pending: Vec<Listen>,
}

impl LiveSession {
    /// Wires a session and a player together.
    #[must_use]
    pub fn new(session: Session, player: Player) -> Self {
        Self {
            session,
            player,
            pending: Vec::new(),
        }
    }

    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    #[must_use]
    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.player
    }

    /// The current anchor.
    #[must_use]
    pub fn anchor(&self) -> PlaybackAnchor {
        self.player.anchor()
    }

    /// Replaces the playing player with a new one (starting a new queue).
    ///
    /// **The old player's accumulated listens are taken.** Writing
    /// `*live.player_mut() = new` directly would drop the old player without
    /// calling `take_listens`: the moment the user made a new search, the track
    /// they had just listened to would silently be lost. The records move from
    /// here to `pending`, and the first `tick` writes them.
    ///
    /// The old audio pipeline closes — `Player` stops when dropped.
    pub fn replace_player(&mut self, player: Player) {
        self.player.stop();
        self.pending.extend(self.player.take_listens());
        self.player = player;
    }

    /// One round: advance the player, write the accumulated listens
    /// **immediately**.
    ///
    /// The write happens every round, not on exit. In an interface left open for
    /// hours, writing on exit would take the whole session's history with it on
    /// a crash or a `kill`. Most rounds have nothing to write — `take_listens`
    /// comes back empty and the store is never touched.
    ///
    /// # Errors
    /// If the player fails while advancing (the source could not be opened, the
    /// code could not be decoded). **A store write error does not produce an
    /// `Err`** — it lands in `TickReport::store_error`.
    pub async fn tick(&mut self) -> Result<TickReport> {
        // The queue position is looked at too: the same track can be in the
        // queue twice, and comparing only the `TrackRef` would miss the
        // transition between them.
        //
        // The case it does not catch is written down on purpose: when
        // `RepeatMode::One` restarts the same track, neither the track nor the
        // position changes, and `track_changed` stays `false`. That is also
        // what is right for the shell — the track shown is the same; the
        // anchor's position says it started over, and `listens_recorded` says
        // there is a new listen.
        let before = (
            self.player.current_track().cloned(),
            self.player.queue().position(),
        );

        let tick_result = self.player.tick().await;

        // The accumulated listens must be written even if the player failed:
        // the error belongs to one track, the history to the whole session.
        let (recorded, store_error) = self.flush();

        let after = (
            self.player.current_track().cloned(),
            self.player.queue().position(),
        );
        let anchor = self.player.anchor();
        let finished =
            self.player.state() == PlayState::Stopped && self.player.current_track().is_none();

        tick_result?;

        Ok(TickReport {
            anchor,
            track_changed: before != after,
            listens_recorded: recorded,
            listens_pending: self.pending.len(),
            store_error,
            finished,
        })
    }

    /// Closes the session: stops the audio, writes every remaining listen.
    ///
    /// # Errors
    /// If the last write fails. The records are then **still at hand**;
    /// `pending_listens()` says how many could not be written.
    pub fn shutdown(&mut self) -> Result<WriteSummary> {
        self.player.stop();
        self.pending.extend(self.player.take_listens());
        if self.pending.is_empty() {
            return Ok(WriteSummary::default());
        }
        let summary = self.session.record_listens(&self.pending)?;
        self.pending.clear();
        Ok(summary)
    }

    /// The number of listens not yet written to the store.
    #[must_use]
    pub fn pending_listens(&self) -> usize {
        self.pending.len()
    }

    /// Tries to write the accumulated listens. If it cannot, it **holds on** to
    /// them.
    fn flush(&mut self) -> (usize, Option<String>) {
        self.pending.extend(self.player.take_listens());
        if self.pending.is_empty() {
            return (0, None);
        }
        let outcome = self.session.record_listens(&self.pending);
        absorb(&mut self.pending, outcome)
    }
}

/// Applies a write result to the buffer.
///
/// A separate function, because the real claim is **on the failure path**: a
/// record that could not be written is not thrown away, it stays at hand.
/// Testing this with a real broken store was tried and was not reliable —
/// with an open file handle SQLite keeps writing even in a read-only
/// directory. A test whose condition cannot be met turns green and proves
/// nothing; the decision was pulled out here so it can be tested directly.
fn absorb(pending: &mut Vec<Listen>, outcome: Result<WriteSummary>) -> (usize, Option<String>) {
    match outcome {
        Ok(summary) => {
            pending.clear();
            (summary.inserted, None)
        }
        // The records stay in `pending`: the next round will retry.
        Err(err) => (0, Some(Error::chain_text(&err))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::diag::Stage;
    use crate::error::ErrorKind;
    use crate::ids::ProviderId;
    use crate::model::{ListenSource, TrackRef};
    use crate::provider::ProviderRegistry;

    fn temp_dir(label: &str) -> crate::test_support::TempDir {
        crate::test_support::TempDir::new(&format!("live-{label}"))
    }

    fn live_at(dir: &std::path::Path) -> LiveSession {
        let session = Session::open(Config::with_data_dir(dir)).expect("the session must open");
        LiveSession::new(session, Player::new(ProviderRegistry::new()))
    }

    fn listen(title: &str) -> Listen {
        Listen {
            track: TrackRef::new("Artist", title),
            played_at: jiff::Timestamp::now(),
            ms_played: 200_000,
            source: ListenSource::Playback {
                provider: ProviderId::new("local"),
            },
            canonical_id: None,
        }
    }

    #[tokio::test]
    async fn an_idle_session_reports_finished_without_inventing_listens() {
        let dir = temp_dir("idle");
        let mut live = live_at(&dir);

        let report = live.tick().await.expect("an idle tick must not fail");

        assert!(
            report.finished,
            "the queue is empty and the audio has stopped"
        );
        assert!(!report.track_changed);
        assert_eq!(report.listens_recorded, 0);
        assert_eq!(report.listens_pending, 0);
        assert!(report.store_error.is_none());
        assert_eq!(report.anchor.state, PlayState::Stopped);
    }

    #[test]
    fn flushing_writes_listens_and_empties_the_buffer() {
        let dir = temp_dir("write");
        let mut live = live_at(&dir);
        live.pending.push(listen("One"));
        live.pending.push(listen("Two"));

        let (recorded, error) = live.flush();

        assert_eq!(recorded, 2, "both must be written");
        assert!(error.is_none(), "{error:?}");
        assert_eq!(live.pending_listens(), 0, "the buffer must empty");
    }

    /// The real claim: **if the store cannot write, no listen is lost.**
    ///
    /// `take_listens` pulls the records out of the player; if the write fails
    /// and they are not held on to, there is no place to get them back from.
    /// Writing used to happen only on exit, so a single error would take the
    /// whole session with it.
    #[test]
    fn a_failing_store_keeps_the_listens_instead_of_dropping_them() {
        let mut pending = vec![listen("Must not be lost"), listen("Nor this one")];

        let (recorded, error) = absorb(
            &mut pending,
            Err(Error::new(
                Stage::LibraryWrite,
                ErrorKind::InvalidInput {
                    detail: "disk full".to_owned(),
                },
            )),
        );

        assert_eq!(recorded, 0, "none were written");
        assert_eq!(
            pending.len(),
            2,
            "the records must stay AT HAND, not be thrown away"
        );
        let text = error.expect("the error must be reported, not swallowed");
        assert!(
            text.starts_with("STEP: "),
            "the stage must be reported (K9): {text}"
        );
        assert!(
            text.contains("disk full"),
            "the reason must be visible: {text}"
        );
    }

    #[test]
    fn a_successful_store_empties_the_buffer() {
        let mut pending = vec![listen("Written")];

        let (recorded, error) = absorb(
            &mut pending,
            Ok(WriteSummary {
                offered: 1,
                inserted: 1,
                duplicates: 0,
                new_tracks: 1,
            }),
        );

        assert_eq!(recorded, 1);
        assert!(error.is_none());
        assert!(
            pending.is_empty(),
            "a written record must not stay in the buffer"
        );
    }

    /// Starting a new queue must not delete the records left over from the
    /// previous round.
    ///
    /// The part this test **does not cover** is written down on purpose: is the
    /// old player's own `pending_listens` carried over here? `Player`'s field is
    /// private and producing listens needs a real audio pipeline; that path is
    /// tested by `tests/playback_local.rs`. What is tested here is that the
    /// buffer **is not thrown away** during the replacement.
    #[test]
    fn replacing_the_player_swaps_the_queue_and_keeps_pending_listens() {
        use crate::ids::ProviderTrackId;
        use crate::playback::QueueItem;

        let dir = temp_dir("replace");
        let mut live = live_at(&dir);
        live.pending
            .push(listen("Left over from the previous round"));

        let mut replacement = Player::new(ProviderRegistry::new());
        replacement.queue_mut().replace(vec![QueueItem {
            id: ProviderTrackId::new(ProviderId::new("local"), "1"),
            track: TrackRef::new("Artist", "New queue"),
        }]);

        live.replace_player(replacement);

        assert_eq!(
            live.player().queue().len(),
            1,
            "the new queue must be in effect"
        );
        assert_eq!(
            live.pending_listens(),
            1,
            "the old buffer must not be thrown away"
        );
    }

    #[test]
    fn shutdown_writes_what_is_still_pending() {
        let dir = temp_dir("close");
        let mut live = live_at(&dir);
        live.pending.push(listen("Son"));

        let summary = live.shutdown().expect("shutdown must be able to write");

        assert_eq!(summary.inserted, 1);
        assert_eq!(live.pending_listens(), 0);
    }
}
