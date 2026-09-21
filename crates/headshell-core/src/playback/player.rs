//! Oynatıcı: kuyruk + ses motoru + scrobble (PLAN §1.5, §1.6).
//!
//! Durum burada tutulur; CLI ve GUI yalnızca [`Player::anchor`] okur (D-015).
//!
//! ## Scrobble (§1.6)
//!
//! Bir parça bittiğinde ya da bırakıldığında, [`PlayRule`]'u geçtiyse bir
//! [`Listen`] üretilir ve **import verisiyle aynı tabloya** yazılır: geçmiş
//! ve bugün tek zaman çizelgesi olur. Kural D-008'deki tek tanımdır — burada
//! ikinci bir eşik yorumu yok.

use std::sync::Arc;

use crate::diag::Stage;
use crate::error::{Error, ErrorKind, Result};
use crate::ids::{CanonicalId, ProviderId, ProviderTrackId};
use crate::model::{Listen, ListenSource, PlayRule, TrackRef};
use crate::provider::{AudioSource, Capabilities, Provider, ProviderRegistry};

use super::anchor::{PlayState, PlaybackAnchor};
use super::queue::{Queue, QueueItem};

/// Çalınmakta olan parçanın izleme kaydı.
///
/// Scrobble üretmek için gereken en az bilgi: ne, ne zaman başladı, ne kadar
/// çalındı. Süre motordan okunur — "kullanıcı ne kadar dinledi" sorusunun
/// cevabı çıkışa verilen sestir, geçen duvar saati değil (duraklatma sayılmaz).
#[derive(Debug, Clone)]
struct NowPlaying {
    item: QueueItem,
    canonical_id: Option<CanonicalId>,
    started_at: jiff::Timestamp,
    provider: ProviderId,
    /// Motordaki dilim numarası (D-024). Geçişin **duyulduğunu** buradan
    /// anlıyoruz: motor başka bir dilime geçtiyse parça değişmiş demektir.
    #[cfg(feature = "audio")]
    seq: u64,
}

/// Oynatıcı.
///
/// Ses motoru `audio` feature'ı arkasında; kapalıyken kuyruk ve çapa çalışır,
/// [`Player::play`] açık bir hata döndürür (sessizce hiçbir şey yapmaz değil).
pub struct Player {
    queue: Queue,
    providers: ProviderRegistry,
    rule: PlayRule,
    now_playing: Option<NowPlaying>,
    /// Bitmiş ama henüz toplanmamış dinleme kayıtları.
    pending_listens: Vec<Listen>,
    #[cfg(feature = "audio")]
    engine: Option<super::engine::AudioEngine>,
    /// Motora önden verilmiş sıradaki parça (D-024): dilim numarası + öğe.
    /// Geçiş duyulunca `now_playing` bu olur.
    #[cfg(feature = "audio")]
    prefetched: Option<(u64, QueueItem)>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("queue_len", &self.queue.len())
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Sağlayıcı kayıt defteriyle bir oynatıcı kurar.
    #[must_use]
    pub fn new(providers: ProviderRegistry) -> Self {
        Self {
            queue: Queue::new(),
            providers,
            rule: PlayRule::default(),
            now_playing: None,
            pending_listens: Vec::new(),
            #[cfg(feature = "audio")]
            engine: None,
            #[cfg(feature = "audio")]
            prefetched: None,
        }
    }

    /// "Sayılan çalma" kuralını değiştirir (D-008'deki tek tanım).
    #[must_use]
    pub fn with_play_rule(mut self, rule: PlayRule) -> Self {
        self.rule = rule;
        self
    }

    /// Kuyruk (okuma).
    #[must_use]
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Kuyruk (yazma). Değişiklik çalan parçayı durdurmaz.
    pub fn queue_mut(&mut self) -> &mut Queue {
        &mut self.queue
    }

    /// Şu anki durum.
    #[must_use]
    pub fn state(&self) -> PlayState {
        #[cfg(feature = "audio")]
        {
            if let Some(engine) = &self.engine {
                return engine.state();
            }
        }
        PlayState::Stopped
    }

    /// Durumun tek gösterimi (D-015). Tüketici pozisyonu bundan hesaplar.
    #[must_use]
    pub fn anchor(&self) -> PlaybackAnchor {
        let state = self.state();
        #[cfg(feature = "audio")]
        let (position_ms, duration_ms) = self
            .engine
            .as_ref()
            .map_or((0, None), |e| (e.position_ms(), e.duration_ms()));
        #[cfg(not(feature = "audio"))]
        let (position_ms, duration_ms) = (0, None);

        PlaybackAnchor {
            track: self
                .now_playing
                .as_ref()
                .and_then(|np| np.canonical_id.clone()),
            wall_time: jiff::Timestamp::now(),
            position_ms,
            rate: if state.advances() { 1.0 } else { 0.0 },
            state,
            duration_ms: duration_ms.or_else(|| {
                self.now_playing
                    .as_ref()
                    .and_then(|np| np.item.track.duration_ms)
            }),
        }
    }

    /// Çalan parçanın üstverisi.
    #[must_use]
    pub fn current_track(&self) -> Option<&TrackRef> {
        self.now_playing.as_ref().map(|np| &np.item.track)
    }

    /// Kuyruğu değiştirir ve baştan çalmaya başlar.
    ///
    /// # Errors
    /// Kuyruk boşsa ya da ilk parça çalınamazsa.
    pub async fn play_items(&mut self, items: Vec<QueueItem>) -> Result<()> {
        if items.is_empty() {
            return Err(Error::new(
                Stage::PlaybackResolve,
                ErrorKind::InvalidInput {
                    detail: "çalınacak parça yok".to_owned(),
                },
            ));
        }
        self.queue.replace(items);
        self.start_current().await
    }

    /// Kuyruktaki geçerli parçayı çalar.
    ///
    /// # Errors
    /// Kuyruk boşsa, sağlayıcı bulunamazsa ya da ses hattı kurulamazsa.
    pub async fn start_current(&mut self) -> Result<()> {
        let item = self.queue.current().cloned().ok_or_else(|| {
            Error::new(
                Stage::PlaybackResolve,
                ErrorKind::NotFound {
                    what: "kuyrukta çalınacak parça".to_owned(),
                },
            )
        })?;

        // Çalan parçayı kapat: yarım kalan dinleme kaydı üretilsin.
        self.finish_current_listen();

        let source = self.resolve_source(&item.id).await?;
        self.start_source(&source, item)?;
        Ok(())
    }

    /// Sağlayıcıdan çalınabilir kaynağı ister.
    async fn resolve_source(&self, id: &ProviderTrackId) -> Result<AudioSource> {
        let provider = self.providers.get(&id.provider).ok_or_else(|| {
            Error::new(
                Stage::PlaybackResolve,
                ErrorKind::NotFound {
                    what: format!("sağlayıcı: {}", id.provider),
                },
            )
        })?;

        let info = provider.info();
        // Yeteneği olmayan sağlayıcıdan akış istemek "sonuç yok" değil,
        // açık bir "yapamıyorum"dur (K9).
        if !info.capabilities.contains(Capabilities::STREAM) {
            return Err(Error::new(
                Stage::PlaybackResolve,
                ErrorKind::Unsupported {
                    provider: info.id.to_string(),
                    what: "ses akışı".to_owned(),
                    capabilities: info.capabilities.describe(),
                },
            ));
        }

        provider.resolve_source(id).await?.ok_or_else(|| {
            Error::new(
                Stage::PlaybackResolve,
                ErrorKind::NotFound {
                    what: format!("{id} için çalınabilir kaynak"),
                },
            )
        })
    }

    /// Kaynağı ses hattına verir.
    ///
    /// Ses hattı kapalı derlemede argümanlar aşağıdaki `let _ = (source, item);`
    /// ile tüketiliyor; ayrıca bir `unused_variables` beklentisi **koymuyoruz**,
    /// çünkü lint hiç tetiklenmiyor ve karşılanmayan beklentinin kendisi hata
    /// oluyordu (`cargo clippy -p headshell-core`).
    fn start_source(&mut self, source: &AudioSource, item: QueueItem) -> Result<()> {
        #[cfg(feature = "audio")]
        {
            // Kullanıcı isteğiyle başlangıç: motoru **yeniden** kuruyoruz.
            // Gapless yalnızca doğal geçiş içindir; kullanıcı tuşa bastığında
            // önden okunmuş sesi çalmak yanlış parçayı duyurmak olurdu.
            let engine = super::engine::AudioEngine::open()?;
            let seq = engine.play_source(source)?;
            self.engine = Some(engine);
            self.prefetched = None;

            self.now_playing = Some(NowPlaying {
                provider: item.id.provider.clone(),
                canonical_id: None,
                started_at: jiff::Timestamp::now(),
                item,
                seq,
            });
            Ok(())
        }
        #[cfg(not(feature = "audio"))]
        {
            let _ = (source, item);
            Err(Error::new(
                Stage::PlaybackOutput,
                ErrorKind::Audio {
                    detail: "bu derlemede ses hattı yok (`audio` feature'ı kapalı)".to_owned(),
                },
            ))
        }
    }

    /// Duraklatır.
    pub fn pause(&self) {
        #[cfg(feature = "audio")]
        if let Some(engine) = &self.engine {
            engine.pause();
        }
    }

    /// Sürdürür.
    pub fn resume(&self) {
        #[cfg(feature = "audio")]
        if let Some(engine) = &self.engine {
            engine.resume();
        }
    }

    /// Çalmayı bırakır ve dinleme kaydını kapatır.
    pub fn stop(&mut self) {
        self.finish_current_listen();
        #[cfg(feature = "audio")]
        {
            self.engine = None;
        }
    }

    /// Kullanıcı isteğiyle sıradaki parçaya geçer.
    ///
    /// # Errors
    /// Sıradaki parça çalınamazsa.
    pub async fn next(&mut self) -> Result<bool> {
        if self.queue.next().is_none() {
            self.stop();
            return Ok(false);
        }
        self.start_current().await?;
        Ok(true)
    }

    /// Önceki parçaya döner.
    ///
    /// # Errors
    /// Önceki parça çalınamazsa.
    pub async fn previous(&mut self) -> Result<bool> {
        if self.queue.previous().is_none() {
            return Ok(false);
        }
        self.start_current().await?;
        Ok(true)
    }

    /// Kuyrukta belirli bir konuma atlayıp çalar.
    ///
    /// TUI/GUI'nin "listeden seç" davranışı. Konum geçersizse `false` döner
    /// ve çalan parça bozulmaz.
    ///
    /// # Errors
    /// Seçilen parça çalınamazsa.
    pub async fn jump_to(&mut self, position: usize) -> Result<bool> {
        if !self.queue.jump_to(position) {
            return Ok(false);
        }
        self.start_current().await?;
        Ok(true)
    }

    /// Duraklatılmışsa sürdürür, çalıyorsa duraklatır.
    ///
    /// Tek tuşla kumanda eden arayüzler için; durum mantığı burada dursun ki
    /// TUI ve GUI aynı kararı iki kez vermesin.
    pub fn toggle_pause(&self) {
        match self.state() {
            PlayState::Playing | PlayState::Buffering => self.pause(),
            PlayState::Paused => self.resume(),
            PlayState::Stopped => {}
        }
    }

    /// Parça doğal olarak bittiyse sıradakine geçer.
    ///
    /// Çağıranın (TUI döngüsü, GUI zamanlayıcı) düzenli çağırması beklenir.
    /// `RepeatMode::One` burada aynı parçayı yeniden başlatır — kullanıcı
    /// isteğiyle geçişten ayrıldığı tek yer (bkz. [`Queue::advance_after_finish`]).
    ///
    /// # Errors
    /// Sıradaki parça çalınamazsa.
    pub async fn tick(&mut self) -> Result<()> {
        #[cfg(feature = "audio")]
        {
            let Some((current_seq, finished)) = self
                .engine
                .as_ref()
                .map(|engine| (engine.current_seq(), engine.finished()))
            else {
                return Ok(());
            };

            // — 1. Geçiş **duyuldu** mu? Motor önden verdiğimiz dilime
            // geçtiyse parça değişmiş demektir; ses hiç kesilmedi.
            if let Some((seq, item)) = self.prefetched.clone()
                && current_seq == Some(seq)
            {
                self.finish_current_listen();
                self.queue.advance_after_finish();
                self.now_playing = Some(NowPlaying {
                    provider: item.id.provider.clone(),
                    canonical_id: None,
                    started_at: jiff::Timestamp::now(),
                    item,
                    seq,
                });
                self.prefetched = None;
            }

            // — 2. Çalacak bir şey kaldı mı?
            if finished {
                if let Some(engine) = &self.engine
                    && let Some(detail) = engine.take_error()
                {
                    // Çözme hatasını yutma — tanıya taşınsın (K9).
                    tracing::warn!(hata = %detail, "çalma sırasında hata");
                }
                self.finish_current_listen();
                // Önden okuma yapılamamışsa (sağlayıcı hatası, kuyruk sonunda
                // sarma) eski yola düşüyoruz: motoru yeniden kur. Boşluk olur
                // ama çalma durmaz.
                if self.queue.advance_after_finish().is_some() {
                    return self.start_current().await;
                }
                self.engine = None;
                self.prefetched = None;
                return Ok(());
            }

            // — 3. Sıradakini önden çöz (gapless'ın olduğu yer).
            self.prefetch_next().await;
        }
        Ok(())
    }

    /// Sıradaki parçayı, bugünkü hâlâ çalarken motora verir (D-024).
    ///
    /// Hata **çalmayı düşürmez**: önden okuma bir iyileştirmedir, geçiş
    /// olmazsa `tick` eski yoldan devam eder. Ama hata sessiz de kalmaz.
    #[cfg(feature = "audio")]
    async fn prefetch_next(&mut self) {
        if self.prefetched.is_some() {
            return;
        }
        let Some(engine) = &self.engine else { return };
        // Motorda hâlâ bekleyen iş varsa erken: tampon zaten dolu.
        if engine.queued_len() > 0 {
            return;
        }
        let Some(item) = self.queue.peek_after_finish().cloned() else {
            return;
        };

        match self.resolve_source(&item.id).await {
            Ok(source) => {
                if let Some(engine) = &self.engine {
                    let seq = engine.enqueue(&source);
                    self.prefetched = Some((seq, item));
                }
            }
            Err(err) => {
                tracing::warn!(
                    parca = item.track.display_name(),
                    hata = %err.chain_text().replace('\n', " "),
                    "sıradaki parça önden çözülemedi; geçişte boşluk olabilir"
                );
            }
        }
    }

    /// Biriken dinleme kayıtlarını alır ve listeyi boşaltır.
    ///
    /// Çağıran bunları kütüphaneye yazar — import verisiyle **aynı tabloya**
    /// (§1.6): geçmiş ve bugün tek zaman çizelgesi olur.
    #[must_use]
    pub fn take_listens(&mut self) -> Vec<Listen> {
        std::mem::take(&mut self.pending_listens)
    }

    /// Çalan parçanın dinleme kaydını kapatır.
    ///
    /// Eşiği geçmeyen çalmalar **kayıt üretmez** ama kaybolmaz: `PlayRule`
    /// zaten "sayılmaz" diyor ve bu bilinçli bir eleme (D-008).
    fn finish_current_listen(&mut self) {
        let Some(now_playing) = self.now_playing.take() else {
            return;
        };

        // Dilimin kendi süresi: geçiş olduysa çıkış artık sıradaki parçada
        // ve `position_ms()` onu gösterir. Biten parçanın scrobble'ı kendi
        // dilimini sormalı (D-024).
        #[cfg(feature = "audio")]
        let ms_played = self.engine.as_ref().map_or(0, |engine| {
            engine
                .played_ms_of(now_playing.seq)
                .unwrap_or_else(|| engine.position_ms())
        });
        #[cfg(not(feature = "audio"))]
        let ms_played = 0u64;

        // Süreyi önce **motordan** soruyoruz: katalog etiketten besleniyor ve
        // etiketsiz dosyada `None` kalıyor. Süre bilinmeyince `PlayRule`'un
        // "parçanın yarısı" kolu çalışamıyor, kural 30 sn eşiğine düşüyor ve
        // baştan sona dinlenmiş kısa bir parça scrobble üretmiyordu (D-008'in
        // kuralı değişmedi; ona verilen veri düzeldi).
        #[cfg(feature = "audio")]
        let duration_ms = self
            .engine
            .as_ref()
            .and_then(|engine| engine.duration_of(now_playing.seq))
            .or(now_playing.item.track.duration_ms);
        #[cfg(not(feature = "audio"))]
        let duration_ms = now_playing.item.track.duration_ms;

        if !self.rule.counts(ms_played, duration_ms) {
            tracing::debug!(
                parca = now_playing.item.track.display_name(),
                ms_played,
                "eşiğin altında kaldı, scrobble üretilmedi"
            );
            return;
        }

        // Ölçülen süre kayda da giriyor. Yoksa buraya "kaydedildi" diye yazılan
        // dinleme, `stats` kuralı yeniden uyguladığında elenirdi: CLI "4
        // dinleme" der, istatistik 2 gösterirdi. Kaptan okunan süre etiketten
        // gelenden daha güvenilir bir ölçüm.
        let mut track = now_playing.item.track;
        track.duration_ms = duration_ms;

        self.pending_listens.push(Listen {
            track,
            played_at: now_playing.started_at,
            ms_played,
            source: ListenSource::Playback {
                provider: now_playing.provider,
            },
            canonical_id: now_playing.canonical_id,
        });
    }
}

/// Bir sağlayıcıyı kayıt defterine ekleyip oynatıcı kurar (kolaylık).
#[must_use]
pub fn with_provider(provider: Arc<dyn Provider>) -> Player {
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    Player::new(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderFuture, ProviderHealth, ProviderInfo, ProviderTrack};

    /// Yeteneği olmayan sahte sağlayıcı: `STREAM` yok.
    struct ControlOnlyProvider;

    impl Provider for ControlOnlyProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: ProviderId::new("uzak"),
                display_name: "Yalnızca kumanda".to_owned(),
                capabilities: Capabilities::CONTROL,
            }
        }
        fn health<'a>(&'a self) -> ProviderFuture<'a, ProviderHealth> {
            Box::pin(async move {
                Ok(ProviderHealth {
                    id: ProviderId::new("uzak"),
                    reachable: true,
                    track_count: None,
                    detail: None,
                })
            })
        }
        fn search<'a>(
            &'a self,
            _query: &'a str,
            _limit: usize,
        ) -> ProviderFuture<'a, Vec<ProviderTrack>> {
            Box::pin(async move { Ok(Vec::new()) })
        }
        fn resolve_source<'a>(
            &'a self,
            _id: &'a ProviderTrackId,
        ) -> ProviderFuture<'a, Option<AudioSource>> {
            Box::pin(async move { Ok(None) })
        }
    }

    fn item(provider: &str, id: &str) -> QueueItem {
        QueueItem {
            id: ProviderTrackId::new(ProviderId::new(provider), id),
            track: TrackRef::new("Artist", "Track"),
        }
    }

    #[tokio::test]
    async fn streaming_from_a_control_only_provider_is_an_explicit_refusal() {
        let mut player = with_provider(Arc::new(ControlOnlyProvider));
        let err = player
            .play_items(vec![item("uzak", "1")])
            .await
            .expect_err("STREAM yeteneği yokken çalınmamalı");

        assert_eq!(err.stage(), Stage::PlaybackResolve);
        let text = err.chain_text();
        assert!(text.contains("yapamıyor"), "{text}");
        assert!(text.contains("CONTROL"), "yetenekler görünmeli: {text}");
    }

    #[tokio::test]
    async fn an_unknown_provider_is_named_in_the_error() {
        let mut player = Player::new(ProviderRegistry::new());
        let err = player
            .play_items(vec![item("olmayan", "1")])
            .await
            .expect_err("kayıtlı olmayan sağlayıcı");
        assert!(err.chain_text().contains("olmayan"), "{}", err.chain_text());
    }

    #[tokio::test]
    async fn playing_an_empty_list_is_rejected() {
        let mut player = Player::new(ProviderRegistry::new());
        let err = player.play_items(vec![]).await.expect_err("boş liste");
        assert!(err.chain_text().contains("çalınacak parça yok"));
    }

    #[test]
    fn a_fresh_player_reports_a_stopped_anchor() {
        let player = Player::new(ProviderRegistry::new());
        let anchor = player.anchor();
        assert_eq!(anchor.state, PlayState::Stopped);
        assert_eq!(anchor.track, None);
        assert_eq!(anchor.rate, 0.0, "durmuşken zaman ilerlememeli");
    }

    #[test]
    fn listens_are_only_produced_above_the_play_rule() {
        let mut player = Player::new(ProviderRegistry::new());
        // Elle bir "çalan parça" kur; motor yok, ms_played 0 olacak.
        player.now_playing = Some(NowPlaying {
            item: item("local", "x"),
            canonical_id: None,
            started_at: jiff::Timestamp::UNIX_EPOCH,
            provider: ProviderId::new("local"),
            #[cfg(feature = "audio")]
            seq: 0,
        });
        player.finish_current_listen();
        assert!(
            player.take_listens().is_empty(),
            "0 ms çalan parça scrobble üretmemeli"
        );
    }
}
