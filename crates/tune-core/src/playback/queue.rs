//! Çalma kuyruğu (PLAN §1.5).
//!
//! Durum çekirdekte tutulur; CLI ve GUI yalnızca gösterir. Kuyruk saf bir
//! veri yapısıdır — ses hattını bilmez, `audio` feature'ından bağımsız
//! derlenir ve testleri ses aygıtı gerektirmez.

use serde::{Deserialize, Serialize};

use crate::ids::ProviderTrackId;
use crate::model::TrackRef;

/// Kuyruktaki bir öğe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueItem {
    /// Hangi sağlayıcıdan, hangi parça.
    pub id: ProviderTrackId,
    pub track: TrackRef,
}

/// Tekrar kipi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    /// Kuyruk bitince durur.
    #[default]
    Off,
    /// Kuyruk bitince başa döner.
    All,
    /// Aynı parçayı tekrarlar.
    One,
}

impl RepeatMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }
}

impl std::fmt::Display for RepeatMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Kuyruğun okunabilir görünümü — **çalma sırasına göre**.
///
/// Kabukların (TUI, GUI) tek seferde alıp çizdiği şey. Ayrı bir "IPC tipi"
/// değil: çekirdekte yaşıyor, `serde` ile olduğu gibi geçiyor ve TUI de
/// aynısını kullanıyor (D-033).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueueView {
    /// Öğeler, çalma sırasında.
    pub items: Vec<QueueItem>,
    /// `items` içindeki çalan konum. Kuyruk boşsa anlamsız.
    pub position: usize,
    pub repeat: RepeatMode,
    pub shuffle: bool,
}

/// Çalma kuyruğu ve içindeki konum.
///
/// Karıştırma **sırayı bozmaz**, ayrı bir çalma sırası tutar: karıştırmayı
/// kapatınca kullanıcı listesini kaybetmez ve "sıradaki ne" sorusu iki kipte
/// de aynı yerden cevaplanır.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    items: Vec<QueueItem>,
    /// `order[position]` → `items` indeksi.
    order: Vec<usize>,
    /// `order` içindeki konum. Kuyruk boşsa anlamsız.
    position: usize,
    repeat: RepeatMode,
    shuffle: bool,
    /// Karıştırma için deterministik üreteç durumu (test edilebilirlik).
    rng_state: u64,
}

impl Queue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            rng_state: 0x2545_F491_4F6C_DD1D,
            ..Self::default()
        }
    }

    /// Kuyruğu verilen parçalarla değiştirir ve başa alır.
    pub fn replace(&mut self, items: Vec<QueueItem>) {
        self.items = items;
        self.order = (0..self.items.len()).collect();
        self.position = 0;
        if self.shuffle {
            self.reshuffle();
        }
    }

    /// Kuyruğun sonuna ekler.
    pub fn append(&mut self, items: impl IntoIterator<Item = QueueItem>) {
        for item in items {
            self.items.push(item);
            self.order.push(self.items.len() - 1);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.order.clear();
        self.position = 0;
    }

    /// Çalma sırasına göre öğeler.
    #[must_use]
    pub fn items(&self) -> Vec<QueueItem> {
        self.order
            .iter()
            .filter_map(|&index| self.items.get(index).cloned())
            .collect()
    }

    /// Kuyruğun dışarıya gösterilen hâli.
    ///
    /// `Queue`'nun kendisi kabuğa gönderilmiyor: içinde `order` ve
    /// `rng_state` var, yani tüketici çalma sırasını **kendisi** kurmak
    /// zorunda kalırdı — sıralama mantığının ikinci bir kopyası JS'te yaşardı.
    /// Bu görünüm sırayı zaten uygulanmış verir.
    #[must_use]
    pub fn view(&self) -> QueueView {
        QueueView {
            items: self.items(),
            position: self.position,
            repeat: self.repeat,
            shuffle: self.shuffle,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Şu an çalması gereken öğe.
    #[must_use]
    pub fn current(&self) -> Option<&QueueItem> {
        let index = *self.order.get(self.position)?;
        self.items.get(index)
    }

    /// Çalma sırasındaki konum (0 tabanlı).
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    #[must_use]
    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    #[must_use]
    pub const fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Karıştırmayı açar/kapatır.
    ///
    /// Açarken **çalan parça yerinde kalır** — altından çekilmez; kalanlar
    /// karıştırılır. Kapatırken özgün sıraya dönülür ve konum çalan parçaya
    /// göre düzeltilir.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if shuffle == self.shuffle {
            return;
        }
        let current_index = self.order.get(self.position).copied();
        self.shuffle = shuffle;
        if shuffle {
            self.reshuffle();
        } else {
            self.order = (0..self.items.len()).collect();
        }
        // Konumu çalan parçanın yeni sırasına taşı.
        if let Some(index) = current_index {
            if let Some(new_position) = self.order.iter().position(|&i| i == index) {
                self.position = new_position;
            }
        }
    }

    /// Belirli bir konuma atlar.
    ///
    /// Sınır dışıysa `false` döner ve kuyruk değişmez — sessizce başa
    /// sarmaz, çünkü çağıran hata yaptığını bilmeli.
    pub fn jump_to(&mut self, position: usize) -> bool {
        if position >= self.order.len() {
            return false;
        }
        self.position = position;
        true
    }

    /// Sıradaki parçaya geçer.
    ///
    /// `RepeatMode::One` **kendiliğinden tekrar etmez** — bu, parça doğal
    /// olarak bittiğinde [`Queue::advance_after_finish`] ile ayrılır.
    /// Kullanıcı "sonraki" derse tekrar kipinde de sıradakine geçilir;
    /// aksi halde tuş çalışmıyormuş gibi görünür.
    #[expect(
        clippy::should_implement_trait,
        reason = "kuyruk bir iterator değil: konumu vardır, geriye de gider (previous). \
                  `next` burada kullanıcının bastığı tuşun adı."
    )]
    pub fn next(&mut self) -> Option<&QueueItem> {
        if self.order.is_empty() {
            return None;
        }
        if self.position + 1 < self.order.len() {
            self.position += 1;
        } else if self.repeat == RepeatMode::All {
            self.position = 0;
            if self.shuffle {
                self.reshuffle();
            }
        } else {
            return None;
        }
        self.current()
    }

    /// Önceki parçaya döner.
    pub fn previous(&mut self) -> Option<&QueueItem> {
        if self.order.is_empty() {
            return None;
        }
        if self.position > 0 {
            self.position -= 1;
        } else if self.repeat == RepeatMode::All {
            self.position = self.order.len() - 1;
        } else {
            return None;
        }
        self.current()
    }

    /// Doğal bitişte hangi parçanın geleceğini **imleci oynatmadan** söyler.
    ///
    /// Gapless önden okuması için (D-024): sıradaki parça, bugünkü parça hâlâ
    /// çalarken çözülmeye başlanmalı. İmleç ancak geçiş **duyulduğunda**
    /// ilerler, yoksa arayüz olmayan bir parçayı çalıyor gösterirdi.
    ///
    /// Kuyruğun sonunda `RepeatMode::All` ile başa sarma durumunda **`None`**
    /// döner: sarma karıştırmayı yeniden üretiyor ([`Queue::reshuffle`]) ve
    /// hangi parçanın geleceği imleç oynamadan bilinemez. Bedeli, tur başına
    /// bir kez boşluk; uydurulmuş bir parçayı önden çözmekten iyidir.
    #[must_use]
    pub fn peek_after_finish(&self) -> Option<&QueueItem> {
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        let next = self.position.checked_add(1)?;
        if next >= self.order.len() {
            return None;
        }
        self.order
            .get(next)
            .and_then(|index| self.items.get(*index))
    }

    /// Parça **doğal olarak bittiğinde** sıradakini seçer.
    ///
    /// [`Queue::next`]'ten farkı: `RepeatMode::One` burada aynı parçayı
    /// yeniden verir. Kullanıcı isteğiyle otomatik geçişin ayrılması gereken
    /// tek yer burası.
    pub fn advance_after_finish(&mut self) -> Option<&QueueItem> {
        if self.repeat == RepeatMode::One {
            return self.current();
        }
        self.next()
    }

    /// Kalanları karıştırır; çalan parça (varsa) başta kalır.
    fn reshuffle(&mut self) {
        let current = self.order.get(self.position).copied();
        let mut rest: Vec<usize> = (0..self.items.len())
            .filter(|index| Some(*index) != current)
            .collect();

        // Fisher-Yates, deterministik üreteçle (xorshift64*).
        for i in (1..rest.len()).rev() {
            let j = (self.next_random() % (i as u64 + 1)) as usize;
            rest.swap(i, j);
        }

        self.order = match current {
            Some(index) => std::iter::once(index).chain(rest).collect(),
            None => rest,
        };
        self.position = 0;
    }

    /// xorshift64* — kriptografik değil, yalnızca tekrarlanabilir karıştırma.
    fn next_random(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng_state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Karıştırma üretecini sabitler. Yalnızca testler için.
    #[doc(hidden)]
    pub fn seed_rng(&mut self, seed: u64) {
        self.rng_state = seed | 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProviderId;

    fn item(title: &str) -> QueueItem {
        QueueItem {
            id: ProviderTrackId::new(ProviderId::new("local"), title),
            track: TrackRef::new("Artist", title),
        }
    }

    fn queue_of(titles: &[&str]) -> Queue {
        let mut queue = Queue::new();
        queue.replace(titles.iter().map(|t| item(t)).collect());
        queue
    }

    fn current_title(queue: &Queue) -> Option<String> {
        queue.current().map(|item| item.track.title.clone())
    }

    #[test]
    fn walks_forward_and_stops_at_the_end() {
        let mut queue = queue_of(&["a", "b"]);
        assert_eq!(current_title(&queue).as_deref(), Some("a"));
        assert_eq!(
            queue.next().map(|i| i.track.title.clone()).as_deref(),
            Some("b")
        );
        assert!(queue.next().is_none(), "repeat kapalıyken kuyruk bitmeli");
    }

    #[test]
    fn repeat_all_wraps_around() {
        let mut queue = queue_of(&["a", "b"]);
        queue.set_repeat(RepeatMode::All);
        queue.next();
        assert_eq!(
            queue.next().map(|i| i.track.title.clone()).as_deref(),
            Some("a")
        );
        assert_eq!(queue.position(), 0);
    }

    #[test]
    fn repeat_one_only_applies_to_natural_finish() {
        let mut queue = queue_of(&["a", "b"]);
        queue.set_repeat(RepeatMode::One);

        // Parça kendiliğinden bitti: aynı parça tekrar.
        assert_eq!(
            queue
                .advance_after_finish()
                .map(|i| i.track.title.clone())
                .as_deref(),
            Some("a")
        );
        // Kullanıcı "sonraki" dedi: tekrar kipinde bile ilerlemeli, yoksa
        // tuş bozukmuş gibi görünür.
        assert_eq!(
            queue.next().map(|i| i.track.title.clone()).as_deref(),
            Some("b")
        );
    }

    #[test]
    fn previous_stops_at_the_start_unless_repeating() {
        let mut queue = queue_of(&["a", "b"]);
        assert!(queue.previous().is_none());
        queue.set_repeat(RepeatMode::All);
        assert_eq!(
            queue.previous().map(|i| i.track.title.clone()).as_deref(),
            Some("b"),
            "repeat all başta geriye giderken sona sarmalı"
        );
    }

    #[test]
    fn shuffle_keeps_the_current_track_playing() {
        let mut queue = queue_of(&["a", "b", "c", "d", "e"]);
        queue.seed_rng(42);
        queue.next(); // "b" çalıyor
        assert_eq!(current_title(&queue).as_deref(), Some("b"));

        queue.set_shuffle(true);
        assert_eq!(
            current_title(&queue).as_deref(),
            Some("b"),
            "karıştırma çalan parçayı altından çekmemeli"
        );
        assert_eq!(queue.len(), 5, "hiçbir parça kaybolmamalı");
    }

    #[test]
    fn unshuffle_restores_the_original_order() {
        let mut queue = queue_of(&["a", "b", "c", "d"]);
        queue.seed_rng(7);
        queue.set_shuffle(true);
        queue.set_shuffle(false);

        let titles: Vec<String> = queue
            .items()
            .iter()
            .map(|i| i.track.title.clone())
            .collect();
        assert_eq!(titles, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn shuffle_actually_reorders_a_long_queue() {
        let titles: Vec<String> = (0..50).map(|i| format!("p{i}")).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        let mut queue = queue_of(&refs);
        queue.seed_rng(1234);
        queue.set_shuffle(true);

        let shuffled: Vec<String> = queue
            .items()
            .iter()
            .map(|i| i.track.title.clone())
            .collect();
        assert_ne!(shuffled, titles, "50 parça aynı sırada kalmamalı");
        // Aynı küme olmalı, yalnızca sıra değişmeli.
        let mut sorted = shuffled.clone();
        sorted.sort();
        let mut expected = titles.clone();
        expected.sort();
        assert_eq!(sorted, expected);
    }

    #[test]
    fn jump_to_rejects_out_of_range_instead_of_wrapping() {
        let mut queue = queue_of(&["a", "b"]);
        assert!(!queue.jump_to(5), "sınır dışı atlama reddedilmeli");
        assert_eq!(queue.position(), 0, "başarısız atlama konumu bozmamalı");
        assert!(queue.jump_to(1));
        assert_eq!(current_title(&queue).as_deref(), Some("b"));
    }

    #[test]
    fn an_empty_queue_is_inert() {
        let mut queue = Queue::new();
        assert!(queue.is_empty());
        assert!(queue.current().is_none());
        assert!(queue.next().is_none());
        assert!(queue.previous().is_none());
        assert!(queue.advance_after_finish().is_none());
    }
}
