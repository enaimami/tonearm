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
│   ├── tune-cli/           # ince CLI kabuğu
│   └── tune/               # Tauri masaüstü kabuğu (Faz 3)
│       ├── src/            # main + env + state + core_thread + commands
│       └── ui/             # düz statik webview — bundler yok, npm yok
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

### 1.2 Yerel dosya sağlayıcı — TAMAM
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

**Dizin izleme yerine bayatlık yoklaması (D-025).** `notify` bağımlılığı
eklenmedi: `tune provider scan --if-stale` sağlayıcıya ucuz bir soru soruyor
(`catalog_changed_since`) ve yalnızca gerekiyorsa tarıyor. Yerel sağlayıcı
bunu **yalnızca dizinleri** gezerek cevaplıyor — dosyalar `stat` edilmiyor.

Üç cevap üçü de farklı şey: değişmiş / değişmemiş / **bilmiyorum**.
"Bilmiyorum" bir tarama sebebi, atlama sebebi değil.

Görmediği şey açıkça yazılı: dosyanın **yerinde yeniden etiketlenmesi**
(dosya değişir, dizin damgası değişmez). O durumda düz `provider scan`
gerekiyor ve komut yardımı bunu söylüyor. Gerçek zamanlı izleme gerekirse
Faz 3'te GUI'nin olay döngüsüyle birlikte yeniden bakılır.

### 1.3 Subsonic / Jellyfin istemcisi — TAMAM (gerçek sunucuda doğrulandı)
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

**Otomatik doğrulama.** `remote_http.rs`'in 10 testi gerçek soket üzerinden
şunları kilitliyor: kaydın parolayı tele hiç çıkarmadığı, Jellyfin'in parolayı
anahtara çevirdiği, 200+`failed` tuzağı, kapalı portun sebepli bir sağlık
cevabı olduğu, akışın tam inip geriye arandığı ve **fixture FLAC'ının HTTP
üzerinden gerçekten çalındığı** (ses aygıtı yoksa test kendini atlıyor,
nedenini `stderr`'e yazarak).

**Gerçek sunucu doğrulaması — 2026-08-31, YAPILDI.** Docker'da Navidrome
0.63.2 ve Jellyfin 10.11.11 kuruldu; ikisinde de kayıt → doğrulama → arama →
**akış** → scrobble zinciri uçtan uca çalıştı. Ayrıntı, yordam ve hâlâ
sınanmayanlar (TLS, ters vekil, transcode, büyük kütüphane, Navidrome dışı
Subsonic uygulamaları) **D-022'nin doğrulama bölümünde**.

Gerçeğin gösterdiği tek kusur: doğrulama başarısız olduğunda dıştaki cümle
"erişilemedi" diyordu, oysa sunucu erişilebilirdi ve parolayı reddetmişti.
Artık "doğrulanamadı" diyor; sebebi `detail` taşıyor (K9). Sahte sunucu bunu
gösteremezdi çünkü kusur protokolde değil, **iki farklı başarısızlığı tek
cümlede birleştirmekteydi**.

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

### 1.5 Kuyruk ve oynatma durumu — TAMAM
Kuyruk, tekrar, karıştırma, gapless. Durum çekirdekte tutulur, CLI yalnızca gösterir.

**Yapıldı:** kuyruk, `RepeatMode::{Off,All,One}`, karıştırma. Durum çekirdekte;
CLI yalnızca gösteriyor.

İki ince nokta testle kilitli:
- **Karıştırma sırayı bozmaz**, ayrı bir çalma sırası tutar. Kapatınca kullanıcı
  listesini kaybetmez; açarken çalan parça **altından çekilmez**, başta kalır.
- **`RepeatMode::One` yalnızca doğal bitişte tekrar eder.** Kullanıcı "sonraki"
  derse tekrar kipinde de ilerler — yoksa tuş bozukmuş gibi görünür.
  Ayrım `Queue::next` ile `advance_after_finish` arasında.

**Gapless geldi (D-024).** cpal akışı ve halka tamponu parçalar arasında
**açık kalıyor**; çözme iş parçacığı bir parça bitince sıradakini alıp aynı
tampona yazmayı sürdürüyor. Eskiden her parça için yeni bir aygıt + yeni bir
çözücü kuruluyordu, boşluk buydu.

Pozisyon artık **dilimlerden** okunuyor: tampon birden çok parçanın örneklerini
yan yana taşıdığı için tek bir sayaç yetmiyor. Her parça çıkış karesi cinsinden
nerede başladığını biliyor; çalan parça, `frames_played`'in düştüğü dilim.
Geçiş **duyulduğunda** ilerliyoruz — sıraya girmiş ama henüz çalınmamış parça
"çalıyor" sayılmıyor, yoksa arayüz duyulmayan parçayı gösterirdi.

Kullanıcı isteğiyle geçiş (`next`, `enter`) kasten gapless **değil**: motor
yeniden kuruluyor. Önden okunmuş sesi çalmak, kullanıcının seçmediği parçayı
duyurmak olurdu.

Ölçüm: 4 fixture parçası (5 sn ses) uçtan uca 5.47 sn'de çalındı.

**Bir tutarsızlık da bu sırada kapandı.** Etiketsiz dosyaların süresi katalogda
bilinmediği için `PlayRule`'un "parçanın yarısı" kolu çalışmıyor, kural 30 sn
eşiğine düşüyordu: baştan sona dinlenen 1 sn'lik parça scrobble üretmiyordu.
Süre artık kaptan okunuyor ve hem kurala hem kayda giriyor — yoksa CLI "4
dinleme kaydedildi" derken `stats` 2 gösteriyordu. Testle kilitli.

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

### 1.8 Faz 1 durum — KAPANDI

**203 test**, clippy ve fmt temiz. `tune play` ve `tune play --tui` uçtan uca
çalışıyor; yerel dosyadan da, Navidrome/Jellyfin'den de.

| Bölüm | Durum |
|---|---|
| 1.1 Provider trait | TAMAM |
| 1.2 Yerel sağlayıcı | TAMAM — indeks kalıcı; `--if-stale` (D-025) |
| 1.3 Subsonic/Jellyfin | TAMAM — Navidrome + Jellyfin'de doğrulandı (D-022) |
| 1.4 Ses hattı | TAMAM |
| 1.5 Kuyruk | TAMAM — gapless dahil (D-024) |
| 1.6 Scrobbling | TAMAM |
| 1.7 TUI | TAMAM |

**Test fixture'ları üretildi (PLAN'ın Faz 1 ön koşulu):** `fixtures/audio/`
altında `ffmpeg` ile üretilmiş sinüs tonları — telifsiz, toplam 84 KB.
FLAC (etiketli), MP3 (etiketli), OGG (etiketsiz), alt dizinde etiketsiz FLAC
ve kasten bozuk bir dosya. D-003'ün "dogfood edilemez" kısıtı böylece
kısmen aşıldı: ses hattı gerçek dosyalarla, gerçek aygıtta sınanıyor.

**Ses aygıtı olmayan ortamda testler kendini atlıyor** (CI için). Atlama
sessiz değil: nedenini `stderr`'e yazıyor.

**Faz 1'in bitti ölçütü karşılandı:** "yerel **ve uzak** kaynaktan çalıyor,
her çalma bir `listen` kaydı üretiyor" — uzak yarısı 2026-08-31'de gerçek
Navidrome ve gerçek Jellyfin üzerinde doğrulandı (D-022). Yedi bölümün
yedisi de TAMAM; Faz 2'ye geçişin önünde bloklayıcı bir iş kalmadı.

**Faz 2'ye devredilenler** (hiçbiri Faz 1'i eksik bırakmıyor):
- `keyring` kararı (D-021) — kimlik bilgisi bugün `servers.json`'da `0600`
  ile duruyor; eklenti izin modeliyle birlikte yeniden bakılacak.
- Gerçek zamanlı dizin izleme (D-025) — bugün tetiklenince bakan bir yoklama
  var; Faz 3'te GUI'nin olay döngüsü geldiğinde yeniden değerlendirilir.
- TLS/ters vekil/transcode altında uzak sağlayıcı doğrulaması (D-022) —
  kod bunları desteklemek üzere yazıldı ama denenmedi.

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

### 2.2 Referans eklenti — SoundCloud (D-027)
Rust olmayan bir dilde (Python) yazılmış bir sağlayıcı — protokolün gerçekten
dil bağımsız olduğunun kanıtı.

Platform seçimi katalog kalitesine göre değil **sınanabilirliğe** göre yapıldı:
SoundCloud abonelik gerektirmeyen tek aday, yani CI'da ve başkasının makinesinde
çalışabilen tek aday. Referans eklentinin işi protokolü kanıtlamak, katalog sunmak
değil.

### 2.3 Kimlik çözümlemesi olgunlaşır
AcoustID / Chromaprint parmak izi. Etiketleri bozuk yerel dosyalar için.

### 2.4 Torrent sağlayıcı
`librqbit`. `set_piece_deadline` ile sıralı akış. Bash prototipinin varisi —
oradaki dersler geçerli: her torrent kendi dizinine, hazırlık için sabit `sleep` yerine
gerçek hazır olma kontrolü, peer sayısı ve indirme hızı raporlanır.

### 2.5 Yayın platformu eklentileri
Hangi platformdan ses çalınabileceği API'nin varlığına değil **DRM'e** bağlı.
Tam liste, gerekçeler ve hukuki çizgi: **EK — Yayın platformları**.

Özet: SoundCloud / Qobuz / YouTube Music akıtılabilir; Tidal / Apple Music /
Deezer yalnızca metadata verir; Spotify yalnızca `CONTROL` (K4);
Amazon Music / Pandora / Idagio / Tencent kapalı.

Hepsi eklenti — hiçbiri çekirdeğe girmez. Sebep yalnızca K5 değil: bu API'ler
haber vermeden bozulur, ve bozulduğunda **çalan müzik durmamalı**, yalnızca o
eklenti düşmeli.

> KARAR NOKTASI: §2.2'nin referans eklentisi hangi platform olacak ve bu fazda
> kaç tane yazılacak? **Sor.**

---

# FAZ 3 — GUI ve Tema

> **SIRADAKİ FAZ — D-027.** Faz 1 kapandıktan sonra Faz 2 yerine bu seçildi:
> topluluk motoru burasıdır (D-002, D-004), geciktirilmesi pahalıdır.
> Faz 2 ertelendi, iptal edilmedi.

**Amaç:** Tauri masaüstü arayüzü + kullanıcıların yazabildiği tema sistemi.
Tema ekosistemi bu projenin dağıtım kanalıdır, sonradan eklenecek bir süs değil.

### 3.1 GO / NO-GO ölçümü — TAMAM (koşullu GO)
Tauri'de 50.000 satırlık sanallaştırılmış liste + CSS animasyon + IPC yükü prototipi.
**Linux'ta WebKitGTK ölç.** Hedef kitle Linux ağırlıklı ve WebKitGTK üç platformun
en zayıfı.

> KARAR NOKTASI: Ölçüm sonucunu sun. Kabul edilemezse Dioxus/yerel Rust GUI
> tartışılır — ama o durumda CSS tema ekosistemi kaybedilir.
> **KAPANDI — D-028: GO, ortam şartıyla.**

**Ölçüldü** (Intel HD 6000 / 2015, WebKitGTK 2.52.6, Tauri 2). Eşikler ölçümden
önce yazıldı (`spike/tauri-gonogo/ESIKLER.md`). Sekiz ölçünün sekizi de GO;
yalnızca "saf CSS" fazı sınırda.

Üç şey buradan çıktı ve sonraki bölümleri bağlıyor:

1. **Linux'ta `GDK_BACKEND=x11` + `WEBKIT_DISABLE_DMABUF_RENDERER=1` şart.**
   Varsayılan ortamda kare hızı 2.4× düşüyor (58.8 → 23.8 fps). İkisi **birlikte**
   gerekiyor; tek başına her biri işe yaramıyor, biri kaydırmayı kötüleştiriyor.
   **D-029: uygulamanın kendisi kuruyor** — `main()`'in ilk işi, yalnızca Linux'ta,
   yalnızca değişken tanımlı değilse. Doğrulandı: dış değişken olmadan 23.8 → 55.6 fps.
   Açık pürüz: `set_var` Rust 2024'te `unsafe` ve workspace `unsafe_code = "forbid"`
   diyor — GUI paketi açılırken çözülecek (D-029).
2. **§3.2'nin gerekçesi değişti** — aşağıya bak.
3. **§3.3 bir kısıt kazandı** — aşağıya bak.

Suçlunun donanım değil motor olduğu **kontrol deneyiyle** ayrıldı: aynı makinede
aynı sayfayı Firefox dört fazın dördünde de 58.8 fps çiziyor. Donanım tavanı
olsaydı yerel Rust GUI'ye kaçmak da kurtarmazdı.

### 3.2 IPC sözleşmesi — TAMAM
~~Webview ile çekirdek arasında saniyede yüzlerce mesaj = takılma.~~
**D-028 bunu ölçtü ve doğrulamadı:** IPC gidiş-dönüş p95 **1 ms**, 30 Hz yoklama
kare süresine **1 ms** ekliyor, köprü **~10.000 olay/sn** taşıyor. Toplu gönderim
performans için gerekli değil.

Oynatma pozisyonu webview'de yine **çapadan tahmin edilir** — ama gerekçesi
performans değil:
- IPC duraksarsa arayüz donmaz, tahmin yürümeye devam eder.
- Faz 4'ün oda primitifi zaten aynı tip (D-015); iki ayrı pozisyon kavramı
  tutulmaz.

Yanlış gerekçeyle savunulan doğru tasarım ilk itirazda düşer; sözleşme bu
düzeltilmiş gerekçeyle yazılacak.

**Çekirdek tarafı hazır (D-032).** `playback::LiveSession` — `Player` ile
`Session`'ı bağlar, tek `tick()` ilerletir + biriken dinlemeleri yazar +
`TickReport` döndürür. Kabuk yalnızca döngüyü sürer. TUI buna geçirildi;
GUI aynı tipi kullanacak, dans ikinci kez yazılmayacak.

Yazma artık **her turda**, çıkışta değil: saatlerce açık kalan bir arayüzde
çökme bütün oturumun geçmişini götürürdü. Yazılamayan kayıt atılmıyor, elde
tutulup yeniden deneniyor; `store_error` ve `listens_pending` bunu görünür
kılıyor (K9).

**Paket düzeni (D-030):** üç paket — `tune-core`, `tune-cli`, `tune` (GUI).
Bağımlılık tek yönlü: `tune-cli` ile `tune` birbirini hiç görmez. Biri
diğerinden bir şey isterse o şey çekirdeğe aittir.

#### Sözleşmenin şekli

**Sözleşme = çekirdeğin yüzeyi + serde.** Ayrı bir "IPC tipi" katmanı
yazılmıyor: her komut zaten var olan bir çekirdek tipini döndürüyor. Çevirmen
katmanı iki tipi zamanla kaydırırdı ve kaymayı hiçbir şey yakalamazdı.
`--json` çıktısı bunu zaten kanıtlıyor — CLI ile GUI **aynı** veriyi alıyor.

**Komutlar** CLI'nin alt komutlarıyla birebir örtüşüyor, çünkü ikisi de aynı
çekirdeğin kabuğu:

| Alan | Komutlar |
|---|---|
| Kütüphane | `search`, `stats`, `wrapped` |
| İçe aktarma | `import` |
| Kimlik | `resolve` |
| Sağlayıcı | `providers`, `provider_test`, `provider_scan`, `servers_list`, `server_add`, `server_remove` |
| Oynatma | `play`, `toggle_pause`, `stop`, `next`, `previous`, `jump_to`, `set_shuffle`, `set_repeat` |
| Durum | `anchor`, `queue` |
| Tanılama | `diag` |

Ayrıca `environment` var: veri dizini, veritabanı yolu, müzik dizinleri. Yeni
bir "durum" tipi değil — hata mesajının yanında "hangi kütüphaneye baktım"
sorusu cevapsız kalmasın diye (K9).

**Olaylar** yalnızca **bir şey değiştiğinde** gönderilir, zamanlayıcıyla değil.
GUI'nin Rust tarafı `LiveSession::tick()` döngüsünü sürer; `TickReport`
webview'e ancak şunlardan biri varsa geçer:

- `track_changed`, `listens_recorded`, `store_error`, `finished`, **ya da**
- **çapanın tahmine girdi olan kısmı değişmişse** — durum, hız, süre, parça
  kimliği. `position_ms` bilerek listede yok: tahminin işi zaten o.

Son madde **D-035**. İlk liste onsuz yazıldı ve arayüzü sessizce dondurdu:
ses `Buffering` başlayıp `Playing`'e geçiyor, geçiş gönderilmediği için
webview elindeki `Buffering` çapasıyla kalıyor ve `Buffering` ilerlemediği
için ilerleme çubuğu 0:00'da donuyordu. Hiçbir hata görünmüyordu. Bu kuralda
asıl risk fazla mesaj değil **eksik** mesajdır: fazlası ölçülür, eksiği iz
bırakmaz.

Aradaki sessizlikte pozisyon çapadan tahmin edilir.

Uzun komutlar (`import`, `resolve`, `provider_scan`, `provider_test`,
`server_add`, `play`) ayrıca `tune://busy` gönderir — bu da bir durum
değişimi, zamanlayıcı değil.

#### Sürümleme gerekmiyor — ve bunun bir sınırı var

§3.3'ün aksine burada **iki taraf da aynı ikilinin içinde**: webview varlıkları
uygulamayla birlikte paketleniyor, bağımsız güncellenemiyor. Çalışma zamanında
sürüm anlaşması yapmak, yalnızca kendisiyle konuşabilen bir sürece el sıkışma
öğretmek olurdu.

**Sınır şurada:** temalara (§3.3) IPC yüzeyi açılırsa sözleşme *dış* bir
sözleşmeye dönüşür ve o gün sürümleme borcu doğar. §3.3 token setini
tasarlarken bu soru cevaplanmalı — tema IPC çağırabilecek mi?

#### Kayma koruması: paylaşılan doğruluk kümesi

Pozisyon webview'de çekirdeğe sorulmadan tahmin ediliyor, yani formülün ikinci
bir kopyası JS'te yaşıyor. İki kopya zamanla kayar ve kayma kimsenin fark
etmediği yerde başlar — ilerleme çubuğu birkaç yüz milisaniye yalan söyler,
kimse şikâyet etmez, sonra **Faz 4'te aynı formül oda senkronunu sürer.**

`fixtures/anchor/position_cases.json` iki tarafın da okuduğu tek doğruluk
kaynağı (12 vaka). Rust tarafını `tune-core/tests/anchor_parity.rs`, JS
tarafını `tune/ui/anchor.js` + `tune/tests/anchor_parity.mjs` bağlıyor.

Kümenin en değerli vakası yazılırken bulundu: `rate = 1.001` ile 100 sn'de
çekirdek **100099 ms** diyor, 100100 değil — `100000 × 1.001` ikilik tabanda
tam değil ve `as u64` kırpıyor. JS `Math.round` kullanırsa iki kopya tam
buradan ayrılır. Doğru karşılık `Math.floor(gecen_ms * rate)`.

JS koşumu `node` istiyor ve `node` yoksa test **atlanmıyor, düşüyor**.
Atlanabilir olsaydı kurulu olmayan bir makinede yeşil yanıp hiçbir şey
kanıtlamazdı — D-032'de tam bu yüzden bir test silinmişti.

#### Paket — TAMAM

`crates/tune/`: Tauri kabuğu, ikili adı `tune-desktop` (D-034). Rust tarafı
`main.rs` + `env.rs` (D-031 ortam düzeltmesi) + `state.rs` + `core_thread.rs`
+ `commands.rs`; webview `ui/` altında düz statik dosya — bundler yok, npm
yok, `frontendDist: "ui"`.

**Çekirdek kendi iş parçacığında yaşıyor (D-034):** `Session` `Send` ama
`Sync` değil ve `import_archive`'ın future'ı `Send` değil, Tauri ise komut
future'larının `Send` olmasını istiyor. Kilit yerine kanal: komutlar çekirdek
iş parçacığına bir kapanış gönderip cevabı bekliyor, tik döngüsü de aynı
iş parçacığında. Çekirdek bunun için değişmedi.

Doğrulandı: pencere açılıyor, `environment`/`queue`/`anchor` cevaplıyor,
tarama ve arama sonuçları CLI'nin `--json` çıktısıyla aynı sayıları veriyor,
yerel dosya çalıyor ve ilerleme çubuğu çapadan yürüyor. Hata yolu da
görüldü — `ADIM: PLAYBACK_RESOLVE` aşamasıyla, tam zinciri katlanmış hâlde.

### 3.3 Tema API'si — sürümlenmiş sözleşme
Spicetify'ın en büyük derdi: üst uygulama değişiyor, temalar bozuluyor.
Bunu yaşamamak için semantik token seti (CSS custom properties), taahhüt edilen
slot isimleri ve **sürümlenmiş tema formatı** baştan tasarlanır.
İçeride ne değişirse değişsin bu yüzey sabit kalır.

**D-028'den gelen kısıt — sözleşme neyin canlandırılabileceğini de söylemeli.**
Düzeltilmiş ortamda bile `height` / `box-shadow` / `filter` /
`background-position` animasyonları 58.8 → 47.6 fps götürüyor; `transform` +
`opacity` hiç düşürmüyor. Tema yazarına bu söylenmezse fark **kullanıcının**
makinesinde ortaya çıkar ve suçlanan tema değil uygulama olur.

**Dil kapandı (D-036):** token adları — ve genel olarak bütün tanımlayıcılar —
İngilizce. Tema seti bu projenin en dışa dönük yüzeyi; onu tüketen tanımadığımız
bir tema yazarı. Yorum ve arayüz metni Türkçe kalır. Bu karar uygulanırken
`crates/tune`, paylaşılan doğruluk kümesi ve ses fixture'ları da çevrildi.

> KARAR NOKTASI: Token setini yazmadan önce sun. Bu bir kez yayınlandıktan sonra
> geriye dönük uyumluluk borcu doğar.
>
> Cevaplanacak dört soru: (1) granülerlik — dar semantik küme mi, bölge
> geçersiz kılmalı katmanlı küme mi; (2) seçici vaadi — `data-tune="..."`
> öznitelikleri mi, sınıf adları mı, hiçbiri mi; (3) temaya IPC açılacak mı
> (D-033 bunu buraya bıraktı: açılırsa sözleşme *dış* sözleşmeye döner ve o
> gün sürümleme borcu doğar); (4) paket biçimi ve `api` sürüm alanı.
>
> **KAPANDI — D-037:** (1) dar semantik küme; (2) sınıf adları sözleşmenin
> parçası (mevcut `.topbar` vb. artık stabil, D-036'nın "iç ayrıntı" varsayımı
> burada geçersiz); (3) hayır, temalar yalnızca görünüm — IPC yok; (4) manifest
> + `api` sürüm alanı, uyuşmazsa açıkça reddedilir.
>
> **Yeni açık madde — "mod" paketleri.** D-037'de kullanıcı, tema paketleriyle
> birlikte dağıtılan, davranış değiştiren bir "mod" kavramı önerdi. Bilinçli
> olarak ertelendi. Bir mod'un webview içinde çalışıp IPC çağırması K4'ün
> sağlayıcı eklentileri için şart koştuğu "alt süreç + JSON-RPC" modelinden
> köklü biçimde farklı bir güvenlik sınıfı taşır (aynı webview'de çöküp bütün
> arayüzü dondurabilir). §3.3 kapandıktan sonra ayrı bir tur gerekiyor:
>
> KARAR NOKTASI: Mod'lar nasıl çalışır — ayrı bir süreç mi (K4 modeli, ama o
> zaman "webview içinde" vaadi düşer), yoksa sınırlı/izinli bir webview JS
> sandbox'ı mı? IPC'nin hangi alt kümesi (varsa) açılır? Tema ile aynı paket
> biçimini mi paylaşır yoksa ayrı mı? **Sor.**

#### Token seti v1 — TAMAM (D-037)

`crates/tune/ui/style.css`'in `:root` bloğu artık sözleşmenin kendisi. On dört
token, hepsi dosyada en az bir yerde kullanılıyor — kullanılmayan token yok:

| Token | Değer | Anlamı |
|---|---|---|
| `--tune-color-scheme` | `dark` | motorun kendi çizdiği parçalar (onay kutusu, imleç, kaydırma çubuğu) |
| `--tune-bg` | `#12100e` | sayfa arka planı |
| `--tune-surface` | `#1b1815` | panel arka planı (topbar, sidebar, player, block) |
| `--tune-surface-raised` | `#221e1a` | etkileşimli/hover arka planı (input, aktif nav, hover, toast) |
| `--tune-border` | `#2e2925` | kenarlık, ayraç, ilerleme çubuğu izi |
| `--tune-text` | `#eae2d8` | birincil metin |
| `--tune-text-dim` | `#9a8f83` | ikincil/soluk metin |
| `--tune-accent` | `#ffb454` | marka + etkileşim vurgusu |
| `--tune-success` | `#7bd88f` | çalıyor / başarı durumu |
| `--tune-error` | `#ff6b6b` | hata durumu |
| `--tune-info` | `#79c0ff` | arabelleğe alma / bilgi durumu |
| `--tune-radius-sm` | `6px` | kontrol köşe yarıçapı (nav, buton, girdi, kuyruk satırı) |
| `--tune-radius-lg` | `8px` | panel köşe yarıçapı (block, toast, dropzone) |
| `--tune-duration` | `120ms` | tek canlandırma süresi (yalnızca `opacity`/`transform`, D-028) |

`--tune-color-scheme` on üçüncü değil **on dördüncüdür**: ilk sette yoktu,
§3.4'ün açık teması yazılırken ortaya çıktı ve sonradan eklendi (D-039).
Token'lar yalnızca *bizim* çizdiğimiz renkleri değiştiriyor; onay kutusu,
metin imleci ve kaydırma çubuğu motorun kendi çizdiği parçalar ve açık bir
palette koyu kalıyorlardı. Referans temanın işi tam olarak buydu.

Hover/focus/disabled ayrı renk token'ı almadı — hover zaten var olan
token'ların bileşimiyle ifade ediliyor (`.nav:hover` → `--tune-text`,
`button:hover` → `--tune-accent` kenarlık). Şu an hiçbir kural `:focus-visible`
veya devre dışı durum tanımlamıyor; kullanılmayan bir token yazmak yerine bu
**bilinen bir boşluk** olarak bırakıldı — gerçek bir görsel kural yazılınca
token da gelir.

Sınıf adları (D-037/2) artık sözleşmenin parçası: `.topbar`, `.sidebar`,
`.nav`, `.content`, `.queue` (+ `.index`, `.current`), `.player`, `.controls`,
`.now`, `.state.{playing,paused,buffering,stopped}`, `.bar`, `.fill`, `.time`,
`.toast` (+ `.stage`, `.info`), `.block`, `.summary` (+ `.big`, `.label`),
`.table`, `.ranking`, `.dropzone` (+ `.over`), `.busy`, `.brand`, `.check`,
`.row`, `.grid`, `.hint`, `.empty`, `.two-col`, ve tema ekranının kendisi:
`.themes`, `.theme` (+ `.name`, `.author`, `.current`), `.tag` (+ `.warn`),
`.rejected` (+ `.id`).

Yeni sınıf **eklemek** `api`'yi artırmaz — eski temalar onu hedeflemiyordu.
Artıran şey var olanı kaldırmak ya da anlamını değiştirmek. Aynı kural
token'lar için de geçerli, ve `--tune-color-scheme` bunun ilk örneği.

#### Manifest biçimi ve yükleyici — TAMAM

Bir tema iki dosyalı bir dizin:

```
<tema-dizini>/
  theme.json   # manifest
  theme.css    # yalnızca :root { --tune-*: ...; } — ek seçici yok
```

`theme.json`:

```json
{
  "name": "Örnek Tema",
  "author": "Birisi",
  "api": 1
}
```

`api`, bu tablodaki token/sınıf sözleşmesinin sürümü. Uygulama başlangıçta
kendi desteklediği `api` sayısıyla karşılaştırır; uyuşmazsa temayı **sessizce
yok saymaz**, hangi temanın hangi sürüm beklediğini söyleyip reddeder (K9).
`theme.css`'in `:root` dışına taşması (ör. `.topbar { ... }` yazıp yeni bir
seçiciye dayanması) şimdilik engellenmiyor — motoru CSS, kısıtlamak ayrı bir
adım (aşağıya bak).

> KARAR NOKTASI (uygulama öncesi, küçük): `theme.css` `:root` dışına
> yazarsa (örn. doğrudan `.topbar` hedefleyip token'ları atlarsa) reddedilsin
> mi, yoksa serbest mi bırakılsın? Reddetmek sözleşmeyi token seviyesinde
> kilitler ama tema yazarına en ufak esnekliği (örn. tek bir öğeye özel bir
> `box-shadow` eklemek — ki D-028 zaten bunu önermez) kapatır.
>
> **KAPANDI — D-038: ne reddet ne serbest, işaretle.** Yükleyici yükler ama
> tema listesinde "genişletilmiş / garantisi yok" diye etiketler. Garanti
> her zaman yalnızca `:root`'taki token'lar için geçerli.

**Uygulandı:** `crates/tune/src/theme.rs` — çekirdekte değil kabukta, çünkü bu
tune-core'un taşıyacağı bir domain kavramı değil, webview'e özgü bir CSS
mekanizması. Mobil bağlamalar CSS custom property kullanmayacak; Altın
Kural'ın "çekirdek bunu sunabilir mi" testi burada `crates/tune`'u işaret
ediyor. Üç IPC komutu: `themes_list`, `theme_active`, `theme_select`.

Seçim `<data_dir>/ui.json`'da; şema değişmedi, veritabanına dokunulmadı.
Depo durum tutmuyor, her çağrı diski okuyor — tema yazarı dosyayı düzenleyip
"listeyi yenile" deyince değişikliği görüyor, uygulamayı kapatması gerekmiyor.

Dört davranış K9'a bağlı ve testle kilitli (18 test):

- **`api` uyuşmazlığı reddedilir ve iki sürüm de yazılır** ("tema sürüm 9
  istiyor, bu yapı sürüm 1 sunuyor"). "Temam görünmüyor" bir tanı sorusu
  olmamalı.
- **`:root` dışına taşan tema yüklenir ama işaretlenir** (D-038): listede
  "genişletilmiş · garantisi yok".
- **Seçili tema silinmişse varsayılana dönülür ama sebebiyle.** Sessiz dönüş,
  kullanıcının temasının neden kaybolduğunu hiç öğrenememesi olurdu.
- **Yerleşik bir temanın adını gölgeleyen dizin sessizce kazanmaz**, sebebiyle
  reddedilir — hangi dosyanın kazandığı tahmin edilmemeli.

Sıra da kasıtlı: **önce doğrula, sonra yaz.** Yüklenemeyen bir temayı seçim
olarak kaydetmek, uygulamayı bir dahaki açılışta bozuk bir seçimle
başlatırdı; test bunu kilitliyor.

### 3.4 Referans temalar — TAMAM
En az iki farklı temada tema API'sinin yeterli olduğunu kanıtla.

**İkisi de `crates/tune/themes/` altında ve uygulamayla birlikte geliyor**
(`include_str!`) — ama **ayrıcalıkları yok**: diskteki bir tema gibi aynı
doğrulamadan geçiyorlar. Ayrıcalıklı olsalardı sözleşmenin yeterli olduğunu
değil yalnızca kendilerini kanıtlarlardı.

| Tema | Sınadığı eksen |
|---|---|
| **Gün Işığı** (`daylight`) | renk — temel arayüz koyu yazılmıştı, aynı sözleşmeyle açık bir arayüz çıkıyor |
| **Yüksek Karşıtlık** (`contrast`) | renk **dışı** — `--tune-radius-*` sıfıra iniyor, `--tune-duration` `0ms` |

İkinci tema bilerek ikinci bir palet değil: iki paletle "iki tema" ölçütü
kâğıt üstünde karşılanır ama token setinin renk dışındaki ekseni hiç
sınanmamış olurdu. Yarıçap ve süre token'ları gerçekten kullanılıyorsa bu
temada görünür — kullanılmıyorsa ölü token demektir. Test bunu kilitliyor
(`the_two_reference_themes_differ_on_more_than_color`).

**Ve sözleşme yetmedi.** Açık tema token'ların hepsini doğru uyguladığı hâlde
onay kutuları, metin imleci ve kaydırma çubuğu koyu kalıyordu: bunları biz
değil motor çiziyor. Eksik `--tune-color-scheme` olarak kapandı (D-039).
§3.4'ün "yeterli olduğunu kanıtla" ölçütü tam olarak bunu yakalamak içindi ve
yakaladı — iki tema yazılmasaydı eksik, ilk tema yazarının makinesinde
ortaya çıkardı.

**Uçtan uca doğrulandı** (2026-09-01, WebKitGTK): seçim `ui.json`'dan
okunup açılışta uygulanıyor, tema ekranı yerleşik/genişletilmiş/reddedilen
üç durumu da doğru gösteriyor, `api: 9` isteyen tema listede sebebiyle
duruyor.

---

### 3.5 Faz 3 durum — KAPANDI

**235 test**, clippy ve fmt temiz.

| Bölüm | Durum |
|---|---|
| 3.1 GO/NO-GO ölçümü | TAMAM — koşullu GO (D-028), ortamı uygulama kuruyor (D-029/D-031) |
| 3.2 IPC sözleşmesi | TAMAM — çekirdeğin yüzeyi + serde (D-033), olaylar değişimde (D-035) |
| 3.3 Tema API'si | TAMAM — 14 token + sınıf adları + manifest/yükleyici (D-037…D-039) |
| 3.4 Referans temalar | TAMAM — `daylight` (renk) + `contrast` (yarıçap/süre) |

Tema yazarına dönük belge: `crates/tune/themes/README.md`.

**Faz 3'ten devredilen açık karar:** "mod" paketleri — davranış değiştiren,
tema paketleriyle birlikte dağıtılan eklentiler. §3.3'te bilinçli ertelendi;
karar noktası metni orada duruyor. Tema sözleşmesi kapandığı için artık ayrı
bir tur olarak açılabilir. **Faz 3'ü eksik bırakmıyor:** temalar bugün
tasarlandıkları gibi çalışıyor ve mod'lar onların üstüne değil yanına gelecek.

Sıradaki iş **Faz 2** (D-027'de ertelenmişti, iptal edilmemişti): eklenti
sınırı ve sağlayıcı genişlemesi. Faz 2'nin ilk karar noktası eklenti izin
modeli (§2.1) ve oraya Faz 1'den iki borç taşınıyor — `keyring` (D-021) ve
gerçek zamanlı dizin izleme (D-025).

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

# EK — Yayın platformları

"Hangi platformdan çalabiliriz" sorusunun cevabı burada durur.

> **Buradaki API bilgileri doğrulanmadı.** Platformlar arayüzlerini haber vermeden
> değiştirir. Bir eklenti yazılmadan önce ilgili satır yeniden sınanır — ASLA YAPMA
> listesindeki "bilmediğin bir API şeklini varsayma" buraya da geçerli.

## Ayırıcı çizgi teknik değil, hukuki

Soru "resmî API var mı" değil. Soru: **ses DRM ile korunuyor mu?**

- **DRM yok** → eklenti akışı doğrudan çözebilir. Belgelenmemiş bir API kullanmak
  hizmet şartlarını ihlal edebilir; bu bir risk, ama ayrı bir suç değil.
- **DRM var** (Widevine, FairPlay, Deezer'ın Blowfish'i) → akışı açmak bir
  **teknolojik koruma önlemini aşmaktır.** Bu çoğu ülkede ayrı bir kanun maddesidir
  (DMCA §1201, EU 2001/29 m.6) ve telif ihlalinden bağımsız olarak yasaktır.
  **Bu projede yazılmaz.** D-002 gereği bu yayınlanacak bir üründür; kişisel kullanım
  muafiyeti yoktur — K4'ün Spotify için dediğinin aynısı.

DRM'li platformlar için geriye iki yol kalır: platformun **kendi** oynatıcısını
sürmek (`CONTROL`), ya da yalnızca metadata almak.

## Tablo

| Platform | Yetenek | Gerekçe |
|---|---|---|
| **SoundCloud** | `SEARCH BROWSE STREAM` | DRM yok. Resmî API var ama anahtar başvurusu yıllardır kapalı/aralıklı; pratik yol yt-dlp. CLAUDE.md'nin K5 örneği zaten bu. Bazı parçalar (Go+, gizli) erişilemez — eklenti bunu **sayıp raporlar**, sessizce atlamaz (K9). |
| **Qobuz** | `SEARCH BROWSE STREAM` | DRM yok; FLAC aboneye şifresiz iniyor. Resmî public API yok, tersine mühendislikle biliniyor. Abonelik şart. Hi-res katalog en temiz kaynak. |
| **YouTube Music** | `SEARCH BROWSE STREAM` | DRM yok, yol yt-dlp. **Bakım maliyeti en yükseği:** YouTube aktif olarak zorlaştırıyor (nsig, PO token). Tam da bu yüzden eklenti — bozulduğunda çekirdek ayakta kalır. |
| **Tidal** | `SEARCH BROWSE` | Resmî geliştirici API'si katalog/arama veriyor. Tam akış partner programına bağlı, yüksek kalite katmanları DRM'li. Tidal Connect sertifikalı cihaz programı, herkese açık değil. |
| **Apple Music** | `SEARCH BROWSE` | MusicKit **resmî ve meşru** — developer token + user token ile katalog ve kullanıcının kütüphanesi alınır. Ama çalmak MusicKit çalışma zamanı ister: Linux'ta yok, WebKitGTK'da FairPlay yok. Faz 6'nın iOS/macOS sürümlerinde `STREAM` açılabilir. |
| **Deezer** | `SEARCH BROWSE` | Public API metadata için resmî ve iyi. Akış Blowfish ile şifreli → koruma önlemi → çizginin öbür tarafı. |
| **Spotify** | `CONTROL` | K4. Ayrı depo, ayrı paket; çekirdeğin bağımlılık ağacında görünmez. Web API'nin player uçları Premium ister. librespot ToS ihlali. Metadata çekilmez, veritabanına yazılmaz — geçmiş export yoluyla gelir (K2). |
| **Amazon Music** | — | Public API yok, Widevine. Metadata bile alınamıyor. |
| **Pandora** | — | 2011'den beri public API yok, ABD'ye kilitli. Ayrıca **model uyumsuz:** biz parça adresliyoruz, Pandora istasyon adresliyor. |
| **Idagio** | — | Public API yok, DRM'li. Ayrıca veri modeli farklı (eser / bölüm / icra), parça değil. Klasik müzik kimliği kendi başına bir iş — sağlayıcı işi değil, `identity/` işi. |
| **Tencent (QQ/Kugou/Kuwo)** | — | Anakara Çin'e kilitli; Çin telefonu ve ödeme yöntemi şart, akışlar şifreli. **Ama K5'in var oluş sebebi tam olarak bu:** o bölgedeki biri eklentiyi kendi yazabilir. Biz protokolü veririz, listeyi değil. |

## İçe aktarma tarafı bu çizgiyi tanımıyor

K2'nin bütün meselesi bu: **export bir yasal haktır, hizmet şartı onu kısıtlayamaz.**
Tablonun "kapalı" satırları bile dinleme geçmişini verir.

GDPR/CCPA kapsamında export veren: Spotify (yapıldı, §0.2), Apple, Google/YouTube
(Takeout), Deezer, Qobuz, Tidal, SoundCloud, Amazon. Pandora ve Tencent daha zayıf —
talep üzerine ve biçimleri belgelenmemiş.

Yani: **çalabildiğimiz platform üç, dinleme kimliğini alabildiğimiz platform on.**
Ürün ikincisiydi — CLAUDE.md'nin ilk cümlesi.

---

# ASLA YAPMA

- CLI'ye iş mantığı koyma
- Sağlayıcı API'sinden geçmiş/kütüphane çekme — export kullan
- Sunucudan ses akıtma
- **DRM'li bir akışı çözen kod yazma** (D-026) — Widevine, FairPlay, Deezer'ın
  Blowfish'i. Koruma önlemi aşmak telif ihlalinden ayrı bir kanun maddesidir;
  D-002 gereği kişisel kullanım muafiyeti yok. Bkz. EK — Yayın platformları
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
