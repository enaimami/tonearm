//! Oynatma (Faz 1).
//!
//! ## Katmanlar
//!
//! - [`anchor`] — durumun tek gösterimi: [`PlaybackAnchor`] (D-015).
//!   Tüketici pozisyonu çapadan kendisi hesaplar; çekirdek bildirim yağdırmaz.
//! - [`queue`] — kuyruk, tekrar, karıştırma. Saf veri yapısı.
//! - `engine` — symphonia (çözme) + cpal (çıkış). **`audio` feature'ı
//!   arkasında** (D-016): kapalıyken kuyruk ve çapa yine derlenir, yalnızca
//!   gerçek ses çıkışı düşer. Sunucu ve mobil derlemeleri ALSA'ya bağlanmasın.
//!
//! ## Neden çapa
//!
//! Faz 4'ün oda senkron primitifi birebir [`PlaybackAnchor`]. Bugün yerel
//! oynatma için yazılıyor, yarın ağdan yayınlanacak — iki ayrı durum modeli
//! tutmamak için baştan aynı tip.

pub mod anchor;
#[cfg(feature = "audio")]
pub mod engine;
/// Uzak akışı symphonia'ya bağlayan ilerlemeli okuyucu (§1.3).
/// Hem ses hattı hem HTTP istemcisi açıkken derlenir.
#[cfg(all(feature = "audio", feature = "http-client"))]
pub mod http_source;
pub mod player;
pub mod queue;

pub use anchor::{PlayState, PlaybackAnchor};
#[cfg(feature = "audio")]
pub use engine::AudioEngine;
pub use player::Player;
pub use queue::{Queue, QueueItem, RepeatMode};
