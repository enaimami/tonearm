//! The terminal player interface (PLAN §1.7).
//!
//! **No business logic here.** This file only maps keys to core calls, draws
//! the [`PlaybackAnchor`] and refreshes the screen. The queue, repeat,
//! shuffle, scrobbling — all of it is inside `headshell-core`.
//!
//! The TUI is not the GUI's prototype; it is the proof that the core is fully
//! usable. The core must still be able to offer everything deleted from here.
//!
//! ## Why the position is not polled
//!
//! The core does not shower notifications (D-015). The TUI calls
//! `anchor.position_at(now)` at its own drawing rate and fills in the time in
//! between itself — the GUI and mobile will use the same formula, which is why
//! the formula is in the core.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph};

use headshell_core::playback::{LiveSession, PlayState, PlaybackAnchor, Player, RepeatMode};

/// How often the screen is refreshed. Independent of the audio pipeline: only
/// drawing.
const TICK: Duration = Duration::from_millis(200);

/// Tests that the terminal can really be taken **before** the TUI opens.
///
/// `--tui` needs two resources: a terminal and an audio output. The terminal
/// is tested for free, the audio output opens a hardware resource — which is
/// why this is the order. In the reverse order, in an environment without a
/// terminal the user saw a sound card error even though they had typed
/// `--tui`; the wrong diagnosis, the wrong stage.
///
/// The test is done with `enable_raw_mode` itself, not with a separate
/// `is_terminal` criterion: if the two criteria drifted apart, "passed the
/// test, failed when opening" would be born. Raw mode is given back right
/// away; the screen is not changed.
///
/// # Errors
/// If the terminal cannot be put into raw mode.
pub fn require_terminal() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    terminal::disable_raw_mode()
}

/// Puts the terminal into raw mode and gives it back on exit **in every
/// case**.
///
/// Restoring with `Drop`: even on a panic the user's terminal must not be left
/// broken. This is safer than calling `restore()` by hand.
struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// What the key the user pressed means.
///
/// The key → action mapping is here, the action → core call below. Keeping
/// them apart brings changing the key layout down to a single place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Quit,
    TogglePause,
    Next,
    Previous,
    SelectionUp,
    SelectionDown,
    PlaySelected,
    ToggleShuffle,
    CycleRepeat,
}

fn action_for(key: KeyEvent) -> Option<Action> {
    // Ctrl+C always exits: the behaviour expected in a terminal.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Char(' ') | KeyCode::Char('p') => Some(Action::TogglePause),
        KeyCode::Char('n') | KeyCode::Right => Some(Action::Next),
        KeyCode::Char('b') | KeyCode::Left => Some(Action::Previous),
        KeyCode::Up | KeyCode::Char('k') => Some(Action::SelectionUp),
        KeyCode::Down | KeyCode::Char('j') => Some(Action::SelectionDown),
        KeyCode::Enter => Some(Action::PlaySelected),
        KeyCode::Char('s') => Some(Action::ToggleShuffle),
        KeyCode::Char('r') => Some(Action::CycleRepeat),
        _ => None,
    }
}

/// Runs the TUI; it lasts until the user quits or the queue ends.
///
/// On return the accumulated listen records are written to the library
/// (§1.6) and how many were written is returned.
///
/// # Errors
/// If the terminal cannot be set up or the core returns an error.
pub async fn run(live: &mut LiveSession) -> anyhow::Result<usize> {
    let mut guard = TerminalGuard::enter()?;
    let mut selection = ListState::default();
    selection.select(Some(live.player().queue().position()));
    let mut last_error: Option<String> = None;
    let mut recorded = 0usize;

    let result = loop {
        // — Draw.
        let player = live.player();
        let anchor = player.anchor();
        // The queue view comes from the core in one piece: play order,
        // position, repeat and shuffle from the same moment. The GUI gets the
        // same type.
        let queue = player.queue().view();
        let queue_items: Vec<String> = queue
            .items
            .iter()
            .map(|item| item.track.display_name())
            .collect();
        let title = player
            .current_track()
            .map(|track| track.display_name())
            .unwrap_or_else(|| "—".to_owned());

        guard.terminal.draw(|frame| {
            draw(
                frame,
                &View {
                    anchor: &anchor,
                    title: &title,
                    queue: &queue_items,
                    playing: queue.position,
                    repeat: queue.repeat,
                    shuffle: queue.shuffle,
                    error: last_error.as_deref(),
                },
                &mut selection,
            );
        })?;

        // — Read a key (with a timeout: the screen must still refresh).
        let deadline = Instant::now() + TICK;
        let mut quit = false;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !event::poll(remaining)? {
                break;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            // On Windows releasing a key produces an event too; take only presses.
            if key.kind != KeyEventKind::Press {
                continue;
            }
            let Some(action) = action_for(key) else {
                continue;
            };
            if action == Action::Quit {
                quit = true;
                break;
            }
            if let Err(err) =
                apply(live.player_mut(), action, &mut selection, queue_items.len()).await
            {
                // An error does not bring the TUI down: it is shown to the user and the
                // loop carries on.
                last_error = Some(err.chain_text());
            } else {
                last_error = None;
            }
        }
        if quit {
            break Ok(());
        }

        // — One round: if the track ended, move on to the next; write the
        // accumulated listens.
        match live.tick().await {
            Ok(report) => {
                recorded += report.listens_recorded;
                // If the store could not write, it does not stay silent: the records
                // were held and will be retried next round, but the user should know.
                //
                // It is only written **if** there is an error: an unconditional
                // assignment would wipe out, on the next round, the error of the key
                // the user just pressed.
                if let Some(text) = report.store_error {
                    last_error = Some(format!(
                        "{text}\n  → {} listens held back",
                        report.listens_pending
                    ));
                }
                if report.finished {
                    break Ok(());
                }
            }
            Err(err) => last_error = Some(err.chain_text()),
        }
    };
    let result: io::Result<()> = result;
    result?;

    let summary = live.shutdown()?;
    Ok(recorded + summary.inserted)
}

/// Turns an action into a core call.
///
/// Note: no decision is made here. The answer to "pause or resume" is in
/// `Player::toggle_pause`; the TUI only passes it on.
async fn apply(
    player: &mut Player,
    action: Action,
    selection: &mut ListState,
    queue_len: usize,
) -> headshell_core::Result<()> {
    match action {
        // Quitting is handled in the calling loop; it never gets here.
        Action::Quit => {}
        Action::TogglePause => player.toggle_pause(),
        Action::Next => {
            player.next().await?;
            selection.select(Some(player.queue().position()));
        }
        Action::Previous => {
            player.previous().await?;
            selection.select(Some(player.queue().position()));
        }
        Action::SelectionUp => {
            let current = selection.selected().unwrap_or(0);
            selection.select(Some(current.saturating_sub(1)));
        }
        Action::SelectionDown => {
            let current = selection.selected().unwrap_or(0);
            selection.select(Some((current + 1).min(queue_len.saturating_sub(1))));
        }
        Action::PlaySelected => {
            if let Some(index) = selection.selected() {
                player.jump_to(index).await?;
            }
        }
        Action::ToggleShuffle => {
            let shuffle = player.queue().shuffle();
            player.queue_mut().set_shuffle(!shuffle);
            selection.select(Some(player.queue().position()));
        }
        Action::CycleRepeat => {
            let next = match player.queue().repeat() {
                RepeatMode::Off => RepeatMode::All,
                RepeatMode::All => RepeatMode::One,
                RepeatMode::One => RepeatMode::Off,
            };
            player.queue_mut().set_repeat(next);
        }
    }
    Ok(())
}

/// Everything needed for drawing — a read-only view gathered in one place.
struct View<'a> {
    anchor: &'a PlaybackAnchor,
    title: &'a str,
    queue: &'a [String],
    playing: usize,
    repeat: RepeatMode,
    shuffle: bool,
    error: Option<&'a str>,
}

fn draw(frame: &mut ratatui::Frame<'_>, view: &View<'_>, selection: &mut ListState) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3), // playing track
        Constraint::Length(3), // progress
        Constraint::Min(3),    // queue
        Constraint::Length(if view.error.is_some() { 3 } else { 1 }),
    ])
    .split(area);

    draw_now_playing(frame, chunks[0], view);
    draw_progress(frame, chunks[1], view.anchor);
    draw_queue(frame, chunks[2], view, selection);
    draw_footer(frame, chunks[3], view);
}

fn draw_now_playing(frame: &mut ratatui::Frame<'_>, area: Rect, view: &View<'_>) {
    let state = match view.anchor.state {
        PlayState::Playing => ("▶", Color::Green, "playing"),
        PlayState::Paused => ("⏸", Color::Yellow, "paused"),
        PlayState::Buffering => ("⋯", Color::Cyan, "waiting"),
        PlayState::Stopped => ("■", Color::DarkGray, "stopped"),
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", state.0),
            Style::default().fg(state.1).add_modifier(Modifier::BOLD),
        ),
        Span::styled(view.title, Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  ({})", state.2),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL).title(" headshell ")),
        area,
    );
}

fn draw_progress(frame: &mut ratatui::Frame<'_>, area: Rect, anchor: &PlaybackAnchor) {
    // The position is **computed** from the anchor — the core is not asked
    // (D-015).
    let position = anchor.position_now();
    let (ratio, label) = match anchor.duration_ms {
        Some(duration) if duration > 0 => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "progress bar ratio; the loss is invisible at ms scale"
            )]
            let ratio = (position as f64 / duration as f64).clamp(0.0, 1.0);
            (ratio, format!("{} / {}", clock(position), clock(duration)))
        }
        _ => (0.0, clock(position)),
    };

    frame.render_widget(
        Gauge::default()
            .block(Block::default().borders(Borders::ALL))
            .gauge_style(Style::default().fg(Color::Rgb(255, 180, 84)))
            .ratio(ratio)
            .label(label),
        area,
    );
}

fn draw_queue(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &View<'_>,
    selection: &mut ListState,
) {
    let items: Vec<ListItem<'_>> = view
        .queue
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let marker = if index == view.playing { "▸ " } else { "  " };
            let style = if index == view.playing {
                Style::default()
                    .fg(Color::Rgb(255, 180, 84))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::raw(marker),
                Span::styled(name.clone(), style),
            ]))
        })
        .collect();

    let title = format!(
        " queue ({}) · repeat: {} · shuffle: {} ",
        view.queue.len(),
        repeat_label(view.repeat),
        if view.shuffle { "on" } else { "off" }
    );
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        selection,
    );
}

/// The **user-facing** name of the repeat mode.
///
/// `RepeatMode`'s `Display` prints `off`/`all`/`one`, and that is the right
/// place for it: that string is the wire value going to JSON and IPC. The text
/// written on screen is a separate thing (D-036, D-073) — tying the two into
/// one function would make it impossible to fix the interface text without
/// changing the wire format.
fn repeat_label(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::All => "all",
        RepeatMode::One => "one",
    }
}

fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, view: &View<'_>) {
    if let Some(error) = view.error {
        // The error does not cover the screen, but it is not hidden either (K9):
        // its first line is enough; all of it can be read with `headshell diag`.
        let first = error.lines().next().unwrap_or(error);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {first}  (details: headshell diag)"),
                Style::default().fg(Color::Red),
            )))
            .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " space pause · n/b next/previous · ↑↓ select · enter play · s shuffle · r repeat · q quit",
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

/// Turns milliseconds into the `3:07` format.
fn clock(ms: u64) -> String {
    let total_seconds = ms / 1000;
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire value stays as it is; the text written on screen is a separate
    /// string (D-036).
    #[test]
    fn the_repeat_label_is_turkish_while_the_wire_value_stays_english() {
        assert_eq!(repeat_label(RepeatMode::Off), "off");
        assert_eq!(repeat_label(RepeatMode::All), "all");
        assert_eq!(repeat_label(RepeatMode::One), "one");
        // The wire side must not change: JSON and IPC read it.
        assert_eq!(RepeatMode::All.as_str(), "all");
        assert_eq!(RepeatMode::One.to_string(), "one");
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn clock_pads_seconds() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(7_000), "0:07");
        assert_eq!(clock(187_000), "3:07");
        assert_eq!(clock(3_600_000), "60:00");
    }

    #[test]
    fn keys_map_to_the_expected_actions() {
        assert_eq!(
            action_for(key(KeyCode::Char(' '))),
            Some(Action::TogglePause)
        );
        assert_eq!(action_for(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(action_for(key(KeyCode::Esc)), Some(Action::Quit));
        assert_eq!(action_for(key(KeyCode::Char('n'))), Some(Action::Next));
        assert_eq!(action_for(key(KeyCode::Left)), Some(Action::Previous));
        assert_eq!(action_for(key(KeyCode::Enter)), Some(Action::PlaySelected));
        assert_eq!(
            action_for(key(KeyCode::Char('s'))),
            Some(Action::ToggleShuffle)
        );
        assert_eq!(
            action_for(key(KeyCode::Char('r'))),
            Some(Action::CycleRepeat)
        );
        assert_eq!(
            action_for(key(KeyCode::Char('z'))),
            None,
            "an unknown key must be ignored"
        );
    }

    #[test]
    fn ctrl_c_always_quits() {
        // The behaviour expected in a terminal, even though 'c' alone does nothing.
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(action_for(ctrl_c), Some(Action::Quit));
        assert_eq!(action_for(key(KeyCode::Char('c'))), None);
    }

    #[test]
    fn vim_keys_move_the_selection() {
        assert_eq!(
            action_for(key(KeyCode::Char('j'))),
            Some(Action::SelectionDown)
        );
        assert_eq!(
            action_for(key(KeyCode::Char('k'))),
            Some(Action::SelectionUp)
        );
    }
}
