//! Terminal oynatıcı arayüzü (PLAN §1.7).
//!
//! **Burada iş mantığı yok.** Bu dosya yalnızca: tuşları çekirdek çağrılarına
//! eşler, [`PlaybackAnchor`]'ı çizer, ekranı tazeler. Kuyruk, tekrar,
//! karıştırma, scrobble — hepsi `tune-core` içinde.
//!
//! TUI, GUI'nin prototipi değil; çekirdeğin tam kullanılabilir olduğunun
//! kanıtı. Buradan silinen her şeyi çekirdek hâlâ sunabiliyor olmalı.
//!
//! ## Pozisyon neden yoklanmıyor
//!
//! Çekirdek bildirim yağdırmaz (D-015). TUI kendi çizim hızında
//! `anchor.position_at(now)` çağırır ve aradaki zamanı kendisi doldurur —
//! aynı formülü GUI ve mobil de kullanacak, o yüzden formül çekirdekte.

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

use tune_core::playback::{LiveSession, PlayState, PlaybackAnchor, Player, RepeatMode};

/// Ekran ne sıklıkta tazelenecek. Ses hattından bağımsız: yalnızca çizim.
const TICK: Duration = Duration::from_millis(200);

/// Terminali ham kipe alır ve çıkışta **her durumda** geri verir.
///
/// `Drop` ile geri alma: panik olsa bile kullanıcının terminali bozuk
/// kalmasın. Bu, elle `restore()` çağırmaktan daha güvenli.
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

/// Kullanıcının bastığı tuşun anlamı.
///
/// Tuş → eylem eşlemesi burada, eylem → çekirdek çağrısı aşağıda. İkisini
/// ayırmak, tuş dizilimini değiştirmeyi tek bir yere indirir.
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
    // Ctrl+C her zaman çıkış: terminalde beklenen davranış.
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

/// TUI'yi çalıştırır; kullanıcı çıkana ya da kuyruk bitene kadar sürer.
///
/// Dönüşte biriken dinleme kayıtları kütüphaneye yazılır (§1.6) ve kaç
/// tanesinin yazıldığı döndürülür.
///
/// # Errors
/// Terminal kurulamazsa ya da çekirdek bir hata döndürürse.
pub async fn run(live: &mut LiveSession) -> anyhow::Result<usize> {
    let mut guard = TerminalGuard::enter()?;
    let mut selection = ListState::default();
    selection.select(Some(live.player().queue().position()));
    let mut last_error: Option<String> = None;
    let mut recorded = 0usize;

    let result = loop {
        // — Çiz.
        let player = live.player();
        let anchor = player.anchor();
        let queue_items: Vec<String> = player
            .queue()
            .items()
            .iter()
            .map(|item| item.track.display_name())
            .collect();
        let position = player.queue().position();
        let repeat = player.queue().repeat();
        let shuffle = player.queue().shuffle();
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
                    playing: position,
                    repeat,
                    shuffle,
                    error: last_error.as_deref(),
                },
                &mut selection,
            );
        })?;

        // — Tuş oku (zaman aşımıyla: ekran yine de tazelenmeli).
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
            // Windows'ta tuş bırakma da olay üretir; yalnızca basışı al.
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
                // Hata TUI'yi düşürmez: kullanıcıya gösterilir, döngü sürer.
                last_error = Some(err.chain_text());
            } else {
                last_error = None;
            }
        }
        if quit {
            break Ok(());
        }

        // — Bir tur: parça bittiyse sıradakine geç, biriken dinlemeleri yaz.
        match live.tick().await {
            Ok(report) => {
                recorded += report.listens_recorded;
                // Depo yazamadıysa sessiz kalınmıyor: kayıtlar elde tutuldu
                // ve sonraki turda yeniden denenecek, ama kullanıcı bilsin.
                //
                // Yalnızca hata **varsa** yazılıyor: koşulsuz atama, kullanıcının
                // az önce bastığı tuşun hatasını bir sonraki turda silerdi.
                if let Some(text) = report.store_error {
                    last_error = Some(format!(
                        "{text}\n  → {} dinleme elde tutuldu",
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

/// Bir eylemi çekirdek çağrısına çevirir.
///
/// Dikkat: burada karar verilmiyor. "Duraklat mı sürdür mü" sorusunun
/// cevabı `Player::toggle_pause` içinde; TUI yalnızca iletiyor.
async fn apply(
    player: &mut Player,
    action: Action,
    selection: &mut ListState,
    queue_len: usize,
) -> tune_core::Result<()> {
    match action {
        // Çıkış çağrı döngüsünde ele alınıyor; buraya gelmez.
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

/// Çizim için gereken her şey — tek yerde toplanmış salt okunur görünüm.
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
        Constraint::Length(3), // çalan parça
        Constraint::Length(3), // ilerleme
        Constraint::Min(3),    // kuyruk
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
        PlayState::Playing => ("▶", Color::Green, "çalıyor"),
        PlayState::Paused => ("⏸", Color::Yellow, "duraklatıldı"),
        PlayState::Buffering => ("⋯", Color::Cyan, "bekliyor"),
        PlayState::Stopped => ("■", Color::DarkGray, "durdu"),
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
        Paragraph::new(line).block(Block::default().borders(Borders::ALL).title(" tune ")),
        area,
    );
}

fn draw_progress(frame: &mut ratatui::Frame<'_>, area: Rect, anchor: &PlaybackAnchor) {
    // Pozisyon çapadan **hesaplanıyor** — çekirdeğe sorulmuyor (D-015).
    let position = anchor.position_now();
    let (ratio, label) = match anchor.duration_ms {
        Some(duration) if duration > 0 => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "ilerleme çubuğu oranı; ms ölçeğinde kayıp görünmez"
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
        " kuyruk ({}) · tekrar: {} · karıştır: {} ",
        view.queue.len(),
        view.repeat,
        if view.shuffle { "açık" } else { "kapalı" }
    );
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
        selection,
    );
}

fn draw_footer(frame: &mut ratatui::Frame<'_>, area: Rect, view: &View<'_>) {
    if let Some(error) = view.error {
        // Hata ekranı kaplamıyor ama saklanmıyor da (K9): ilk satırı yeter,
        // tamamı `tune diag` ile okunur.
        let first = error.lines().next().unwrap_or(error);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {first}  (ayrıntı: tune diag)"),
                Style::default().fg(Color::Red),
            )))
            .block(Block::default().borders(Borders::ALL)),
            area,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " boşluk duraklat · n/b sonraki/önceki · ↑↓ seç · enter çal · s karıştır · r tekrar · q çık",
            Style::default().fg(Color::DarkGray),
        ))),
        area,
    );
}

/// Milisaniyeyi `3:07` biçimine çevirir.
fn clock(ms: u64) -> String {
    let total_seconds = ms / 1000;
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "bilinmeyen tuş yok sayılmalı"
        );
    }

    #[test]
    fn ctrl_c_always_quits() {
        // Terminalde beklenen davranış; 'c' tek başına bir şey yapmasa da.
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
