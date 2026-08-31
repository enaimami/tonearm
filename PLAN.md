# PLAN.md — Faz Faz Yürütme Planı

Bu dosya projenin yol haritası ve kural setidir. `CLAUDE.md` kısa, her oturumda okunan
özet; bu dosya iş planlarken okunur. Çelişki olursa **bu dosya geçerlidir**.

> Proje adı `tune` bir yer tutucudur.

---

# 0. ÖNCE OKU: Çalışma Protokolü

## 0.1 Ne zaman durup soracaksın

Aşağıdakilerden biri olduğunda **kod yazma, dur ve sor.** Tahmin etme, varsayma, "muhtemelen
şunu istiyordur" deme.

| Durum | Ne yap |
|---|---|
| Bir **değişmez kuralı** ihlal etmen gerekiyor | DUR. Neden gerektiğini açıkla, sor. |
| Mevcut **fazın kapsamı dışına** çıkıyorsun | DUR. Hangi faza ait olduğunu söyle, sor. |
| **Yeni bağımlılık** eklemen gerekiyor | Sor. Crate adı, boyut, neden gerekli, alternatifi ne. |
| **Veritabanı şeması** değişecek | Sor. Migration gerekiyorsa özellikle sor. |
| İki makul tasarım var ve seçim **geri alınamaz** | Sor. Seçenekleri ve takasları sun. |
| Kimlik eşleştirme **doğruluk oranını düşürecek** bir değişiklik | Sor. Önce/sonra sayılarıyla. |
| Bir servisin davranışını **bilmiyorsun** (API şekli, dosya formatı) | Sor ya da doğrula. **Uydurma.** |
| Dışa açılan bir **API imzası** değişecek | Sor. Mobil/GUI hattını kırabilir. |
| Bir test **kırıldı** ve düzeltmek davranışı değiştiriyor | Sor. Testi susturma. |

## 0.2 Nasıl soracaksın

Soruyu şu biçimde sor:

```
KARAR GEREKLİ: <tek cümlelik konu>
Bağlam:    <neden bu noktaya gelindi>
Seçenek A: <ne> — artı: <...> eksi: <...>
Seçenek B: <ne> — artı: <...> eksi: <...>
Önerim:    <hangisi ve neden>
```

Sonra **bekle.** Cevap gelmeden ilerleme, geçici çözümle devam etme.

## 0.3 Karar defteri

Her cevaplanan soru `DECISIONS.md` dosyasına eklenir:

```
## D-007 — Kimlik eşleşme eşiği
Tarih: 2026-09-01
Soru: Bulanık eşleşmede minimum güven skoru kaç olmalı?
Karar: 0.85. Altındakiler "eşleşmedi" sayılır, kullanıcıya elle onay için sunulur.
Gerekçe: 0.75'te yanlış eşleşme oranı %4'e çıkıyordu.
```

Aynı soru iki kez sorulmaz. Bir şeye karar vermeden önce `DECISIONS.md`'yi oku.

## 0.4 Bir işi "bitti" saymadan önce

1. `cargo test --workspace` temiz
2. `cargo clippy --workspace --all-targets -- -D warnings` temiz
3. `cargo fmt --all` uygulanmış
4. Yeni yetenek varsa **CLI'de bir alt komutu var** ve `--json` destekliyor
5. Kimlik/import'a dokunduysan doğruluk kümesi çalıştırıldı, sayı raporlandı
6. Değişiklik `DECISIONS.md`'deki bir kararı geçersiz kılıyorsa güncellendi

---

# 1. TEMEL KARARLAR (cevaplandı — ayrıntı `DECISIONS.md`)

| Konu | Karar | Sonuç |
|---|---|---|
| **Mobil** | Evet, sonraki fazlarda — ama API **bugünden** uyumlu (D-001) | K7 bağlayıcıdır, taviz yok |
| **Dağıtım** | Yayınlanacak, topluluk hedefleniyor (D-002) | Faz 4 kapsamda; public repo hijyeni gerekli |
| **Yerel arşiv** | Yok, test fixture'ı üretilebilir (D-003) | Faz 1 dogfood edilemez, sıra buna göre |
| **Wrapped** | Açık hedef, aralık sezonu takvimi belirliyor (D-004) | **Faz 0.5 eklendi** |

**Hâlâ açık:** Proje adı (`tune` yer tutucu). Lisans kapandı — **D-005: MIT OR Apache-2.0**.

---

# 2. DEĞİŞMEZ KURALLAR

Tartışmaya kapalı. İhlal gerekiyorsa §0.1'e göre sor.

### K1 — Altın Kural: CLI ince kabuktur
Bütün mantık `tune-core` içindedir. CLI yalnızca: argüman ayrıştırma, çekirdek çağrısı,
çıktı biçimleme, çıkış kodu.

**Test:** bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.

CLI içinde asla: iş mantığı, veri dönüşümü, ağ çağrısı, SQL, eşleştirme algoritması.

### K2 — İçe aktarma export dosyalarından yapılır, API'den değil
Spotify/Apple/Google export zip'leri. GDPR taşınabilirlik hakkı bunu garanti eder;
sağlayıcı geliştirici şartları kısıtlayamaz. **Projenin kırılamayan tek parçası budur.**
Geçmiş veya kütüphane çekmek için sağlayıcı API'sine gitme.

### K3 — Ses asla röle edilmez, yalnızca pozisyon senkronlanır
Odalarda her istemci kendi kaynağından çalar; ağdan sadece zaman çapası geçer.
Sunucudan ses akıtan tasarım önerme. (Maliyet + hukuk + çoklu sağlayıcı, üçü birden.)

### K4 — Spotify çekirdeğe girmez
Ayrı, opsiyonel eklenti. `tune-core`'un bağımlılık ağacında Spotify'a ait hiçbir şey olmaz.

### K5 — Eklentiler alt süreç + JSON-RPC ile konuşur
Dinamik kütüphane değil. Eklenti çökerse çekirdek düşmez; eklentiler herhangi bir dilde yazılır.

### K6 — Kanonik kimlik zinciri sırası
`ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre) → AcoustID parmak izi`
Sıra bozulmaz. Her adım bir **güven skoru** döndürür.

### K7 — Çekirdek API'si `uniffi` ile ifade edilebilir olmalı
D-001 gereği **bağlayıcıdır**, gevşetilemez.

Dışa açılan imzalarda **yasak**: generic parametre (`<L: Trait>`), lifetime, closure parametresi.
**Serbest**: `Arc<dyn Trait>` — `uniffi` bunu callback interface olarak modeller
(`#[uniffi::export(with_foreign)]`), yani Kotlin/Swift tarafında uygulanabilir.
`async fn` de serbest, `uniffi` destekliyor.

> Bu kuralın ilk yazımı "trait object yok" diyordu; fazla katıydı ve D-006'da düzeltildi.

### K8 — `tune-core` içinde `unwrap()` / `expect()` / `panic!()` yok
Testler hariç. Sessiz `unwrap_or_default()` de yasak — veri kaybını yutar.

### K9 — Her başarısızlık hangi aşamada olduğunu söyler
Kısmi başarı üreten her işlem özet döndürür: kaç geldi, kaçı başarılı, kaçı hangi yolla,
kaçı başarısız.

### K10 — Faz sınırı aşılmaz
Sonraki fazın kodunu "hazır olsun diye" yazma. Faz 3 gelmeden sunucu kodu yok.

---

# 3. KONVANSİYONLAR

- Hata tipleri: `tune-core` → `thiserror`; `tune-cli` → `anyhow` serbest.
- Loglama: `tracing`. `println!` yalnızca CLI'nin kullanıcıya dönük çıktısında.
- Genel API `async`; çalışma zamanını çağıran seçer. Çekirdek `#[tokio::main]` kurmaz.
- Ağ ve dosya sistemine dokunan her şey trait arkasında (testler fake kullanabilsin).
- Kimlikler newtype: `CanonicalId`, `ProviderTrackId`, `ListenId`. Çıplak `String` yok.
- Bağımlılık ağacı küçük kalır (mobil binary boyutu).
- Kod ve tanımlayıcılar İngilizce; yorumlar ve dokümanlar Türkçe.

## Workspace

```
tune/
├── Cargo.toml              # workspace
├── CLAUDE.md               # kısa özet
├── PLAN.md                 # bu dosya
├── DECISIONS.md            # karar defteri
├── crates/
│   ├── tune-core/
│   │   ├── import/         # export ayrıştırıcıları
│   │   ├── identity/       # kanonik çözümleme
│   │   ├── stats/          # istatistik motoru
│   │   ├── library/        # SQLite + FTS
│   │   ├── provider/       # trait'ler + JSON-RPC istemcisi
│   │   ├── playback/       # symphonia + cpal        (Faz 1)
│   │   ├── sync/           # çapa protokolü          (Faz 4)
│   │   └── diag/           # tanılama
│   └── tune-cli/
├── spike/                  # ATILABILIR prototipler — workspace DIŞI, CI DIŞI
└── fixtures/               # kırpılmış export'lar, doğruluk kümesi
```

## Komutlar

```bash
cargo run -p tune-cli -- <alt-komut>
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

---

# FAZ 0 — Kimlik Katmanı

**Amaç:** Kullanıcı export zip'ini verir, ömür boyu istatistiklerini görür.
Oynatma yok, sunucu yok, hesap yok. Bu faz tek başına değerli bir üründür.

**Bitti sayılır:** `tune import x.zip && tune stats --year 2024` çalışıyor ve
kimlik doğruluk oranı ölçülüp raporlanıyor.

### 0.1 Workspace iskeleti
Cargo workspace, iki crate, CI (test + clippy + fmt), `DECISIONS.md` boş dosya.

### 0.2 Export ayrıştırıcı
- Spotify **extended streaming history**: `Streaming_History_Audio_*.json`
  Alanlar: `ts`, `master_metadata_track_name`, `master_metadata_album_artist_name`,
  `master_metadata_album_album_name`, `ms_played`, `spotify_track_uri`, `skipped`, `platform`.
- Spotify **account data**: `Playlist1.json`, kütüphane, takipler.
- Zip'i akış halinde oku — 8 yıllık arşiv büyük olabilir, tamamını belleğe alma.
- **Format değişebilir.** Beklenmeyen alan görürsen düşürme; sayıp raporla (K9).

> KARAR NOKTASI: Last.fm / ListenBrainz içe aktarma bu fazda mı, Faz 2'de mi? **Sor.**

### 0.3 Depolama
SQLite. Ham `listen` olayları **asla silinmez** — istatistikler bunlardan türetilir.
Şema versiyonlanır, migration'lar baştan planlanır.

> KARAR NOKTASI: Şema taslağını yazmadan önce sun ve onay al. Sonradan değiştirmek pahalı.

### 0.4 Kanonik kimlik çözümlemesi
**Önce `spike/` içinde Python ile prototiple.** Algoritma belli değil; eşik değerleri,
süre toleransı, remaster/live/deluxe ayrımı, "feat." temizliği on kez değişecek.

`fixtures/identity/cases.json` içinde **etiketli doğruluk kümesi** tut.
Doğruluk oranı bu projenin en önemli metriğidir; her değişiklikte ölç.

Tatmin edici orana ulaşınca `identity/`'ye porta. Zincir K6'daki sırayı izler.

> KARAR NOKTASI: Kabul edilebilir minimum doğruluk oranı ve güven eşiği. **Sor.**

### 0.5 İstatistik motoru
Toplam süre, en çok dinlenenler, dönemsel kırılım, atlanma oranı, ilk/son dinleme,
keşif zaman çizelgesi. Hepsi ham olaylardan hesaplanır, önceden hesaplanmış tablo tutma
(şimdilik — performans sorun olursa sor).

### 0.6 CLI komutları
```
tune import <zip> [--dry-run]
tune stats [--year N] [--top N] [--artist X]
tune resolve "<sanatçı> - <başlık>"
tune diag
```
Hepsi `--json` destekler.

### 0.7 Tanılama
`tune diag` son çalıştırmanın ortamını, aşamasını, hata zincirini ve sayılarını
tek blokta, kopyalanabilir biçimde basar.

---

### 0.8 Faz 0 kapanış işleri — DURUM: TAMAM

Faz 0 kodu yazıldı ve üç kapı da temiz (**69 test**, clippy, fmt).

- [x] **D-006** — `import_archive` / `resolve_track` generic'i kalktı; çekirdeğin
      dışa açık yüzeyinde generic parametre yok. `Arc<dyn MetadataLookup>`,
      trait `dyn` uyumlu olsun diye kutulanmış future döndürüyor. Yeni bağımlılık yok.
- [x] **D-007** — `diag` hata zinciri `err.source()`'tan başlıyor; `ADIM:` başlığı
      tam bir kez basılıyor, regresyon testiyle kilitli.
- [x] **D-008** — `model::PlayRule` tek tanım. `stats` ve `library search` aynı
      kuralı çağırıyor; SQL karşılığı kuraldan üretiliyor ve eşliği gerçek SQLite
      üzerinde testle doğrulanıyor. `SearchHit.plays` → `play_count`; ham sayı
      (`listen_events`) yalnızca tanı sayaçlarında.
- [x] **D-009** — Küme 15 → **69 vaka** (21 negatif), sınıf etiketli, sınıf bazında
      kırılım basılıyor. Küme küçülürse test düşüyor.

**Kimlik doğruluğu — projenin en önemli metriği:**

| Aşama | Oran |
|---|---|
| Eski küme (15 vaka) | 15/15 = %100 — *ölçüm değil* |
| Yeni küme, eski algoritma | 62/69 = **%89.9** |
| Yeni küme, düzeltilmiş algoritma | 68/69 = **%98.6** |
| D-010 karara bağlandıktan sonra | 69/69 = **%100** (22 negatif) |

Büyüyen küme iki gerçek kusur buldu: canlı kayıtlar stüdyo kaydına %100 güvenle
bağlanıyordu (`live` etiketi atılabilir sürüm eki sayılıyordu), ve yabancı bir
sanatçı yalnızca başlık benzerliğiyle eşiği geçebiliyordu. İkisi de düzeltildi;
ayrıntı ve sayılar `DECISIONS.md` D-009'da. Test eşiği %90 → %95 → **%97**.

Kalan tek vaka (radio edit) bir kusur değil cevaplanmamış bir soruydu; **D-010**
ile "radio edit ayrı bir kayıttır" diye karara bağlandı. Kod değişmedi, vaka
yeniden etiketlendi.

**Faz 0 kapandı.** Sıradaki iş Faz 0.5 (Wrapped). Rasterizasyon kararı bağlandı
(**D-011**: resvg, opsiyonel `render-png` feature'ı), kart tasarımı kararı bağlandı
(**D-012**: sabit tasarım) ve **D-005 (lisans)** kapandı: MIT OR Apache-2.0.

---

# FAZ 0.5 — Paylaşılabilir Wrapped

**Amaç:** Faz 0'ın çıktısı terminal metni; terminal metni yayılmaz. Topluluk hedefi (D-002, D-004)
paylaşılabilir bir görsel artefakt gerektiriyor. Aralık Wrapped sezonu yılda bir açılıyor
ve bedava dikkat penceresi.

**Bitti sayılır:** `tune wrapped --year 2026 --out kart.png` çalışıyor ve çıkan görsel
sosyal medyada açıklamasız paylaşılabilir kalitede.

### 0.5.1 Kart üreticisi çekirdekte yaşar
SVG üret, PNG'ye rasterize et. **`stats/` üstünde, `wrapped/` modülü olarak çekirdekte.**
CLI yalnızca sürer. GUI ve mobil aynı üreticiyi çağıracak — burada yazılan kod üç kez yazılmaz.

### 0.5.2 Biçimler
Kare (feed) ve dikey 9:16 (story) en az. Ölçüler parametre, gömülü sabit değil.

### 0.5.3 İçerik
Toplam süre, en çok dinlenen sanatçı/parça/albüm, keşif zaman çizelgesi, ilk dinleme tarihi.
**Ayırt edici nokta:** "8 yıllık geçmişin" — sağlayıcının 12 ayla sınırlı Wrapped'ının
yapamadığı şey. Kart bunu görünür kılmalı.

> KARAR NOKTASI: Rasterizasyon nasıl yapılacak? **KAPANDI — D-011:** resvg opsiyonel
> `render-png` feature'ı arkasında. Çekirdek her zaman SVG üretir; PNG yalnızca
> feature açıkken derlenir. CLI açar, mobil açmaz.

> KARAR NOKTASI: Kart tasarımı tema sisteminin (Faz 3) önizlemesi mi olsun, yoksa
> sabit mi? **KAPANDI — D-012:** sabit tasarım, isimli iç sabitlerle. Ölçüler
> parametre (`CardSize`), renkler modül içi sabitler; token sözleşmesi Faz 3'te
> tasarlanır.

### 0.5.4 Yayın hijyeni (D-002)
README, LİSANS (**D-005 kapandı: MIT OR Apache-2.0**), CONTRIBUTING, sürüm etiketleme.
Faz 0.5 muhtemelen ilk public sürüm olacak — repo o gün hazır olmalı.

---

### 0.5.5 Faz 0.5 durum — TAMAM (`v0.0.1-beta`)

Kart üreticisi çekirdekte (`wrapped/`), CLI yalnızca sürüyor. Üç kapı temiz
(**87 test**, clippy, fmt).

- [x] **0.5.1** — `wrapped/` modülü üç katman: `data` (veri türetme),
      `svg` (çizim), `png` (rasterizasyon, feature'lı). CLI'de yalnızca
      argüman ayrıştırma + `Session::wrapped` çağrısı + çıktı biçimleme var.
      `CardFormat` (clap `ValueEnum`) CLI'de duruyor ki `clap` çekirdeğe sızmasın.
- [x] **0.5.2** — `CardSize` parametre; `square()` 1080×1080, `story()` 1080×1920.
      Ölçüler gömülü sabit değil, `CardSize::new` ile herhangi bir ölçü verilebilir.
- [x] **0.5.3** — Toplam süre, en çok dinlenen sanatçı/parça/albüm, keşif
      çizelgesi, ilk dinleme tarihi, yıllara göre bar grafiği. Alt bilgi
      arşivin yaşını basıyor ("1 Ocak 2023'dan beri (2 yıl)") — sağlayıcının
      12 aylık Wrapped'ının yapamadığı şey burada görünür.
- [x] **0.5.4** — Yayın hijyeni tamam. README (ne/neden/nasıl + örnek kart),
      `LICENSE-MIT` + `LICENSE-APACHE` (D-005), CONTRIBUTING (üç kapı,
      Altın Kural, değişmez kurallar, doğruluk kümesi yordamı).
      Repo `git init` edildi ve ilk commit atıldı; sürüm `0.0.0` → **`0.0.1-beta`**,
      `v0.0.1-beta` etiketi Faz 0.5 kapanışına konuldu.
      `target/`, `spike/`, `tmp/` commit dışında; fixture'lar sentetik.

**D-011 ölçüldü:** `render-png` kapalıyken bağımlılık ağacı **56 crate**,
açıkken **114**. Yani kararın gerekçesi (mobil binary boyutu, K7) gerçek:
mobil bağlamalar 58 crate'lik rasterizasyon ağacını taşımıyor, CLI taşıyor.

**Keşif mantığının iki kipi var** ve bu kasıtlı: yıl kartında "bu yıl ilk kez
dinlediklerin" (ilk dinleme yılı = sorgu yılı), tüm zamanlar kartında ise en
çok dinlediklerinin ilk dinleme yılları, kronolojik. İkisi de sanatçının
**tüm arşivdeki** ilk yılına bakar — yıl filtresi keşfi tanımlamaz, yalnızca
kapsamı daraltır. Aksi halde her yıl her sanatçı "yeni keşif" görünürdü.

**Düzen sığdırması testle kilitli.** İlk çalışan sürümde bar bölümü kartın
alt kenarından taşıp alt bilgiyle çakışıyordu; sebep `tall = h >= w`
karşılaştırmasının kare kartı da "uzun" sayıp ona 5 keşif satırı + 260px bar
alanı vermesiydi. Artık `h > w` ve içerik sığmıyorsa keşif satırı sayısı,
sonra bar yüksekliği kısılıyor. `content_never_spills_past_the_card` ve
`a_long_archive_still_fits_the_square_card` testleri çizilen en alt `y`
koordinatını ölçüp kart yüksekliğiyle karşılaştırıyor — taşma sessizce
geri gelemez.

**Faz 0.5 kapandı.** Sıradaki iş bir karar: Faz 1 (oynatma) mı önce gelecek,
Faz 3 (GUI + tema) mi — aşağıdaki Faz 1 karar noktası. Bu, aralık Wrapped
penceresine ne yetişeceğini belirliyor ve **cevaplanmadan kod yazılmaz.**

---

# FAZ 1 — Oynatma

**Amaç:** Yerel dosyalar ve Subsonic/Jellyfin'den çalabilmek. **Bu andan sonra
scrobble'ı sen üretirsin** — geçmiş artık hiçbir sağlayıcıda oluşmaz.

**Bitti sayılır:** `tune play` ile yerel ve uzak kaynaktan kesintisiz çalıyor,
her çalma bir `listen` kaydı üretiyor.

**Dikkat (D-003):** Geliştiricinin yerel arşivi yok. Bu fazı **dogfood edemezsin** —
kendi kullanımınla test edemediğin kod, sezgiyle değil yalnızca testle doğrulanır.

Sonuçları:
1. Faz 1 başlamadan önce telifsiz test fixture'ları üret (Creative Commons / public domain,
   kısa kayıtlar, farklı formatlar: FLAC, MP3, OGG, bozuk etiketli örnekler dahil).
2. Yerel sağlayıcı ile Subsonic sağlayıcısından hangisinin önce geleceği, hangi ortamın
   gerçekten test edilebildiğine bağlı.
3. **Faz sırası sorgulanabilir.** Wrapped + topluluk hedefi (D-004) göz önüne alındığında
   Faz 3 (GUI + tema) Faz 1'den önce gelebilir — çünkü topluluk motoru temalar, ve
   oynatma geliştirici tarafından denenemiyor. Karşı argüman: müzik çalmayan bir müzik
   uygulamasının kimliği zayıf.

> KARAR NOKTASI: Faz 0.5 sonrası sıra — önce Faz 1 (oynatma) mı, önce Faz 3 (GUI + tema) mi?
> **KAPANDI — D-014: önce Faz 1 (oynatma).** Oynatma bir taban; GUI ve tema
> onun üstüne kurulur. D-003'ün dogfood kısıtı kabul edilmiş risk olarak
> taşınıyor — telifsiz fixture'larla ve testle doğrulanacak.

### 1.1 Provider trait tasarımı — TAMAM
Yetenek bayrakları şart: `SEARCH | BROWSE | STREAM | CONTROL`.
Hepsi aynı şeyi yapamaz — Spotify ileride yalnızca `CONTROL` olacak.
Trait'i tek tip varsayarsan soyutlama ilk uzak oynatıcıda çöker.

> KARAR NOKTASI: Trait imzalarını yazmadan önce sun. **KAPANDI — D-015:**
> durum çapa ile açılır (`PlaybackAnchor`), observer/callback yok.

**Uygulandı.** `provider::Provider` trait'i `Arc<dyn>` uyumlu (K7): kutulanmış
future döndürür, generic/lifetime/closure taşımaz. `Capabilities` bit maskesi
`uniffi` için `u32`. Yeteneği olmayan bir çağrı **açık hata** döndürür
(`ErrorKind::Unsupported`), sessiz boş liste değil — "yapamıyorum" ile
"sonuç yok" farklı şeylerdir (K9). `rescan` trait'e varsayılan uygulamalı
metot olarak kondu: downcast yerine, eklentiler (Faz 2) de kendi taramasını
verebilsin diye.

### 1.2 Yerel dosya sağlayıcı — KISMEN TAMAM
Dizin tarama, etiket okuma, izleme (watch), kütüphane indeksleme, SQLite FTS ile arama.

**Yapıldı:** özyinelemeli dizin tarama, symphonia ile etiket okuma
(sanatçı/başlık/albüm/ISRC/süre), etiket yoksa dosya adından türetme.
Tarama K9'a uygun özet döndürüyor: `files_seen / audio_files / indexed /
tag_fallback / failed / unreadable_dirs / unchanged`. Bozuk dosya taramayı
düşürmüyor ama **sayılıyor**; okunamayan alt dizin de öyle.

**İndeks artık kalıcı** (şema v2, `provider_tracks` + FTS5). `tune play`
tarama yapmıyor; kullanıcı bir kez `tune provider scan` der, sonraki
çalmalar katalogdan okur. Arama da doğrusal değil FTS.

Üç tasarım kararı:
- **Katalog `tracks`/`listens`'tan ayrı tablo.** `tracks` "ne dinledin"
  (ham olaylardan türer, asla silinmez), `provider_tracks` "ne çalabilirsin"
  (kaynağın aynası, dosya silinince satır da gider). Birleştirmek, diskten
  sildiğin bir dosyanın **geçmişini** de silmek olurdu — testle kilitli
  (`dropping_a_file_from_the_catalog_never_touches_its_history`).
- **Artımlı tarama mtime damgasıyla.** Damgası değişmemiş dosyanın etiketi
  yeniden okunmuyor; taramanın pahalı kısmı buydu. Fixture dizininde ikinci
  tarama 4/4 dosyayı `unchanged` sayıyor.
- **`resolve_source` kök kontrolü `canonicalize` ile.** Eskiden bellek
  indeksine bakılıyordu; indeks katalogda olduğu için artık yol, taranan
  köklerin altında mı diye sınanıyor. `<kök>/../../etc/passwd` reddediliyor.

**Kalan:** dizin izleme (watch) yok — değişiklikler `provider scan` ile
alınıyor.

### 1.3 Subsonic / Jellyfin istemcisi — KOD TAMAM, GERÇEK SUNUCUDA DOĞRULANMADI
Subsonic API yaygın standart. Bu, ileride kendi sunucunun Subsonic uyumlu
konuşması ihtimalini de açık tutar.

> KARAR NOKTASI: 1.3'e başlamak §0.1'deki üç tetikleyiciye birden basıyor —
> **yeni bağımlılık**, **bilmediğin servis davranışı**, **geri alınamaz tasarım**.
> **KAPANDI — D-019…D-022.** Aşağıdaki dört alt soru da cevaplandı; asıl soru
> metni tarih olarak bu bölümün sonunda duruyor.

**Uygulandı.** Dört karar da koda döndü:

- **D-019 (kapsam):** Subsonic **ve** Jellyfin. `provider/remote/` altında
  ortak plumbing (sunucu kaydı, kimlik saklama, akış kaynağı) + `subsonic.rs`
  + `jellyfin.rs`. Ayrışan yalnızca uç nokta ve JSON şekli.
- **D-020 (taşıma):** `net::HttpClient` trait'i **her zaman** derleniyor;
  somut istemci (`ureq` + rustls) `http-client` feature'ı arkasında. Sağlayıcı
  mantığı feature kapalıyken de derleniyor ve test ediliyor — çağıran kendi
  `Arc<dyn HttpClient>`'ını verirse çalışıyor da. Ağaç ölçümü D-020'de.
- **D-021 (kimlik):** `servers.json`, veri dizininde, unix'te `0600`. Şema
  değişmedi. Subsonic'te parola diske düz yazılmıyor (salt + `md5(parola+salt)`),
  Jellyfin'de bir kez erişim anahtarına çevriliyor. md5 için bağımlılık
  eklenmedi, RFC 1321 çekirdekte (`provider/remote/md5.rs`).
- **D-022 (test yolu):** `tests/remote_http.rs` — `std::net` ile elle yazılmış
  sahte sunucu, **gerçek `UreqClient`** ile konuşuyor. Yeni bağımlılık yok.

`AudioSource::HttpStream` artık çalınıyor. `playback/http_source.rs` akışı
arka planda indirirken çözücü ilk baytları okumaya başlıyor; `Player`'ın
eski "Faz 1.3'te gelecek" hatası yerini **derleme kararını söyleyen** bir
hataya bıraktı (`http-client` kapalıysa: "bu derlemede yok", K9).

Üç ayrıntı kayda değer:

- **Subsonic hatayı HTTP 200 ile gönderiyor.** Zarftaki `status: "failed"`
  okunmazsa "her şey yolunda" sanılırdı. Zarf her yanıtta denetleniyor ve
  hata `ErrorKind::RemoteApi` oluyor — taşıma hatasından (`Network`,
  `HttpStatus`) ayrı, çünkü "sunucuya ulaşamadım" ile "sunucu hayır dedi"
  farklı tanılar ve farklı çözümler (K9). Yeni aşama: `NETWORK_REQUEST`.
- **Kimlik nerede taşınır sağlayıcıya göre değişiyor.** Subsonic'te sorgu
  dizesinde (protokolün dayattığı yol), Jellyfin'de `Authorization`
  başlığında. İkincisi kasıtlı: akış adresi log'a ya da ekrana düşerse
  anahtar sızmasın. Testler ikisini de kilitliyor.
- **Ulaşılamamak bir sağlık cevabıdır**, komutun hatası değil. `provider test`
  sebebi gösteriyor; `health()` `Err` döndürmüyor.

**Doğrulanan ve doğrulanmayan.** `remote_http.rs`'in 10 testi gerçek soket
üzerinden şunları kilitliyor: kaydın parolayı tele hiç çıkarmadığı, Jellyfin'in
parolayı anahtara çevirdiği, 200+`failed` tuzağı, kapalı portun sebepli bir
sağlık cevabı olduğu, akışın tam inip geriye arandığı ve **fixture FLAC'ının
HTTP üzerinden gerçekten çalındığı** (ses aygıtı yoksa test kendini atlıyor,
nedenini `stderr`'e yazarak).

Sahte sunucu protokolün *bizim anladığımız hâlini* kilitler, doğru
anladığımızı kanıtlamaz: yönlendirme, transcode, tarih biçimleri ve sürüm
farkları görünmüyor. D-022 gereği **1.3 "gerçek sunucuda doğrulanmadı" diye
işaretli kalıyor**; kapanışı kullanıcının Docker'da kuracağı Navidrome /
Jellyfin denemesine bağlı.

**CLI yüzeyi** (Altın Kural sınavı geçildi — kabukta karar yok):

```
tune provider add subsonic --url https://muzik.ev --user adin [--name ev] [--no-verify]
tune provider add jellyfin --url https://jf.ev --user adin [--api-key ANAHTAR]
tune provider servers      # kimlik bilgisi gösterilmez
tune provider remove <ad>
```

Parola **argüman değil**: `TUNE_PASSWORD`'dan ya da terminalden yankısız
okunuyor — komut satırına yazılan parola kabuk geçmişine ve `ps` çıktısına
düşerdi. Tty yoksa sessizce yankılı okumaya düşülmüyor, `TUNE_PASSWORD`
gösteriliyor. Yankısız okuma için yeni bağımlılık yok (`crossterm` zaten
TUI için vardı). Ad önerisi (`https://muzik.ev:4533` → `muzik`) çekirdekte,
CLI'de değil: GUI de aynı öneriyi gösterecek.

<details>
<summary>Karar noktasının özgün metni (tarih olarak duruyor)</summary>

```
KARAR GEREKLİ: Faz 1.3 uzak sağlayıcı — kapsam, taşıma katmanı, kimlik, test yolu

Bağlam:  tune-core'un bağımlılık ağacında bugün HTTP/TLS yok; `tokio` bile
         yalnızca dev-dependency (genel API çalışma zamanından bağımsız, K:
         "çekirdek #[tokio::main] kurmasın"). İlk ağ çağrısı bu dengeyi
         değiştirir. Ayrıca D-017 "kullanıcının çalışan bir Subsonic/Jellyfin
         sunucusu YOK" varsayımını kayda geçirmişti; D-003 ise "hangi ortam
         gerçekten test edilebiliyorsa o önce gelmeli" diyor.

S1 — Kapsam
  A: Yalnızca Subsonic (OpenSubsonic). Jellyfin de Subsonic eklentisiyle
     konuşabiliyor. — artı: tek API, tek test yüzeyi, 1.3 hızlı kapanır.
     eksi: eklentisiz Jellyfin kurulumları dışarıda kalır.
  B: Subsonic + yerel Jellyfin API'si. — artı: kapsama geniş.
     eksi: iki ayrı istemci, iki auth modeli; 1.3 iki katına çıkar.
  Önerim: A. PLAN'ın kendi gerekçesi ("Subsonic yaygın standart") bunu
  destekliyor; Jellyfin'i ayrı sağlayıcı olarak Faz 2'de eklenti sınırının
  ilk gerçek müşterisi yapmak daha temiz.

S2 — Taşıma katmanı / bağımlılık
  A: Çekirdeğe `reqwest` (rustls, default-features kapalı). — artı: async
     API'ye doğrudan oturur. eksi: en büyük ağaç büyümesi (hyper+tokio+rustls),
     tokio'yu çekirdeğe **kalıcı** bağımlılık yapar; mobil binary boyutu.
  B: Çekirdeğe `ureq` (rustls). — artı: küçük ağaç, tokio yok.
     eksi: bloklayan API; async trait metodunun içinde yürütücüyü bloklar,
     sarmalamak gerekir.
  C: Çekirdekte `HttpClient` trait'i + Subsonic mantığı; somut istemci
     `http-client` feature'ı arkasında (tıpkı `audio` gibi), CLI açar.
     — artı: K "ağa dokunan her şey trait arkasında olsun, testler sahte
     kullansın" kuralının tam karşılığı; ağ olmadan test edilebilir; mobil
     kendi taşımasını verebilir. eksi: bir dolaylılık katmanı daha.
  Önerim: C — trait çekirdekte, somut istemci feature arkasında. Feature
  içinde hangi crate (ureq mi reqwest mi) ikincil ve geri alınabilir bir
  seçime dönüşür; asıl karar olan "çekirdek ağa doğrudan bağlanmasın" korunur.

S3 — Kimlik doğrulama ve saklama
  Subsonic auth: `t=md5(parola+salt)&s=salt` (md5 için bir crate gerekir) ya
  da HTTPS üzerinde düz `p=`. Sunucu adresi + kimlik nereye yazılacak?
  A: `config.rs`'in yanında düz metin config dosyası (0600).
  B: SQLite'ta yeni tablo (şema v3 → migration).
  C: OS anahtarlığı (`keyring` crate) — yeni bağımlılık, başsız Linux'ta kırılgan.
  Önerim: A + md5 token (parolayı diskte düz tutmamak için salt/token yolu),
  şema değişmeden. Anahtarlık Faz 2'de eklenti izin modeliyle birlikte.

S4 — Test yolu (bunu cevaplamadan 1.3 "bitti" sayılamaz)
  Elinde çalışan bir Subsonic/Jellyfin sunucusu **var mı**?
  Yoksa doğrulama, testte ayağa kalkan sahte bir HTTP sunucusuna dayanır
  (std::net ile elle yazılabilir, yeni bağımlılık gerekmez) — gerçek sunucuya
  karşı hiç koşmamış bir istemci olur. D-003'ün kısıtı burada yeniden çıkıyor.
  Önerim: sunucu yoksa S2/C artı elle yazılmış sahte sunucu; ve 1.3
  "kod tamam, gerçek sunucuda doğrulanmadı" diye açıkça işaretlensin.
```

</details>

### 1.4 Oynatma hattı — TAMAM
`symphonia` (çözme) + `cpal` (çıkış). Saf Rust, harici bağımlılık yok.
Video kapsam dışı — gerekirse ayrı sağlayıcı olarak tartışılır.

**Uygulandı (D-016).** Çözme arka plan iş parçacığında, çıkış cpal geri
çağrısında; aralarında halka tamponu var ve **ses geri çağrısı hiç bloklanmıyor**
(kilit alınamazsa sessizlik yazılıyor — cızırtı yerine boşluk).

İki tasarım kararı kayda değer:
1. **Pozisyon çıkışa verilmiş kareden hesaplanıyor**, çözülmüş kareden değil.
   Aradaki fark bir tampon dolusu zamandır; çözülene bakmak ilerleme çubuğunu
   sesin önüne düşürürdü.
2. **`Buffering` ayrı bir durum.** `Paused` kullanıcının kararı, `Buffering`
   hattın beklemesi; ikisini birleştirmek kullanıcıya yanlış şey söyler.

Yeniden örnekleme en yakın komşu (mono→stereo kopyalama dahil). Kaliteli
resampling gerekirse ayrıca ölçülür — Faz 1'in hedefi doğru ses üretmekti.

### 1.5 Kuyruk ve oynatma durumu — KISMEN TAMAM
Kuyruk, tekrar, karıştırma, gapless. Durum çekirdekte tutulur, CLI yalnızca gösterir.

**Yapıldı:** kuyruk, `RepeatMode::{Off,All,One}`, karıştırma. Durum çekirdekte;
CLI yalnızca gösteriyor.

İki ince nokta testle kilitli:
- **Karıştırma sırayı bozmaz**, ayrı bir çalma sırası tutar. Kapatınca kullanıcı
  listesini kaybetmez; açarken çalan parça **altından çekilmez**, başta kalır.
- **`RepeatMode::One` yalnızca doğal bitişte tekrar eder.** Kullanıcı "sonraki"
  derse tekrar kipinde de ilerler — yoksa tuş bozukmuş gibi görünür.
  Ayrım `Queue::next` ile `advance_after_finish` arasında.

**Kalan:** gapless geçiş yok (parça bitince yeni motor kuruluyor, aralarında
kısa boşluk var).

### 1.6 Scrobbling — TAMAM
Her çalma bir `listen` kaydı. Faz 0'daki import verisiyle aynı tabloya yazılır —
geçmiş ve bugün tek bir zaman çizelgesi olur.

**Uygulandı ve uçtan uca doğrulandı.** `tune play` ile çalınan parça
`stats` ve `library search` çıktısında görünüyor; CLI testi bunu kilitliyor
(`playing_a_local_file_records_a_listen_in_the_same_table_as_imports`).

Süre olarak **çıkışa verilen ses** yazılıyor, geçen duvar saati değil —
duraklatılan süre dinlenmiş sayılmaz. Eşik D-008'deki tek `PlayRule`;
burada ikinci bir yorum yok.

### 1.7 CLI oynatıcı arayüzü — TAMAM
`ratatui` ile TUI. Bu, GUI'nin prototipi değil; çekirdeğin tam kullanılabilir olduğunun kanıtı.

**Uygulandı:** `tune play <sorgu> --tui`. Çalan parça + durum, ilerleme çubuğu,
kuyruk (çalan `▸` ile işaretli), tekrar/karıştırma göstergesi, tuş yardımı.
Tuşlar: boşluk duraklat, `n`/`b` sonraki/önceki, `↑↓`/`jk` seçim, `enter` seçileni
çal, `s` karıştır, `r` tekrar kipi, `q`/`Esc`/`Ctrl+C` çık.

**Altın Kural'ın sınavını geçti.** TUI'de karar veren tek satır yok: tuş →
eylem eşlemesi (`action_for`) ve eylem → çekirdek çağrısı (`apply`) ayrı, ikisi
de yalnızca iletiyor. "Duraklat mı sürdür mü" kararı bile çekirdekte
(`Player::toggle_pause`) — TUI ve GUI aynı kararı iki kez vermesin diye.

Pozisyon **yoklanmıyor**: TUI kendi çizim hızında `anchor.position_now()`
çağırıyor, formül çekirdekte (D-015). Bu, PLAN 3.2'nin GUI için istediği
davranışın aynısı — yani TUI o tasarımın çalıştığının kanıtı oldu.

İki ayrıntı:
- `TerminalGuard` `Drop` ile ham kipi geri veriyor: panik olsa bile
  kullanıcının terminali bozuk kalmıyor.
- Çekirdekten gelen hata TUI'yi **düşürmüyor**; alt satırda gösteriliyor ve
  döngü sürüyor. Terminal hiç açılamazsa `ADIM: PLAYBACK_OUTPUT` ile düşüyor
  (testle kilitli) — sessizce metin kipine kaçmıyor.

---

### 1.8 Faz 1 durum — yerel **ve** uzak kaynaktan çalıyor

**197 test**, clippy ve fmt temiz. `tune play` ve `tune play --tui` uçtan uca
çalışıyor.

| Bölüm | Durum |
|---|---|
| 1.1 Provider trait | TAMAM |
| 1.2 Yerel sağlayıcı | Kısmen — indeks kalıcı; watch yok |
| 1.3 Subsonic/Jellyfin | Kod tamam — gerçek sunucuda doğrulanmadı (D-022) |
| 1.4 Ses hattı | TAMAM |
| 1.5 Kuyruk | Kısmen — gapless yok |
| 1.6 Scrobbling | TAMAM |
| 1.7 TUI | TAMAM |

**Test fixture'ları üretildi (PLAN'ın Faz 1 ön koşulu):** `fixtures/audio/`
altında `ffmpeg` ile üretilmiş sinüs tonları — telifsiz, toplam 84 KB.
FLAC (etiketli), MP3 (etiketli), OGG (etiketsiz), alt dizinde etiketsiz FLAC
ve kasten bozuk bir dosya. D-003'ün "dogfood edilemez" kısıtı böylece
kısmen aşıldı: ses hattı gerçek dosyalarla, gerçek aygıtta sınanıyor.

**Ses aygıtı olmayan ortamda testler kendini atlıyor** (CI için). Atlama
sessiz değil: nedenini `stderr`'e yazıyor.

**Faz 1'in çekirdeği tamam.** Kalan iki iş, ikisi de bloklayıcı değil:
gapless geçiş ve dizin izleme (watch).

Faz 1'in bitti ölçütü ("yerel ve uzak kaynaktan çalıyor") **koda göre
karşılandı**: uzak akış testte gerçek soketten inip gerçek aygıtta çalıyor.
Ama D-022 gereği kalan tek doğrulama duruyor — **gerçek bir Navidrome /
Jellyfin kurulumuna karşı bir kez koşturmak.** Faz 2'ye geçmeden önce
yapılması gereken tek iş bu; yeni kod değil, bir deneme.

**Faz 2'ye devredilen borç:** `keyring` kararı (D-021, kimlik bilgisi
`servers.json`'da düz duruyor — token/anahtar, parola değil) eklenti izin
modeliyle birlikte yeniden bakılacak.

---

# FAZ 2 — Eklenti Sınırı ve Sağlayıcı Genişlemesi

**Amaç:** Sağlayıcılar çekirdeğin dışına çıkar. Üçüncü taraf eklenti yazabilir hale gelir.

**Bitti sayılır:** Rust olmayan bir referans eklenti çalışıyor ve çekirdek onu
sürüm uyumsuzluğunda çökmeden reddedebiliyor.

### 2.1 JSON-RPC eklenti protokolü
Yaşam döngüsü, el sıkışma, **sürümleme**, zaman aşımı, çökme izolasyonu.
Protokol sürümlenir; uyumsuz eklenti yüklenmez, hata mesajı verir.

> KARAR NOKTASI: Eklenti izin modeli (ağ/dosya erişimi kısıtlanacak mı?). **Sor.**

### 2.2 Referans eklenti
Rust olmayan bir dilde (Python) yazılmış bir sağlayıcı — protokolün gerçekten
dil bağımsız olduğunun kanıtı.

### 2.3 Kimlik çözümlemesi olgunlaşır
AcoustID / Chromaprint parmak izi. Etiketleri bozuk yerel dosyalar için.

### 2.4 Torrent sağlayıcı
`librqbit`. `set_piece_deadline` ile sıralı akış. Bash prototipinin varisi —
oradaki dersler geçerli: her torrent kendi dizinine, hazırlık için sabit `sleep` yerine
gerçek hazır olma kontrolü, peer sayısı ve indirme hızı raporlanır.

---

# FAZ 3 — GUI ve Tema

> **Sıra açık:** Faz 1'den önce mi sonra mı geleceği karara bağlı — bkz. Faz 1 karar noktası.
> Topluluk motoru burasıdır (D-002, D-004), o yüzden geciktirilmesi pahalıdır.

**Amaç:** Tauri masaüstü arayüzü + kullanıcıların yazabildiği tema sistemi.
Tema ekosistemi bu projenin dağıtım kanalıdır, sonradan eklenecek bir süs değil.

### 3.1 GO / NO-GO ölçümü — ÖNCE BU
Tauri'de 50.000 satırlık sanallaştırılmış liste + CSS animasyon + IPC yükü prototipi.
**Linux'ta WebKitGTK ölç.** Hedef kitle Linux ağırlıklı ve WebKitGTK üç platformun
en zayıfı.

> KARAR NOKTASI: Ölçüm sonucunu sun. Kabul edilemezse Dioxus/yerel Rust GUI
> tartışılır — ama o durumda CSS tema ekosistemi kaybedilir. **Sor.**

### 3.2 IPC sözleşmesi
Webview ile çekirdek arasında saniyede yüzlerce mesaj = takılma.
Toplu gönderim; oynatma pozisyonu webview'de **çapadan tahmin edilir**, sürekli
çekirdekten sorulmaz.

### 3.3 Tema API'si — sürümlenmiş sözleşme
Spicetify'ın en büyük derdi: üst uygulama değişiyor, temalar bozuluyor.
Bunu yaşamamak için semantik token seti (CSS custom properties), taahhüt edilen
slot isimleri ve **sürümlenmiş tema formatı** baştan tasarlanır.
İçeride ne değişirse değişsin bu yüzey sabit kalır.

> KARAR NOKTASI: Token setini yazmadan önce sun. Bu bir kez yayınlandıktan sonra
> geriye dönük uyumluluk borcu doğar.

### 3.4 Referans temalar
En az iki farklı temada tema API'sinin yeterli olduğunu kanıtla.

---

# FAZ 4 — Odalar (backend burada başlar)

**Amaç:** Birlikte senkron dinleme. Discord müzik botlarının bıraktığı boşluk.

**Ön koşul:** D-002 gereği kapsamdadır. Ama sunucu işletmek gelir gerektirmeyen bir
maliyet doğurur — bu faza başlamadan önce kullanıcı tabanının var olduğu kanıtlanmış olmalı.
Faz 0.5 ve 3 kullanıcı getirmediyse Faz 4'e girme.

### 4.1 Çapa protokolü
Tek primitif:
```
{ track: CanonicalId, anchor_wall_time: T, anchor_position: P, rate: f64, state: Playing|Paused }
```
İstemci kendi pozisyonunu hesaplar: `pos = P + (now - T) * rate`.
Olay yayını değil çapa yayını — sonradan katılan tek mesajla yerine oturur,
paket kaybı kendini düzeltir.

### 4.2 Saat senkronu
NTP tarzı offset: `offset = ((t2-t1) + (t3-t4)) / 2`. Birkaç ölçümün medyanı, periyodik tekrar.

### 4.3 Zamanlanmış başlangıç
Sağlayıcılar farklı gecikmelerle açılır (yerel ~20ms, uzak ~800ms).
"Şimdi başla" değil, "T+2sn'de başla" — herkes önceden buffer'lar ve seek eder.
Parça geçişlerinde sonraki parça önceden duyurulur.

### 4.4 Sürüklenme düzeltmesi
Tolerans geniş: farklı evlerde 50-100ms fark fark edilmez.
Yumuşak düzeltme (hızda %0.1 oynama veya mikro-seek). Snapcast'in ~1ms hedefi gerekmiyor.

### 4.5 v1: tek sağlayıcı, v2: karışık
v1'de herkes aynı sağlayıcıdaysa kanonik çözümleme gerekmez — `provider_track_id` yeter.
Karışık sağlayıcı v2'ye bırakılır. Bu, odaların kimlik katmanı olgunlaşmadan çıkmasını sağlar.

### 4.6 Röle sunucusu
Yalnızca sinyal taşır (K3). Katılımcı başına bir websocket, saniyede birkaç bayt.
P2P sinyal için **gereksiz** — tek doğruluk kaynağı basitliği kazandırır.
P2P sadece ileride sesli sohbet eklenirse tartışılır.

> KARAR NOKTASI: Sesli sohbet kapsamda mı? Öneri: **hayır**, kullanıcılar zaten
> Discord'da konuşuyor. Eklenecekse WebRTC + TURN maliyeti hesaplanmalı. **Sor.**

---

# FAZ 5 — Sosyal Graf

Arkadaşlık **kurulmaz, türetilir**: "sık sık birlikte dinlediklerin" odalardan birikir.
Boş bir "arkadaş ekle" ekranıyla açılış yapma — soğuk başlangıç problemi.

---

# FAZ 6 — Mobil

**Onaylı hedef (D-001).** `uniffi` ile Kotlin/Swift bağlamaları.

Uygulama bu fazda, ama **kısıt bugünden geçerli**: çekirdeği değiştirmeden gelmeli.
Gelmiyorsa çekirdek yanlış tasarlanmış demektir (K7).

**Erken uyarı mekanizması:** Faz 1'den itibaren CI'da `uniffi` scaffolding üretimi
denensin — derlenmesi bile gerekmez, sadece "bu API ifade edilebilir mi?" sorusunu
her commit'te sorar. Aksi halde ihlaller aylarca birikir ve toplu halde ortaya çıkar.

> KARAR NOKTASI: Bu CI kontrolü ne zaman eklenecek? Öneri: Faz 0.8'deki D-006
> düzeltmesiyle birlikte, hemen. **Sor.**

---

# EK — Spotify

Ayrı depo, ayrı paket. Çekirdeğe asla girmez (K4).
Yalnızca `CONTROL` yetenekli bir uzak oynatıcı olarak modellenir — metadata çekilmez,
veritabanına yazılmaz. Kullanıcının geçmişi export yoluyla gelir (K2).

---

# ASLA YAPMA

- CLI'ye iş mantığı koyma
- Sağlayıcı API'sinden geçmiş/kütüphane çekme — export kullan
- Sunucudan ses akıtma
- Çekirdeğe Spotify bağımlılığı ekleme
- Faz sınırını aşma ("ileride lazım olur" diye kod yazma)
- Çekirdek API'sine `uniffi`'nin ifade edemeyeceği tip sızdırma
- Ham `listen` kayıtlarını silme veya üzerine yazma
- İzinsiz bağımlılık ekleme
- Kırılan testi susturma
- Bilmediğin bir format/API şeklini varsayma — sor veya doğrula

---

# SÖZLÜK

| Terim | Anlamı |
|---|---|
| **canonical id** | Sağlayıcıdan bağımsız parça kimliği (tercihen MBID) |
| **provider** | Ses kaynağı: yerel, Subsonic, SoundCloud, torrent… |
| **plugin** | Alt süreç olarak çalışan, JSON-RPC konuşan sağlayıcı |
| **listen** | Tek dinleme olayı: parça + zaman damgası + süre + kaynak |
| **anchor** | `{track, wall_time, position, rate, state}` — oda senkronunun tek primitifi |
| **resolve** | Parçayı kanonik kimliğe, oradan sağlayıcı kimliğine eşleme |
| **doğruluk kümesi** | `fixtures/identity/cases.json` — elle etiketlenmiş eşleşme testleri |
