//! Çalan bir oturum: `Player` + `Session` tek yerde bağlanır.
//!
//! **Neden çekirdekte (K1):** her kabuk aynı dansı yapıyordu — düzenli
//! `tick()`, sonra biriken dinlemeleri depoya yazmak. TUI bunu bir kez yazdı,
//! GUI ikinci, mobil üçüncü kez yazacaktı. Dinlemeyi depoya yazmayı unutan bir
//! kabuk **sessizce geçmiş kaybeder** — kaybı fark ettiren hiçbir şey yok.
//!
//! **D-015'e sadık:** observer/callback yok. Kabuk döngüyü kendi sürer,
//! her turda bir `TickReport` alır ve pozisyonu çapadan tahmin eder.
//! **K7'ye sadık:** kapanış (closure) parametresi, generic, ömür sızıntısı yok.

use serde::{Deserialize, Serialize};

use crate::library::WriteSummary;
use crate::model::Listen;
use crate::playback::{PlayState, PlaybackAnchor, Player};
use crate::session::Session;
use crate::{Error, Result};

/// Bir `tick`'te ne olduğu.
///
/// Kabuk buna bakıp yeniden çizer. Pozisyon burada **yok**: o `anchor`'dan
/// tahmin edilir, yoksa saniyede onlarca kez sorulması gerekirdi.
///
/// `Serialize`: GUI bunu webview'e olduğu gibi gönderiyor (§3.2).
/// Ayrı bir "IPC tipi" yazılmıyor — çevirmen katmanı iki tipi kaydırırdı.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickReport {
    /// Her zaman dolu. Kabuk pozisyonu bundan hesaplar
    /// (`PlaybackAnchor::position_at`).
    pub anchor: PlaybackAnchor,
    /// Bu turda çalan parça değişti mi.
    pub track_changed: bool,
    /// Bu turda depoya **yazılan** dinleme sayısı.
    pub listens_recorded: usize,
    /// Yazılamayıp elde tutulan dinleme sayısı.
    ///
    /// Sıfırdan büyükse depo yazma hatası vermiş demektir; kayıtlar
    /// **atılmadı**, sonraki turda yeniden denenecek. Kabuk bunu göstermeli.
    pub listens_pending: usize,
    /// Depo yazması bu turda başarısız olduysa hata zinciri.
    ///
    /// `tick` bu yüzden `Err` dönmez: ses çalmaya devam ediyor, oturumu
    /// düşürmek veri kaybını artırırdı. Ama sessiz de kalınmıyor (K9).
    pub store_error: Option<String>,
    /// Kuyruk tükendi ve ses durdu.
    pub finished: bool,
}

/// Çalan oturum.
///
/// `Session` (kütüphane, istatistik, tanılama) ile `Player` (ses, kuyruk)
/// burada birlikte yaşar. İkisine erişim açık kalır: arama ve istatistik
/// çalarken de gerekiyor.
pub struct LiveSession {
    session: Session,
    player: Player,
    /// Depoya yazılamamış dinlemeler. `take_listens` onları oynatıcıdan
    /// çekip aldığı için, yazma başarısız olursa **burada tutulmazlarsa
    /// kaybolurlar**.
    pending: Vec<Listen>,
}

impl LiveSession {
    /// Bir oturum ile bir oynatıcıyı bağlar.
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

    /// Şu anki çapa.
    #[must_use]
    pub fn anchor(&self) -> PlaybackAnchor {
        self.player.anchor()
    }

    /// Çalan oynatıcıyı yenisiyle değiştirir (yeni bir kuyruk başlatmak).
    ///
    /// **Eski oynatıcının biriken dinlemeleri alınır.** Doğrudan
    /// `*live.player_mut() = yeni` yazmak eski oynatıcıyı `take_listens`
    /// çağrılmadan düşürürdü: kullanıcı yeni bir arama yaptığı anda az önce
    /// dinlediği parça sessizce kaybolurdu. Kayıtlar buradan `pending`'e
    /// geçer, ilk `tick` onları yazar.
    ///
    /// Eski ses hattı kapanır — `Player` düşerken durur.
    pub fn replace_player(&mut self, player: Player) {
        self.player.stop();
        self.pending.extend(self.player.take_listens());
        self.player = player;
    }

    /// Bir tur: oynatıcıyı ilerlet, biriken dinlemeleri **hemen** yaz.
    ///
    /// Yazma her turda yapılıyor, çıkışta değil. Saatlerce açık kalan bir
    /// arayüzde çıkışta yazmak, çökme ya da `kill` durumunda bütün oturumun
    /// geçmişini götürürdü. Çoğu turda yazılacak bir şey olmaz — `take_listens`
    /// boş döner ve depoya hiç gidilmez.
    ///
    /// # Errors
    /// Oynatıcı ilerlerken hata verirse (kaynak açılamadı, kod çözülemedi).
    /// **Depo yazma hatası `Err` üretmez** — `TickReport::store_error`'a düşer.
    pub async fn tick(&mut self) -> Result<TickReport> {
        // Kuyruk konumu da bakılıyor: aynı parça kuyrukta iki kez olabilir,
        // yalnızca `TrackRef` karşılaştırmak aralarındaki geçişi kaçırırdı.
        //
        // Yakalamadığı durum bilerek yazılıyor: `RepeatMode::One` aynı parçayı
        // baştan başlattığında ne parça ne konum değişir, `track_changed`
        // `false` kalır. Kabuk için doğru olan da bu — gösterilen parça aynı;
        // baştan başladığını çapanın pozisyonu, yeni dinlemeyi
        // `listens_recorded` söylüyor.
        let before = (
            self.player.current_track().cloned(),
            self.player.queue().position(),
        );

        let tick_result = self.player.tick().await;

        // Oynatıcı hata verse bile biriken dinlemeler yazılmalı: hata
        // bir parçaya ait, geçmiş bütün oturuma.
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

    /// Oturumu kapatır: sesi durdurur, kalan her dinlemeyi yazar.
    ///
    /// # Errors
    /// Son yazma başarısız olursa. Bu durumda kayıtlar **hâlâ elde**;
    /// `pending_listens()` kaç tanesinin yazılamadığını söyler.
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

    /// Depoya yazılamamış dinleme sayısı.
    #[must_use]
    pub fn pending_listens(&self) -> usize {
        self.pending.len()
    }

    /// Biriken dinlemeleri yazmayı dener. Yazamazsa **elde tutar**.
    fn flush(&mut self) -> (usize, Option<String>) {
        self.pending.extend(self.player.take_listens());
        if self.pending.is_empty() {
            return (0, None);
        }
        let outcome = self.session.record_listens(&self.pending);
        absorb(&mut self.pending, outcome)
    }
}

/// Yazma sonucunu tampona uygular.
///
/// Ayrı bir fonksiyon, çünkü asıl iddia **başarısızlık yolunda**: yazılamayan
/// kayıt atılmaz, elde kalır. Bunu gerçek bir bozuk depoyla sınamak denendi ve
/// güvenilir olmadı — SQLite açık dosya tanıtıcısıyla salt-okunur dizinde bile
/// yazmayı sürdürüyor. Koşulu sağlanamayan bir test yeşil yanar ve hiçbir şey
/// kanıtlamaz; karar buraya çıkarıldı ki doğrudan sınanabilsin.
fn absorb(pending: &mut Vec<Listen>, outcome: Result<WriteSummary>) -> (usize, Option<String>) {
    match outcome {
        Ok(summary) => {
            pending.clear();
            (summary.inserted, None)
        }
        // Kayıtlar `pending`'de kalıyor: sonraki tur yeniden denenecek.
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

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("tune-live-{label}-{unique}"));
        std::fs::create_dir_all(&dir).expect("geçici dizin");
        dir
    }

    fn live_at(dir: &std::path::Path) -> LiveSession {
        let session = Session::open(Config::with_data_dir(dir)).expect("oturum açılmalı");
        LiveSession::new(session, Player::new(ProviderRegistry::new()))
    }

    fn listen(title: &str) -> Listen {
        Listen {
            track: TrackRef::new("Artist", title),
            played_at: jiff::Timestamp::now(),
            ms_played: 200_000,
            source: ListenSource::Playback {
                provider: ProviderId::new("yerel"),
            },
            canonical_id: None,
        }
    }

    #[tokio::test]
    async fn an_idle_session_reports_finished_without_inventing_listens() {
        let dir = temp_dir("bosta");
        let mut live = live_at(&dir);

        let report = live.tick().await.expect("boşta tick hata vermemeli");

        assert!(report.finished, "kuyruk boş ve ses durmuş");
        assert!(!report.track_changed);
        assert_eq!(report.listens_recorded, 0);
        assert_eq!(report.listens_pending, 0);
        assert!(report.store_error.is_none());
        assert_eq!(report.anchor.state, PlayState::Stopped);
    }

    #[test]
    fn flushing_writes_listens_and_empties_the_buffer() {
        let dir = temp_dir("yaz");
        let mut live = live_at(&dir);
        live.pending.push(listen("Bir"));
        live.pending.push(listen("İki"));

        let (recorded, error) = live.flush();

        assert_eq!(recorded, 2, "ikisi de yazılmalı");
        assert!(error.is_none(), "{error:?}");
        assert_eq!(live.pending_listens(), 0, "tampon boşalmalı");
    }

    /// Asıl iddia: **depo yazamazsa dinleme kaybolmaz.**
    ///
    /// `take_listens` kayıtları oynatıcıdan çekip alıyor; yazma başarısız olur
    /// ve elde tutulmazlarsa geri alınacakları bir yer yok. Eskiden yazma
    /// yalnızca çıkışta yapılıyordu, yani tek bir hata bütün oturumu götürürdü.
    #[test]
    fn a_failing_store_keeps_the_listens_instead_of_dropping_them() {
        let mut pending = vec![listen("Kaybolmamalı"), listen("Bu da")];

        let (recorded, error) = absorb(
            &mut pending,
            Err(Error::new(
                Stage::LibraryWrite,
                ErrorKind::InvalidInput {
                    detail: "disk dolu".to_owned(),
                },
            )),
        );

        assert_eq!(recorded, 0, "hiçbiri yazılmadı");
        assert_eq!(pending.len(), 2, "kayıtlar ELDE kalmalı, atılmamalı");
        let text = error.expect("hata bildirilmeli, yutulmamalı");
        assert!(
            text.starts_with("ADIM: "),
            "aşama bildirilmeli (K9): {text}"
        );
        assert!(text.contains("disk dolu"), "sebep görünmeli: {text}");
    }

    #[test]
    fn a_successful_store_empties_the_buffer() {
        let mut pending = vec![listen("Yazıldı")];

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
        assert!(pending.is_empty(), "yazılan kayıt tamponda kalmamalı");
    }

    /// Yeni bir kuyruk başlatmak, önceki turdan kalan kayıtları silmemeli.
    ///
    /// Bu testin **kapsamadığı** kısım bilerek yazılıyor: eski oynatıcının
    /// kendi `pending_listens`'i buraya aktarılıyor mu? `Player`'ın alanı
    /// özel ve dinleme üretmek gerçek bir ses hattı ister; o yol
    /// `tests/playback_local.rs` ile sınanıyor. Burada sınanan, tamponun
    /// değiştirme sırasında **atılmadığı**.
    #[test]
    fn replacing_the_player_swaps_the_queue_and_keeps_pending_listens() {
        use crate::ids::ProviderTrackId;
        use crate::playback::QueueItem;

        let dir = temp_dir("degistir");
        let mut live = live_at(&dir);
        live.pending.push(listen("Önceki turdan kalan"));

        let mut yeni = Player::new(ProviderRegistry::new());
        yeni.queue_mut().replace(vec![QueueItem {
            id: ProviderTrackId::new(ProviderId::new("yerel"), "1"),
            track: TrackRef::new("Artist", "New queue"),
        }]);

        live.replace_player(yeni);

        assert_eq!(live.player().queue().len(), 1, "yeni kuyruk geçerli olmalı");
        assert_eq!(live.pending_listens(), 1, "eski tampon atılmamalı");
    }

    #[test]
    fn shutdown_writes_what_is_still_pending() {
        let dir = temp_dir("kapat");
        let mut live = live_at(&dir);
        live.pending.push(listen("Son"));

        let summary = live.shutdown().expect("kapanış yazabilmeli");

        assert_eq!(summary.inserted, 1);
        assert_eq!(live.pending_listens(), 0);
    }
}
