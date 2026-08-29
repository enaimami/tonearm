//! Oynatma çapası — durumun tek gösterimi (D-015).
//!
//! Tüketici (TUI, GUI, mobil, Faz 4'te oda) pozisyonu **kendisi hesaplar**:
//!
//! ```text
//! pos = position_ms + (now - wall_time) * rate
//! ```
//!
//! Bu yüzden çekirdek saniyede yüzlerce bildirim göndermez; tüketici ne sıklıkta
//! çizmek istiyorsa o sıklıkta çapayı okur ve aradaki zamanı kendisi doldurur.
//!
//! **Faz 4 notu:** Odaların senkron primitifi birebir bu tiptir (PLAN 4.1).
//! Bugün yerel oynatma için yazılıyor, yarın ağdan yayınlanacak — iki ayrı
//! durum modeli tutulmasın diye baştan aynı şekilde tasarlandı.

use serde::{Deserialize, Serialize};

use crate::ids::CanonicalId;

/// Oynatıcının kaba durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayState {
    /// Hiçbir şey yüklü değil.
    Stopped,
    /// Yüklü ve ilerliyor.
    Playing,
    /// Yüklü ama duraklatılmış; pozisyon donmuş.
    Paused,
    /// Yüklü, ilerlemiyor, veri bekliyor (ağ/disk).
    ///
    /// `Paused`'dan ayrı: kullanıcı istemedi, hat bekliyor. Tüketici bunu
    /// farklı göstermeli — sessizce "duraklatıldı" demek kullanıcıyı yanıltır.
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

    /// Zaman ilerliyor mu? Yalnızca `Playing`'de.
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

/// Oynatma durumunun tek gösterimi: bir zaman çapası.
///
/// `uniffi` için düz bir record — trait object, lifetime, closure yok (K7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackAnchor {
    /// Çalan parçanın kanonik kimliği. `Stopped` iken `None`.
    pub track: Option<CanonicalId>,
    /// Çapanın alındığı duvar saati.
    pub wall_time: jiff::Timestamp,
    /// Parça içindeki pozisyon, `wall_time` anında.
    pub position_ms: u64,
    /// Çalma hızı. `1.0` normal; `0.0` ilerlemiyor demek.
    ///
    /// Faz 4'te sürüklenme düzeltmesi bunu `1.001` gibi değerlere çekecek
    /// (PLAN 4.4) — bugünden alan olarak var ki o gün yüzey değişmesin.
    pub rate: f64,
    pub state: PlayState,
    /// Parçanın toplam süresi (biliniyorsa). İlerleme çubuğu için.
    pub duration_ms: Option<u64>,
}

impl PlaybackAnchor {
    /// Hiçbir şey çalmıyor.
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

    /// Verilen an için pozisyonu hesaplar.
    ///
    /// Tüketicinin yapacağı hesabın çekirdekteki karşılığı — GUI, TUI ve mobil
    /// aynı formülü üç kez yazmasın diye burada duruyor (Altın Kural).
    /// Pozisyon `duration_ms` biliniyorsa onu aşmaz.
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
            reason = "milisaniye ölçeğinde gösterim; f64 kaybı duyulmaz"
        )]
        let advanced = (elapsed as f64 * self.rate) as u64;
        self.clamp_to_duration(self.position_ms.saturating_add(advanced))
    }

    /// Şu andaki pozisyon.
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
        jiff::Timestamp::from_second(seconds).expect("test zaman damgası")
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
        // Çapadan 10 saniye sonra: 30sn + 10sn.
        assert_eq!(a.position_at(at(1010)), 40_000);
    }

    #[test]
    fn paused_position_is_frozen() {
        let a = anchor(PlayState::Paused, 30_000, 0.0);
        assert_eq!(
            a.position_at(at(1010)),
            30_000,
            "duraklatılmış ilerlememeli"
        );
    }

    #[test]
    fn buffering_does_not_advance_either() {
        // Buffering'de ses çıkmıyor; pozisyonun ilerlemesi kullanıcıyı yanıltır.
        let a = anchor(PlayState::Buffering, 30_000, 1.0);
        assert_eq!(a.position_at(at(1010)), 30_000);
    }

    #[test]
    fn rate_scales_the_elapsed_time() {
        // Faz 4 sürüklenme düzeltmesi: %10 hızlı çalarken 10sn'de 11sn ilerler.
        let a = anchor(PlayState::Playing, 0, 1.1);
        assert_eq!(a.position_at(at(1010)), 11_000);
    }

    #[test]
    fn position_never_exceeds_the_known_duration() {
        let a = anchor(PlayState::Playing, 230_000, 1.0);
        // 60 saniye sonra 290sn olurdu ama parça 240sn.
        assert_eq!(a.position_at(at(1060)), 240_000);
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_rewind() {
        let a = anchor(PlayState::Playing, 30_000, 1.0);
        assert_eq!(
            a.position_at(at(900)),
            30_000,
            "geriye giden saat pozisyonu geri sarmamalı"
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
