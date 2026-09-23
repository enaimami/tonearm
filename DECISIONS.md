# DECISIONS.md — Karar Defteri

Her cevaplanan soru buraya yazılır. Aynı soru iki kez sorulmaz.
Bir şeye karar vermeden önce bu dosyayı oku.

---

## D-001 — Mobil hedefte mi?
**Tarih:** 2026-08-28
**Karar:** Evet. Uygulama sonraki fazlarda, ama **API tasarımı bugünden mobile uyumlu olacak.**
**Gerekçe:** Sonradan eklemek çekirdek API'sinin baştan yazılması demek.
**Sonuç:** Değişmez Kural K7 bağlayıcıdır. `uniffi` ile ifade edilemeyen public imza kabul edilmez.

---

## D-002 — Kişisel araç mı, yayınlanacak ürün mü?
**Tarih:** 2026-08-28
**Karar:** Yayınlanacak. Etrafında topluluk hedefleniyor.
**Sonuç:**
- Faz 4 (odalar, sunucu) kapsam dahilinde.
- Lisans kararı gerekli (bkz. D-005, açık).
- Spotify konusunda K4 katı uygulanır — kişisel kullanım muafiyeti yok.
- Public repo hijyeni gerekli: README, CONTRIBUTING, davranış kuralları, sürüm notları.

---

## D-003 — Yerel müzik arşivi var mı?
**Tarih:** 2026-08-28
**Karar:** Hayır. Test için fixture üretilebilir.
**Sonuç:**
- Faz 1 (oynatma) geliştirici tarafından **dogfood edilemez**. Bu, faz sırasını etkiler (bkz. D-004).
- Faz 1 başladığında önce küçük, telifsiz test fixture'ları üretilecek
  (Creative Commons / public domain kayıtlar, kısa süreli).

---

## D-004 — Sleeve ve topluluk hedefi, faz sırası
**Tarih:** 2026-08-28
**Karar:** Sleeve açık bir hedef. Aralık yıl sonu kartı sezonu (Spotify Wrapped®) takvimi belirliyor.
**Sonuç:**
- **Yeni Faz 0.5 eklendi:** paylaşılabilir Sleeve kartı üretimi.
- Gerekçe: Faz 0'ın çıktısı terminal metni. Terminal metni yayılmaz. Topluluk
  hedefi paylaşılabilir bir görsel artefakt gerektiriyor ve o şu an planda yoktu.
- Faz 0.5 çekirdekte yaşar (GUI ve mobil aynı üreticiyi kullanacak), CLI'den sürülür.

---

## D-005 — Lisans
**Tarih:** 2026-08-29 · **Durum:** KAPANDI — **MIT OR Apache-2.0**
**Karar:** MIT OR Apache-2.0. Cargo.toml'deki yer tutucu gerçek karara dönüştü.
**Gerekçe:** Rust ekosistemi standardı; benimsemeyi maksimize eder. Eklentiler alt
süreç + JSON-RPC konuştuğu için (K5) çekirdeğin lisansı eklenti yazarlarını yasal
olarak neredeyse bağlamıyor — copyleft'in asıl faydası burada gerçekleşmiyor.
**Sonuç:**
- Faz 0.5.4'te `LICENSE-MIT` ve `LICENSE-APACHE` dosyaları repo köküne konur.
- Faz 4'ün sunucu bileşeni ayrı crate/repo olursa orada AGPL **ayrıca**
  değerlendirilir — bu karar yalnızca çekirdek/CLI için.

**Uygulandı (2026-08-29).** Her iki lisans dosyası repo kökünde; MIT'te telif
sahibi `enaimami`. `CONTRIBUTING.md` katkının çift lisans altında yayımlanmayı
kabul ettiğini söylüyor. Cargo.toml'deki `license` alanı zaten doğruydu, artık
yer tutucu değil karar.

---

## D-006 — `MetadataLookup` generic'i (rapor Bulgu 3)
**Tarih:** 2026-08-28 · **Durum:** UYGULANDI (2026-08-28)
**Karar:** D-001 gereği bu **bugün bir K7 ihlalidir** ve düzeltilecek.
**Mevcut durum:** `Session::import_archive<L: MetadataLookup>` ve
`Session::resolve_track<L: MetadataLookup>` generic parametre taşıyor. `uniffi` generic ifade edemez.
**Yön:** Generic yerine `Arc<dyn MetadataLookup>`. `uniffi` bunu **callback interface**
olarak modelleyebilir (`#[uniffi::export(with_foreign)]`), yani yabancı dilde (Kotlin/Swift)
uygulanan bir trait olarak geçer.
**Not:** K7'nin ilk yazımı "trait object yok" diyordu; bu fazla katıydı ve düzeltildi.
`Arc<dyn Trait>` uniffi'nin desteklediği yoldur. Yasak olan **generic** ve **lifetime**.

**Uygulama:** `MetadataLookup`, `dyn` uyumlu olabilmek için `-> impl Future`
yerine `LookupFuture<'a, T> = Pin<Box<dyn Future<..> + Send + 'a>>` döndürüyor
(`async-trait` makrosunun elle yazılmış hâli — yeni bağımlılık eklenmedi).
Trait'e `Send + Sync` eklendi. `Resolver<L>` de generic'ini bıraktı:
`Resolver { lookup: Arc<dyn MetadataLookup> }`. `session::default_lookup()`
artık somut `OfflineLookup` değil `Arc<dyn MetadataLookup>` döndürüyor, böylece
kaynak değiştiğinde çağıran imza görmüyor. Çekirdeğin dışa açık yüzeyinde
generic parametre kalmadı.

---

## D-007 — `diag` hata zincirinde tekrar eden ilk satır (rapor Bulgu 1)
**Tarih:** 2026-08-28 · **Durum:** UYGULANDI (2026-08-28)
**Karar:** Hata, düzeltilecek.
**Sebep:** `diag/mod.rs:233` zinciri `err.to_string()` ile başlatıyor. `crate::Error`'ın
Display'i `#[error("ADIM: {stage}")]` olduğu için `error_chain[0]`, `failed_at`'ten zaten
basılan başlığın kopyası oluyor.
**Düzeltme:** Zincir `err`'in kendisinden değil, `err.source()`'tan başlasın.
`error_chain` yalnızca gerçek nedenleri içersin.

**Uygulama:** `diag::error_chain` boş `Vec` ile başlayıp `err.source()`'tan
yürüyor. `error_chain_does_not_repeat_the_stage_header` testi hem zincirin
`"ADIM:"` ile başlayan satır içermediğini hem de `render()` çıktısında başlığın
tam olarak bir kez geçtiğini doğruluyor.

---

## D-008 — "çalma" etiketinin iki anlamı (rapor Bulgu 2)
**Tarih:** 2026-08-28 · **Durum:** UYGULANDI (2026-08-28)
**Karar:** Etiket değil, **kavram** düzeltilecek.
**Sebep:** `library search` ham dinlemeyi sayıyor (eşik yok), `stats` ise `min_ms_played`
(30 sn) eşiğinin üstündekileri. Aynı fixture'da "18 çalma" ve "9 çalma" çıkıyor.
**Düzeltme:** Çekirdekte tek bir paylaşılan kavram tanımlanır:
- `play_count` → eşiği geçen, "sayılan" çalma (scrobble konvansiyonu: ≥30 sn veya parçanın yarısı)
- `listen_events` → ham olay sayısı, yalnızca `diag` ve hata ayıklamada görünür
Her iki yüzey de aynı hesabı çağırır; SQL iki yerde ayrı yazılmaz. Bu, aynı hatanın
tekrar doğmasını engeller.

**Uygulama:** Kural `model::PlayRule` içinde tek yerde tanımlı; `PlayRule::counts`
scrobble konvansiyonunu uyguluyor (≥ eşik **veya** parçanın yarısı — yarım-parça
kolu yalnızca süre biliniyorsa çalışır). `StatsQuery::play_rule()` ve
`ListenStore::search(.., rule)` aynı örneği kullanıyor. SQL tarafı elle yazılmıyor:
`library::play_predicate_sql(rule)` kuralı tek bir yerde SQLite ifadesine çeviriyor
ve `sql_play_rule_agrees_with_rust` testi gerçek SQLite üzerinde 11 kenar durumda
Rust ile SQL'in aynı cevabı verdiğini kilitliyor.

**Adlandırma:** `SearchHit.plays` → `SearchHit.play_count`. Ham olay sayısı
kullanıcı yüzeyinden çıktı; `SearchOutcome.listen_events` olarak yalnızca tanı
sayaçlarına yazılıyor (`search.play_count`, `search.listen_events`).
`headshell library search` de `stats` gibi `--min-ms` alıyor ki iki yüzey aynı eşikle
sürülebilsin.

**Ölçülen etki:** `spotify_extended_mini` fixture'ında `Creep` satırı
`plays: 18` iken `play_count: 9` oldu — `stats`'ın saydığıyla birebir aynı.

---

## D-009 — Kimlik doğruluk kümesi yetersiz
**Tarih:** 2026-08-28 · **Durum:** UYGULANDI (2026-08-28)
**Karar:** 15/15 = %100 geçerli bir sinyal değil. Küme zor vakalarla büyütülecek.
**Gerekçe:** CLAUDE.md bu sayıyı "projenin en önemli metriği" olarak tanımlıyor.
Kolay bir kümede %100, ölçüm yapılmadığı anlamına gelir.
**Eklenmesi gereken vaka sınıfları:**
- Remaster / Deluxe / Anniversary sürümleri (aynı kayıt sayılmalı)
- Live kayıtlar (stüdyo versiyonundan **ayrı** olmalı)
- Cover'lar (eşleşme**meli** — negatif vaka)
- `feat.` / `ft.` / `with` varyasyonları
- Sanatçı adında Türkçe karakter ve transliterasyon (Müslüm Gürses / Muslum Gurses)
- Klasik müzik: besteci ve icracı ayrımı
- Aynı ada sahip farklı sanatçılar
- Uzun/kısa (radio edit) versiyonlar
**Hedef:** en az 60 vaka, en az 15'i negatif (eşleşmemesi gereken).

**Uygulama:** Küme 15 vakadan **69 vakaya** çıkarıldı; 21'i negatif (eşleşmemesi
gereken). Her vaka bir `class` etiketi taşıyor ve test toplam oranın yanında
**sınıf bazında** kırılım da basıyor — toplam oran tek bir sınıftaki çöküşü
gizleyebiliyor. Testin kendisi de kümeyi koruyor: 60 vakadan ve 15 negatiften
aşağı düşerse başarısız oluyor.

Şema iki alanla genişledi: `class` ve `expect_method`. İkincisi "katalogda
olmayan ISRC yine de otoritedir, ama MBID'e değil ISRC kimliğine bağlanır"
gibi vakaları ifade edebilmek için gerekliydi.

**İlk ölçüm (küme büyüdü, algoritma eski): 62/69 = %89.9.** Yani D-009 haklıydı —
15/15 = %100 ölçüm yapılmadığı anlamına geliyormuş. Hatalar dağınık değil, iki
gerçek kusurda toplandı:

1. **`live` sınıfı 1/4.** `normalize::strip_edition_suffixes` "live"i atılabilir
   bir sürüm eki sayıyordu; `Creep (Live at Glastonbury)` stüdyo kaydına
   **%100 güvenle** bağlanıyordu. Bu, D-009'un açıkça yasakladığı davranıştı.
2. **`cover` 3/5, `remix` 1/2.** Başlık ağırlığı (0.6) tek başına eşiğe
   dayanabiliyordu: `The Rock Tribute Band - Karma Police` 0.90 güvenle
   Radiohead kaydına bağlanıyordu.

**Düzeltmeler:**
- Etiketler ikiye ayrıldı. `REISSUE_MARKERS` (remaster, deluxe, mono, radio edit…)
  *aynı kaydın* yeniden yayımıdır, atılır. `VARIANT_MARKERS` (live, remix,
  acoustic, karaoke, demo, unplugged, cover, instrumental, reprise) *başka bir
  kayıttır*, atılmaz. `(Live Version)` gibi ikisini birden içeren ekte varyant
  kazanır.
- `fuzzy::similarity` metin benzerliğinin üstüne iki ayrık kural aldı:
  **varyant uyuşmazlığı** (bir tarafta `live` var diğerinde yok → ×0.5) ve
  **sanatçı tabanı** (`ARTIST_MIN_SIMILARITY = 0.7`; altındaysa ×0.5).
  Ceza uyuşmazlıkta, varlıkta değil: iki canlı kayıt birbirine ceza almaz.

**Son ölçüm: 68/69 = %98.6.** Sınıf bazında yalnızca `radio_edit` 2/3 — o vaka
bir kusur değil, cevaplanmamış bir soruydu; D-010 ile karara bağlandı ve küme
**69/69 = %100**'e ulaştı. Test eşiği %90 → %95 → **%97**.

> Bu %100, D-009'un eleştirdiği %100 değil: küme 69 vaka, 22'si negatif ve
> `live` / `cover` / `classical` / `same_title` sınıfları algoritmanın gerçekten
> düştüğü yerlerdi. Yine de tek bir kümede %100, kümenin tükendiği anlamına
> gelir: ağ ve gerçek MusicBrainz verisi geldiğinde yeni hata sınıfları
> eklenmeli — kural aynı, önce vakayı yaz, testin düştüğünü gör.

---

## D-010 — Radio edit orijinaliyle aynı kayıt sayılmalı mı?
**Tarih:** 2026-08-28
**Karar:** **Hayır — ayrı kayıt (Seçenek B).** Kanonik kimlik kayıt (recording)
düzeyindedir; MusicBrainz de radio edit'i ayrı bir recording kabul eder.
**Gerekçe:** Süre cezası, kimlik zincirinin canlı kayıt ve cover yakalayan en
güçlü sinyali; D-009'da `live` sınıfını 1/4'ten 4/4'e çıkaran şeyin parçası.
Onu, ölçemediğimiz bir kullanıcı rahatlığı için gevşetmek kazanılmış doğruluğu
harcamak olurdu. "Aynı şarkının farklı kayıtlarını tek satır göster" ayrı bir
katmandır (work / release-group gruplaması) ve MusicBrainz verisi geldiğinde
doğru yerde çözülür — Faz 0'ın işi kimliği *doğru* kurmak.
**Sonuç:** Kod değişmedi; `Underworld - Born Slippy .NUXX - Radio Edit` vakası
`expect_mbid: null` olarak yeniden etiketlendi. **Doğruluk 69/69 = %100**,
negatif vaka 22. Test eşiği %95 → **%97** (tek vakalık gerileme testi düşürür).

**Kayda değer nüans:** Kümedeki diğer iki radio/extended edit vakası pozitif
kaldı, çünkü süreleri bilinmiyor. Etiketleme "bu şarkı ne", değil "bu kanıtla
zincir ne yapmalı" sorusunu kodluyor: süre farkı bir edit'i kanıtlıyorsa
eşleşme reddedilir, kanıt yoksa zincir bulanık güvenle en iyi tahminini verir.
Bu ayrım kasıtlıdır; ağ ve gerçek süre verisi geldiğinde yeniden ölçülmeli.

**Bağlam:** D-009 kümesindeki tek başarısız vaka:
`Underworld - Born Slippy .NUXX - Radio Edit` (240 sn) katalogdaki 570 sn'lik
kayda bağlanmıyor. Sebep, kusur değil kuralların çatışması: "radio edit" bir
yeniden yayım eki sayılıp atılıyor (başlık birebir eşleşiyor), ama 330 sn'lik
süre farkı `DURATION_MISMATCH_MS` cezasını tetikleyip skoru 0.88'in altına
indiriyor. Süre farkı küçük olan radio edit'ler (örn. `Aerodynamic (Radio Edit)`,
süre bilinmiyor) eşleşiyor.

**Seçenek A — radio edit orijinaliyle birleşsin.** Kullanıcı "aynı şarkıyı
dinledim" der; istatistikte tek satır görmek ister. Uygulaması: süre cezası
başlıklardan biri bir uzunluk eki taşıyorsa gevşetilir.
*Artı:* kullanıcı sezgisine uyar, Sleeve'da parça sayısı bölünmez.
*Eksi:* süre cezası kimlik zincirinin canlı kayıt/cover yakalayan en güçlü
sinyali; gevşetmek D-009'da yeni kazanılan `live` ve `cover` sınıflarını
riske atar.

**Seçenek B — ayrı kayıt sayılsın (bugünkü davranış).** MusicBrainz de radio
edit'i ayrı bir *recording* kabul eder; kanonik kimliğimiz kayıt düzeyinde.
Vakanın etiketi `expect_mbid: null` olarak düzeltilir ve oran 69/69 olur.
*Artı:* kimlik zinciri kayıt düzeyinde tutarlı kalır, hiçbir sinyal gevşemez.
*Eksi:* aynı şarkı istatistikte iki satır olabilir.

**Seçilen: B.**

---

## D-011 — Sleeve kartı rasterizasyonu
**Tarih:** 2026-08-29
**Soru:** `headshell sleeve --out kart.png` PNG'yi nasıl üretecek? (PLAN 0.5.3 karar noktası)
**Karar:** **resvg, opsiyonel `render-png` feature'ı arkasında.** Çekirdek her zaman
SVG üretir; PNG dönüşümü yalnızca feature açıkken derlenir. CLI feature'ı açar,
mobil bağlamalar açmaz.
**Gerekçe:** Bağımlılık ağacı küçük kalmalı (K7/mobil); resvg ağacı (tiny-skia,
fontdb, rustybuzz) büyük. Feature kapalıyken ağaç hiç büyümüyor, açıkken bitti
ölçütü (`--out kart.png`) karşılanıyor. SVG her zaman üretiliyor olduğu için
GUI/mobil istemezse PNG üretmeden kartı alabilir.
**Sonuç:**
- `headshell-core` → `resvg = { version = "0.48", optional = true }`,
  `[features] render-png = ["dep:resvg"]`.
- `headshell-cli` headshell-core'yu `render-png` ile açar.
- Feature-gated kod `sleeve/png.rs` içinde; `render_svg` koşulsuz.

**Uygulandı (2026-08-29).** `sleeve::write_card` uzantıya bakıp biçime karar
veriyor: `.svg` koşulsuz çalışır, `.png` feature kapalıysa "bu derlemede yok,
SVG kullanın" diye **açıkça** hata verir — sessizce yanlış biçim yazmaz.
Rasterizasyon `usvg` + `tiny-skia` üzerinden; `usvg::Options::default()` boş
bir fontdb ile geldiği için sistem fontları elle yükleniyor ve sans-serif
somut bir aileye bağlanıyor (yoksa metin hiç çizilmiyordu).

**Ölçülen etki:** ağaç feature kapalıyken **56 crate**, açıkken **114**.
Kararın gerekçesi doğrulandı: 58 crate'lik fark mobil bağlamaların dışında kalıyor.

## D-012 — Sleeve kartı tasarımı
**Tarih:** 2026-08-29
**Soru:** Kart, tema sisteminin (Faz 3) önizlemesi mi, sabit tasarım mı? (PLAN 0.5.3 karar noktası)
**Karar:** **Sabit tasarım, isimli iç sabitlerle.** Renkler ve ölçüler isimli sabitlerde
toplanır ama dışarıya sürümlü bir token sözleşmesi açılmaz.
**Gerekçe:** Tema token seti yayınlanınca geriye dönük uyumluluk borcu doğar
(PLAN 3.3 — Spicetify dersi). Token seti tasarlanmadan sözleşme doğurmak, Faz 3'ü
plansız öne çekmek olur. Sabitlerin isimli toplanması, ileride token'lara taşımayı
küçük bir iş yapar.
**Sonuç:** Ölçüler parametre (`CardSize`), renkler/boşluklar modül içi isimli
sabitler (`palette`, `metrics`). Faz 3'te tema API'si tasarlanırken bunlar token
setine taşınır.

**Uygulandı (2026-08-29).** `palette` beş renk (zemin, metin, soluk, vurgu,
bar rayı), `metrics` on ölçü. İkisi de `sleeve/svg.rs` içinde `mod`, dışa
açık değil — yani bugün kimse bu isimlere bağımlı olamaz ve Faz 3'te token
setine taşımak geriye dönük uyumluluk borcu doğurmaz. Kararın amacı buydu.

---

## D-013 — Sürüm numarası ve telif sahibi
**Tarih:** 2026-08-29
**Soru:** İlk public sürüm hangi numarayı taşıyacak, LICENSE-MIT'te telif kime ait?
**Karar:** Sürüm **`0.0.1-beta`**, telif sahibi **`enaimami`** (mahlas).
Repo `git init` edildi, ilk commit kullanıcının adı ve e-postasıyla atıldı,
`v0.0.1-beta` etiketi Faz 0.5 kapanışına kondu.
**Not:** Kullanıcı "0.0.1 Beta" dedi; Cargo semver'i boşluklu biçimi kabul
etmediği için `0.0.1-beta` yazıldı — aynı anlam, geçerli semver.
**Sonuç:** Sürüm alanı `workspace.package`'ta tek yerde; iki crate de oradan
alıyor. Snapshot testleri `headshell_version`'ı zaten değişken sayıp normalize
ettiği için sürüm artışı testleri kırmıyor — sonraki artışlarda da kırmayacak.

---

## D-014 — Faz 0.5 sonrası sıra: önce oynatma
**Tarih:** 2026-08-29
**Soru:** Faz 1 (oynatma) mı önce gelecek, Faz 3 (GUI + tema) mi? (PLAN Faz 1 karar noktası)
**Karar:** **Önce Faz 1 — oynatma.** Kullanıcının gerekçesi: "önce bir player'ı
halledelim ki üstüne bir şeyler kurabilelim."
**Gerekçe:** Oynatma bir *taban*; GUI ve tema onun üstüne kurulur. Tersi sırada
GUI'nin göstereceği canlı bir durum (çalan parça, kuyruk, pozisyon) olmaz ve
tema sistemi boşluğu süslemiş olur. Ayrıca Faz 1'den itibaren scrobble'ı
`headshell` üretmeye başlar — geçmiş artık hiçbir sağlayıcıda oluşmaz, ki projenin
asıl iddiası bu.
**Kabul edilen risk:** D-003 gereği geliştiricinin yerel arşivi yok, bu faz
**dogfood edilemez**. Karşılığında telifsiz fixture'larla ve testle doğrulanır;
aralık yıl sonu kartı penceresine (Spotify Wrapped®) GUI yetişmeyebilir.
**Sonuç:** Faz 3 (GUI + tema) Faz 1'den sonraya kaldı. Faz 1'in ilk işi §1.1
provider trait tasarımı — imzalar **yazılmadan önce sunulur** (K7: geri dönüşü
pahalı).

---

## D-015 — Oynatma durumunun dış yüzeyi: çapa + yoklama
**Tarih:** 2026-08-29
**Soru:** Çalma durumu GUI/mobil/TUI'ye nasıl açılacak? (PLAN §1.1, K7 gereği
imzalar yazılmadan önce soruldu)
**Karar:** **Çapa + yoklama.** `Player::anchor()` bir `PlaybackAnchor` döndürür;
tüketici pozisyonu kendisi hesaplar: `pos = position_ms + (now - wall_time) * rate`.
Observer/callback yok, sürekli pozisyon bildirimi yok.
**Gerekçe:** Üç şey aynı yere çıkıyor. (1) PLAN 3.2 zaten bunu istiyor:
"oynatma pozisyonu webview'de çapadan tahmin edilir, sürekli çekirdekten
sorulmaz" — saniyede yüzlerce IPC mesajı takılma demek. (2) Faz 4'ün oda
senkron primitifi (`anchor`) **birebir aynı şey**; bugün yazılan tip yarın
odalarda tekrar kullanılır, iki ayrı durum modeli tutulmaz. (3) `uniffi` için
düz bir record en güvenli yol — callback interface de desteklenir ama
pozisyon için kullanılırsa mobilde köprü trafiği doğurur.
**Sonuç:** Çekirdek `PlaybackAnchor { track, wall_time, position_ms, rate, state }`
tipini dışa açar. Ayrık olaylar (parça bitti → `listen` üretimi) çekirdeğin
içinde halledilir, tüketiciye olay akışı olarak sızmaz.
**Not:** Bir sonraki oturumda "olay kaçırma" sorunu çıkarsa (örneğin GUI
parçanın bittiğini geç fark ederse) `drain_events()` **eklenebilir** — çapa
yüzeyi bozulmadan yanına konur. Bugün eklemiyoruz: K10, gerekmeden yazma.


**Uygulandı (2026-08-29).** `PlaybackAnchor { track, wall_time, position_ms,
rate, state, duration_ms }` — `uniffi` için düz record. `position_at(now)`
formülü çekirdekte duruyor ki GUI/TUI/mobil üç kez yazmasın. `rate` alanı
bugün 1.0 ya da 0.0; Faz 4'ün sürüklenme düzeltmesi (PLAN 4.4) onu 1.001
gibi değerlere çekince yüzey değişmeyecek.

`PlayState`'e **`Buffering`** eklendi (ilk tasarımda yoktu): `Paused`
kullanıcının kararı, `Buffering` hattın beklemesi. İkisini birleştirmek
kullanıcıya "duraklattın" demek olurdu, oysa duraklatmadı.

Not: D-015'in "olay kaçırma çıkarsa `drain_events()` eklenebilir" maddesi
hâlâ geçerli ve hâlâ gereksiz — parça bitişi çekirdeğin içinde
(`Player::tick`) hallediliyor, tüketiciye olay akışı sızmıyor.

---

## D-016 — Ses hattı: symphonia + cpal, elle
**Tarih:** 2026-08-29
**Soru:** `rodio` (symphonia+cpal'i sarar, kuyruk/mixer/gapless hazır) mı,
yoksa PLAN §1.4'ün yazdığı gibi elle mi?
**Karar:** **symphonia (çözme) + cpal (çıkış), elle.** PLAN'ın yazdığı yol.
**Gerekçe:** Faz 4'te odaların sürüklenme düzeltmesi çalma hızını %0.1
oynatmayı gerektiriyor (PLAN 4.4); buna ancak hatta hakimsen ulaşırsın.
`rodio`'nun verdiğiyle sınırlı kalmak, sonradan onu sökmek anlamına gelirdi.
Bağımlılık ağacı da küçük kalır (K7/mobil).
**Kabul edilen maliyet:** Resampling, format dönüşümü ve gapless bizim işimiz —
Faz 1 daha uzun sürecek.

**Uygulandı (2026-08-29).** Çözme arka plan iş parçacığında, çıkış cpal geri
çağrısında; aralarında halka tamponu. Ses geri çağrısı **hiç bloklanmıyor** —
kilit alınamazsa sessizlik yazılıyor, beklenmiyor (bloklamak cızırtı üretir).

Pozisyon **çıkışa verilmiş kareden** hesaplanıyor, çözülmüş kareden değil:
aradaki fark bir tampon dolusu zamandır ve çözülene bakmak ilerleme çubuğunu
sesin önüne düşürürdü. Gerçek dosyayla ölçüldü: 1 sn'lik fixture 96ms/100ms
adımlarla ilerledi, 999ms'de bitti.

Yeniden örnekleme en yakın komşu (mono→stereo kopyalama dahil). Faz 1'in
hedefi doğru ses üretmekti; kaliteli resampling gerekirse ayrıca ölçülür.
`audio` feature'ı kapalıyken kuyruk ve çapa yine derleniyor, yalnızca ses
çıkışı düşüyor — sunucu/mobil derlemeleri ALSA'ya bağlanmıyor.

---

## D-017 — İlk sağlayıcı: yerel dosya
**Tarih:** 2026-08-29
**Soru:** Faz 1'de önce yerel dosya sağlayıcı mı (§1.2), Subsonic/Jellyfin mi (§1.3)?
**Karar:** **Yerel dosya.**
**Gerekçe:** D-003 "hangi ortam gerçekten test edilebiliyorsa o önce gelmeli"
diyor. Bu makinede `ffmpeg`, `flac` ve `lame` kurulu — telifsiz test
fixture'ları (FLAC/MP3/OGG + bozuk etiketli örnekler) üretilebilir, yani
D-003'ün dogfood kısıtı kısmen aşılır. Subsonic istemcisinin de kullanacağı
çözme/çıkış hattı önce yerelde kurulmuş olur.
**Sonuç:** §1.3 (Subsonic/Jellyfin) yerel sağlayıcı çalıştıktan sonra.
Kullanıcının elinde çalışan bir Subsonic/Jellyfin sunucusu **yok** varsayılıyor;
varsa sıra yeniden değerlendirilir.

**Uygulandı (2026-08-29).** `LocalProvider`: özyinelemeli tarama, symphonia
ile etiket okuma, etiket yoksa dosya adından türetme. `SEARCH|BROWSE|STREAM`
— `CONTROL` yok.

Fixture'lar üretildi (`fixtures/audio/`, 84 KB): `ffmpeg` sinüs tonları,
telifsiz. Etiketli FLAC/MP3, etiketsiz OGG, alt dizinde etiketsiz FLAC ve
kasten bozuk bir dosya. D-003'ün kısıtı böylece kısmen aşıldı — ses hattı
gerçek dosyalarla, gerçek aygıtta sınanıyor.

İki güvenlik kararı: `resolve_source` **indekste olmayan yolu reddediyor**
(rastgele dosya okuma yüzeyi değil), indekslendikten sonra silinmiş dosya
ise sessiz `None` değil açık hata veriyor.

---

## D-018 — Kalıcı sağlayıcı kataloğu (şema v2)
**Tarih:** 2026-08-30
**Soru:** Yerel indeks bellekteydi ve her `headshell play` çağrısı diski baştan
tarıyordu. Nereye yazılacak?
**Karar:** SQLite'ta **ayrı bir tablo**: `provider_tracks` + FTS5. Şema v2
olarak eklendi; v1 tabloları değişmedi.
**Gerekçe (asıl karar bu):** `tracks`/`listens` ile katalog **farklı ömürlere
sahip**. `tracks` "ne dinledin" — ham olaylardan türer ve K'ya göre asla
silinmez. `provider_tracks` "ne çalabilirsin" — kaynağın aynasıdır, dosya
silinince satır da gitmeli. Tek tabloda birleştirmek, diskten sildiğin bir
dosyanın geçmişini de silmek olurdu; bu, projenin en temel vaadini
(geçmiş sana ait) çiğnerdi.
**Sonuç:**
- `CatalogStore` trait'i `ListenStore`'dan ayrı.
- `headshell play` **tarama yapmıyor**; `headshell provider scan` bir kez çalışır.
- Artımlı tarama: `mtime_ms` damgası değişmemiş dosyanın etiketi yeniden
  okunmuyor. `ScanSummary.unchanged` bunu sayıyor (K9).
- `replace_catalog` kaynakta olmayan satırları düşürüyor ve kaç satır
  düştüğünü raporluyor.
- Göç var olan kurulumları bozmuyor: `migrating_a_v1_database_keeps_its_listens`
  testi v1 veritabanını kurup v2'ye yükseltiyor ve dinlemelerin durduğunu
  doğruluyor.

**Yan etki — güvenlik.** `LocalProvider::resolve_source` eskiden bellek
indeksinde arıyordu; indeks artık sağlayıcının görmediği bir tabloda.
Yerine **kök kontrolü** kondu: yol `canonicalize` edilip taranan köklerin
altında mı diye bakılıyor. `<kök>/../../etc/passwd` reddediliyor
(`resolve_source_rejects_traversal_out_of_the_roots`). Çözülemeyen yol
(silinmiş dosya) da reddediliyor — şüpheliyi kabul etmek bir dosya okuma
açığı olurdu; kullanıcıya durumu Session anlatıyor.

---

## D-019 — Faz 1.3 kapsamı: Subsonic **ve** Jellyfin
**Tarih:** 2026-08-30
**Soru:** Uzak sağlayıcı yalnızca Subsonic (OpenSubsonic) mi olsun, Jellyfin'in
kendi API'si de mi?
**Karar:** **İkisi de.** İki ayrı sağlayıcı, iki ayrı kimlik modeli.
**Gerekçe:** Jellyfin kurulumlarının çoğunda Subsonic eklentisi açık değil;
"Jellyfin destekliyoruz ama önce eklenti kur" demek desteklememektir. İki
istemcinin ortak yanı (HTTP taşıma, sunucu kaydı, kimlik saklama, akış
kaynağı) zaten paylaşılıyor; ayrışan yalnızca uç nokta ve JSON şekli.
**Sonuç:** `provider/remote/` altında ortak plumbing + `subsonic.rs` +
`jellyfin.rs`. Jellyfin ayrıca Faz 2'de eklenti sınırının müşterisi olmak
zorunda değil; eklenti sınırı kendi referans eklentisiyle sınanacak.

---

## D-020 — Ağ taşıma katmanı: trait çekirdekte, istemci feature arkasında
**Tarih:** 2026-08-30
**Soru:** `headshell-core`'un bağımlılık ağacında bugün HTTP/TLS yok. İlk ağ
çağrısı nasıl girsin?
**Karar:** **Seçenek C.** `net::HttpClient` trait'i çekirdekte, Subsonic/Jellyfin
mantığı çekirdekte; somut istemci `http-client` feature'ı arkasında (`audio` ve
`render-png` ile aynı desen). CLI feature'ı açar, mobil açmaz — kendi taşımasını
`Arc<dyn HttpClient>` olarak verir.
**Gerekçe:** Konvansiyon zaten "ağa dokunan her şey trait arkasında olsun ki
testler sahte kullanabilsin" diyordu. Trait sınırı olmadan ağ mantığı test
edilemez ve mobil kendi HTTP yığınını kullanamaz.
**Sonuç:**
- Feature içindeki crate seçimi ikincil ve geri alınabilir: **`ureq` 3.4**
  (`default-features = false`, `rustls`). `reqwest` seçilmedi çünkü `tokio`'yu
  çekirdeğe kalıcı bağımlılık yapardı — genel API'nin çalışma zamanından
  bağımsız kalması kuralı bunu dışlıyor.
- `ureq` bloklayan bir API; `UreqClient::send` async imzanın içinde bloklar ve
  bu **dokümante edilmiştir**. Çekirdek bir çalışma zamanı seçmediği için
  `spawn_blocking` çağıramaz; bloklamayan taşıma isteyen (GUI, mobil) kendi
  `HttpClient`'ını verir. Trait sınırı bu değiş tokuşu geri alınabilir kılıyor.
- Ağaç ölçümü (D-011'in yaptığı gibi), yordamı yazıyorum ki tekrar ölçülebilsin:
  `cargo tree -p headshell-core --no-default-features [--features F] --prefix none |
  sed 's/ (\*)//' | sort -u | wc -l`.

  | Feature | Crate |
  |---|---|
  | hiçbiri | **51** |
  | `http-client` | **69** (+18) |
  | `audio` | 82 (+31) |
  | `render-png` | 101 (+50) |

  Yani mobil bağlamalar 18 crate'lik TLS ağacını (ureq, rustls, ring, webpki…)
  taşımıyor. **Bu satırın ilk yazımı "56 → 112" diyordu; ölçüm değil tahmindi
  ve yanlıştı** — karar değişmiyor ama gerekçenin büyüklüğü değişiyor:
  `http-client` üç feature'ın **en ucuzu**, en pahalısı değil.

---

## D-021 — Sunucu kaydı ve kimlik bilgisi nerede durur
**Tarih:** 2026-08-30
**Soru:** Sunucu adresi + kullanıcı + parola nereye yazılacak?
**Karar:** **Veri dizininde `servers.json`**, unix'te `0600` izinle. Şema
değişmiyor (SQLite v2 olduğu gibi kalıyor).
**Gerekçe:** Kimlik bilgisi kütüphane verisi değil; `library.db` ile aynı
dosyada durması yedekleme ve paylaşma davranışlarını karıştırır. OS anahtarlığı
(`keyring`) yeni bir bağımlılık ve başsız Linux'ta kırılgan — Faz 2'deki eklenti
izin modeliyle birlikte yeniden değerlendirilecek.
**Sonuç:**
- **Subsonic:** parola diske **düz yazılmaz**. Kayıt anında rastgele bir salt
  üretilip `token = md5(parola + salt)` saklanır; her istek `u/t/s` üçlüsüyle
  gider. Bu Subsonic'in kendi kimlik yolu, uydurma değil.
- **Jellyfin:** parola `AuthenticateByName` ile bir kez erişim anahtarına
  çevrilir; saklanan şey anahtardır. Kullanıcı doğrudan API anahtarı da
  verebilir — o zaman ağa hiç çıkılmaz.
- **md5 için bağımlılık eklenmedi**, RFC 1321 çekirdekte uygulandı
  (`provider/remote/md5.rs`, RFC'nin kendi test vektörleriyle kilitli).
  Gerekçe: ağaç küçük kalmalı ve md5 burada bir güvenlik primitifi değil,
  Subsonic'in dayattığı bir tel biçimi.
- Salt entropisi `/dev/urandom`'dan; okunamazsa saat + adres tabanlı yedek
  kullanılır ve bu **sessiz değil**, kayıt notuna düşer.

---

## D-022 — Faz 1.3'ün test yolu
**Tarih:** 2026-08-30
**Soru:** Elde çalışan bir Subsonic/Jellyfin sunucusu yok. 1.3 nasıl "bitti"
sayılacak?
**Karar:** Otomatik doğrulama **testte ayağa kalkan sahte HTTP sunucusuyla**
(`std::net`, yeni bağımlılık yok). Kullanıcı ayrıca Docker ile gerçek bir sunucu
(Navidrome / Jellyfin) kuracak; 1.3 o doğrulama yapılana kadar
**"kod tamam, gerçek sunucuda doğrulanmadı"** diye açıkça işaretli kalır.
**Gerekçe:** D-003'ün dogfood kısıtı burada yeniden çıkıyor. Sahte sunucu
protokol şeklini kilitler ama gerçek bir sunucunun tuhaflıklarını (yönlendirme,
transcode, tarih biçimleri) göstermez; bunu bildiğimizi yazmak, bilmiyormuş
gibi "TAMAM" yazmaktan iyidir (K9).
**Sonuç:** Sahte sunucu **gerçek** `UreqClient` ile konuşuyor — yani taşıma
katmanı da sınanıyor, yalnızca ayrıştırıcı değil. Uçtan uca test fixture
FLAC'ını HTTP üzerinden servis edip çalıyor: `AudioSource::HttpStream`
yolu gerçekten ses üretiyor (ses aygıtı yoksa test kendini atlıyor, nedenini
`stderr`'e yazarak).

### Doğrulama — 2026-08-31: YAPILDI

Kararın istediği ikinci yarı tamamlandı. Docker'da iki gerçek sunucu kuruldu
ve `headshell` ikisine de bağlandı:

| Sunucu | Sürüm | Sonuç |
|---|---|---|
| Navidrome (OpenSubsonic) | 0.63.2 | kayıt → doğrulama → arama → **akış** → scrobble |
| Jellyfin | 10.11.11 | kayıt (parola→anahtar) → doğrulama → arama → **akış** → scrobble |

Her iki sunucudan da fixture FLAC'ı gerçekten çalındı ve `headshell stats`
çıktısında göründü — Faz 1'in bitti ölçütünün ("yerel **ve uzak** kaynaktan
çalıyor") uzak yarısı artık varsayım değil.

Ayrıca sınanan yollar: yanlış parola (ikisinde de kayıt **yazılmadı**),
Jellyfin `--api-key` (ağa çıkmadan kimlik), `provider list`'te uzak
sağlayıcının görünmesi.

**Sahte sunucunun gizlediği ve gerçeğin gösterdiği tek kusur:** kayıt
doğrulaması başarısız olduğunda dıştaki cümle "sunucusuna **erişilemedi**"
diyordu. Navidrome yanlış parolayı `HTTP 200 + status:"failed"` ile
reddedince mesaj şuna dönüşüyordu: *"erişilemedi: … isteği reddetti: Wrong
username or password"* — yani kendi içinde çelişiyor ve kullanıcıyı ağ
hatası aramaya gönderiyordu. Sağlıksızlığın iki sebebi (ulaşılamadı /
reddedildi) tek cümlede birleştirilemez; dıştaki metin artık yalnızca
"**doğrulanamadı**" diyor, sebebi `detail` taşıyor (K9). Regresyon testle
kilitli (`a_subsonic_failure_arrives_with_http_200_and_still_fails`).

**Doğrulama yordamı** (tekrarlanabilir olsun diye):

```bash
docker run -d --name nav -p 14533:4533 \
  -v "$PWD/fixtures/audio:/music:ro" -v nav-data:/data \
  -e ND_DEVAUTOCREATEADMINPASSWORD=parola123 deluan/navidrome:latest

HEADSHELL_PASSWORD=parola123 headshell provider add subsonic \
  --url http://127.0.0.1:14533 --user admin --name nav
headshell provider test nav && headshell play "Test" --all && headshell stats
```

Jellyfin'de kurulum sihirbazı API'den geçiliyor (`/Startup/Configuration`,
`/Startup/User`, `/Startup/RemoteAccess`, `/Startup/Complete`), sonra
`/Library/VirtualFolders` ile `/music` kütüphane olarak ekleniyor.

**Hâlâ sınanmayan:** HTTPS/TLS (ikisi de düz HTTP üzerinden koşuldu),
ters vekil arkasındaki yönlendirme, sunucu tarafı transcode, büyük kütüphane
(4-5 parça ile sınandı) ve Subsonic'in Navidrome dışındaki uygulamaları
(Airsonic, Gonic, LMS).

---

## D-023 — Reddedilen istek hangi aşamaya ait
**Tarih:** 2026-08-31
**Soru:** Sunucu `HTTP 401` döndürdüğünde hata hangi `Stage` ile raporlanmalı?
Eskiden her 2xx-dışı kod `NETWORK_REQUEST` idi.
**Karar:** **`401` ve `403` → `PROVIDER_CALL`**; geri kalan her kod
(`404`, `429`, `5xx`…) `NETWORK_REQUEST` olarak kalır.
**Gerekçe:** K9'un ayrımı: "ulaşamadım" ile "hayır dedi" farklı tanılar ve
farklı çözümleri var. `401` bir taşıma hatası değildir — bağlantı kuruldu,
istek gitti, sunucu okudu ve reddetti. `NETWORK_REQUEST` demek kullanıcıyı
ağını kurcalamaya gönderir; oysa yapması gereken şey kimliğini düzeltmek.
Kusur gerçek Jellyfin'e karşı görüldü (D-022 doğrulaması): yanlış parola
`ADIM: NETWORK_REQUEST` diye raporlanıyordu.

`404` ve `5xx` bilerek taşıma katmanında bırakıldı: onlarda hatanın hangi
katmandan geldiği gövde okunmadan bilinemez, tahmin etmek yanlış tanı üretir.

**Sonuç:**
- Kural tek yerde: `net::stage_for_status`. Hem `HttpResponse::error_for_status`
  hem `UreqClient::open_stream` (akış açarken alınan 401) onu çağırıyor.
- Geçen bir birim testi **kasten** değişti (`jellyfin::an_http_error_is_
  reported_with_its_status_and_body`); §0.1 gereği önce soruldu.
- Aynı kusur sınıfı iki yerde daha düzeltildi:
  - `remote::prepare_server` doğrulama hatası artık "erişilemedi" değil
    "**doğrulanamadı**" diyor (D-022 doğrulama bölümü).
  - `headshell provider test` başlığı "ERİŞİLEMİYOR" değil "**KULLANILAMIYOR**":
    `ProviderHealth.reachable == false`'ın iki sebebi var, başlık ikisini de
    kapsayan kelimeyi seçiyor, sebebi `not` satırı söylüyor. `ProviderHealth`
    API'si değişmedi — bu bir sunum kararı, CLI'nin işi (Altın Kural).

**Yan bulgu — panik riski kapatıldı (K8).** `error_for_status` gövdeyi
`String::truncate(200)` ile kırpıyordu; `truncate` karakter sınırının
ortasına düşerse **panikler**. Sunucunun Türkçe (ya da herhangi bir çok
baytlı) hata mesajı çekirdeği düşürebilirdi. Yerine sınıra hizalayan
`net::clip` kondu, testle kilitli.

---

## D-024 — Gapless: tek çıkış, sırayla beslenen parçalar
**Tarih:** 2026-08-31
**Soru:** Parçalar arasındaki boşluk nasıl kapatılacak? Her parça için yeni
bir `AudioEngine` (yeni cpal akışı + yeni çözücü) kuruluyordu; boşluk buydu.
**Karar:** **Tek çıkış, sıralı besleme.** cpal akışı ve halka tamponu parçalar
arasında **açık kalır**; çözme iş parçacığının ömrü motorun ömrü kadardır ve
bir parça bitince kuyruktaki sıradakini alıp **aynı tampona** yazmayı sürdürür.
**Gerekçe:** Değerlendirilen alternatif "iki motor, önceden hazırla" idi:
mevcut yapı korunurdu ama aynı anda iki cpal akışı açık olmak zorundaydı ve
bazı ALSA/WASAPI yapılandırmalarında ikincisi açılamaz — gapless **sessizce**
çalışmazdı. Hedef kitle Linux ağırlıklı; sessizce bozulan bir özellik,
olmayan bir özellikten kötüdür.
**Sonuç:**
- **Pozisyon muhasebesi dilimlere dayanıyor.** Tampon artık birden çok parçanın
  örneklerini yan yana taşıdığı için pozisyon tek sayaçtan okunamaz. Her parça
  bir `Span`: çıkış karesi cinsinden nerede başladığı, kaç kare yazdığı, süresi.
  Çalan parça, `frames_played`'in düştüğü dilimdir.
- `start_frame` sıraya girerken **bilinmiyor** (`None`): nerede başlayacağı
  kendinden öncekinin kaç kare yazdığına bağlı ve o parça bitmeden belli olmaz.
  Yalnızca **başlamış** dilimler "çalıyor" sayılır — yoksa geçiş erken duyurulur
  ve arayüz henüz çalınmamış parçayı gösterir.
- **Kullanıcı isteğiyle geçiş gapless değil.** `next`/`jump_to` motoru yeniden
  kuruyor: önden okunmuş sesi çalmak, kullanıcının seçmediği parçayı duyurmak
  olurdu. Gapless yalnızca doğal bitiş içindir.
- **Önden okuma bir iyileştirme, bağımlılık değil.** Sıradaki parça açılamazsa
  (sağlayıcı hatası) `tick` eski yola düşüyor: motoru yeniden kur. Boşluk olur,
  çalma durmaz. Hata log'a düşer (K9).
- `Queue::peek_after_finish` imleci **oynatmadan** sıradakini söylüyor. Kuyruk
  sonunda `RepeatMode::All` ile sarmada `None` dönüyor: sarma karıştırmayı
  yeniden üretiyor ve hangi parçanın geleceği imleç oynamadan bilinemez.
  Bedeli tur başına bir boşluk; uydurulmuş bir parçayı önden çözmekten iyidir.
- Ölçüm: 4 fixture parçası (toplam 5 sn ses) uçtan uca **5.47 sn**'de çalındı —
  aradaki üç geçişin toplam maliyeti ölçülemeyecek kadar küçük.

**Yan bulgu — scrobble tutarsızlığı düzeltildi.** Etiketsiz dosyaların süresi
katalogda `None` olduğu için `PlayRule`'un "parçanın yarısı" kolu çalışamıyor,
kural 30 sn eşiğine düşüyordu: baştan sona dinlenmiş 1 sn'lik bir parça
scrobble üretmiyordu. Süre artık **kaptan** okunuyor (`AudioEngine::duration_of`)
ve hem kurala hem **kayda** giriyor. Kayda da girmesi şart: yoksa CLI "4 dinleme
kaydedildi" derken `stats` kuralı yeniden uygulayıp 2 gösteriyordu. D-008'in
kuralı değişmedi — ona verilen veri düzeldi. Testle kilitli
(`every_queued_track_produces_a_listen_that_stats_also_counts`).

**Yan düzeltme.** `play_file`/`play_http` kaynağı **aygıttan önce** açıyor:
bozuk ya da olmayan dosya, ses çıkışı bulunmayan bir ortamda (CI) da
`PLAYBACK_DECODE` demeli; "aygıt yok" hatası asıl sebebi gizlerdi.

---

## D-025 — Dizin izleme: bağımlılıksız bayatlık yoklaması
**Tarih:** 2026-08-31
**Soru:** Değişiklikler yalnızca elle `headshell provider scan` ile alınıyor.
Dizin izleme (watch) için `notify` crate'i eklensin mi?
**Karar:** **Hayır — bağımlılık eklenmedi.** Yerine `headshell provider scan
--if-stale`: sağlayıcıya ucuz bir soru sorulup yalnızca gerekiyorsa taranıyor.
**Gerekçe:** Tarama zaten artımlı (D-018, mtime damgası) ve pahalı kısmı olan
etiket okuma değişmemiş dosyalarda atlanıyordu; eksik olan "taramaya değer mi"
sorusuydu. `notify` çekirdek ağacını büyütür (K7, mobil binary boyutu) ve
platform başına farklı davranır. Dizin damgaları her yerde aynı biçimde
çalışıyor.
**Sonuç:**
- Soru trait'te: `Provider::catalog_changed_since(since_ms)` → `Option<bool>`.
  Üç cevap üçü de farklı şey (K9): `Some(true)` değişmiş, `Some(false)`
  değişmemiş, **`None` bilmiyorum**. Varsayılan `None` — uzak sağlayıcı ucuz
  bir damga sunmuyor ve "değişmedi" demek yanlış olurdu. Yine downcast yerine
  varsayılan trait metodu (`scan_catalog` ile aynı gerekçe): eklentiler
  (Faz 2) kendi damgalarını verebilsin.
- **"Bilmiyorum" tarama sebebidir.** Bilmediğimiz için atlamak, kullanıcının
  eklediği dosyayı görünmez yapardı. Hiç taranmamış katalog da öyle.
- Yerel sağlayıcı yalnızca **dizinleri** geziyor, dosyaları `stat` etmiyor:
  soru "ne değişti" değil "taramaya değer mi". Okunamayan bir dizin varsa
  cevap `None` — orada bir değişiklik olabilir.
- **Görmediği şey açıkça yazılı:** dosyanın yerinde yeniden etiketlenmesi.
  Dosya değişir, dizin damgası değişmez. Bunu yakalamak her dosyayı `stat`
  etmek, yani zaten artımlı taramanın kendisi olurdu. O durumda düz
  `headshell provider scan` gerekiyor ve komut yardımı bunu söylüyor.
- Şema **değişmedi**: "en son ne zaman tarandı" sorusu `provider_tracks
  .scanned_at`'in `MAX`'ından geliyor. Hiç taranmamışsa `None` — sıfır değil;
  "1970'te baktım" her şeyi bayat gösterirdi.
- `ScanReport` iki alan kazandı: `scanned` (koştu mu) ve `reason` (neden).
  Atlama **sessiz değil**: CLI "tarama atlandı (local: değişmemiş)" basıyor.

Bu bir izleme (watch) değil, tetiklenince bakan bir yoklama. Gerçek zamanlı
izleme gerekirse Faz 3'te GUI'nin olay döngüsüyle birlikte yeniden bakılır —
orada zaten bir döngü olacak.

---

## D-026 — DRM aşmak değişmez kural düzeyinde yasak
**Tarih:** 2026-08-31
**Soru:** Yayın platformları (Qobuz, Tidal, Deezer, Apple Music, YouTube Music,
SoundCloud, Spotify, Amazon, Pandora, Idagio, Tencent) fazlarda adlandırılmamıştı.
Hangileri desteklenebilir?
**Karar:** Ayırıcı ölçüt "resmî API var mı" **değil**, **"ses DRM ile korunuyor mu"**.
DRM'li bir akışı çözen kod bu projede yazılmaz — ve bu bir tercih değil, **ASLA
YAPMA maddesi** (hem PLAN.md hem CLAUDE.md).
**Gerekçe:** Koruma önlemi aşmak telif ihlalinden **ayrı** bir kanun maddesidir
(DMCA §1201, EU 2001/29 m.6): eserin kendisine hakkın olsa bile ihlal sayılır.
D-002 bunun yayınlanacak bir ürün olduğunu söylüyor, dolayısıyla kişisel kullanım
muafiyeti yok — K4'ün Spotify için tek platformda yaptığı şeyin geneli.
Kural olarak yazılmasının sebebi ayrı: gerekçe olarak kalsaydı her yeni platformda
("Deezer'ı da ekleyelim") tartışma yeniden açılırdı.
**Sonuç:**
- Yeni bölüm **PLAN §2.5** ve genişletilmiş **EK — Yayın platformları**
  (eski "EK — Spotify"nin yerine; Spotify içeriği `CONTROL` satırı olarak korundu).
- Sınıflandırma: **akıtılabilir** SoundCloud / Qobuz / YouTube Music;
  **yalnızca metadata** Tidal / Apple Music / Deezer; **yalnızca `CONTROL`**
  Spotify; **kapalı** Amazon Music / Pandora / Idagio / Tencent.
- **Deezer metadata tarafında resmî ve açık, akış tarafında şifreli** (Blowfish).
  Bu onu Tidal'la aynı kutuya değil, çizginin öbür tarafına koyuyor.
- **Apple Music'i eleyen hukuk değil teknik:** MusicKit resmî ve meşru, ama
  çalma MusicKit çalışma zamanı istiyor — Linux'ta yok, WebKitGTK'da FairPlay yok.
  Faz 6'nın iOS/macOS sürümünde `STREAM` açılabilir; satır o yüzden silinmedi.
- **Pandora ve Idagio'yu model uyumsuzluğu da eliyor:** Pandora istasyon adresliyor,
  biz parça. Idagio eser/bölüm/icra adresliyor — o `identity/` işi, sağlayıcı işi değil.
- **Tencent listede kaldı ("kapalı" olarak).** K5'in gerekçesi tam bu: o bölgedeki
  biri eklentiyi kendi yazar. Biz protokolü veriyoruz, listeyi değil.
- **Hiçbiri çekirdeğe girmez.** Sebep yalnızca K5 değil: bu API'ler haber vermeden
  bozulur ve bozulduğunda çalan müzik durmamalı, yalnızca o eklenti düşmeli.
- EK'in başına "**bu API bilgileri doğrulanmadı**" uyarısı kondu — eklenti
  yazılmadan önce ilgili satır yeniden sınanacak.

Asıl bulgu tabloda değil altında: **çalabildiğimiz platform üç, dinleme kimliğini
alabildiğimiz platform on.** İçe aktarma bu çizgiyi tanımıyor çünkü export bir
yasal haktır, hizmet şartı onu kısıtlayamaz (K2). "Kapalı" satırlar bile geçmişi
veriyor. Ürün de zaten ikincisiydi.

---

## D-027 — Faz 1'den sonra sıra: Faz 3 (GUI + tema)
**Tarih:** 2026-08-31
**Soru:** D-014 yalnızca "önce Faz 1" demişti, sonrasını bağlamamıştı.
Sıra Faz 2 (eklenti sınırı) mı, Faz 3 (GUI + tema) mi?
**Karar:** **Faz 3.** İlk iş §3.1 GO/NO-GO ölçümü.
**Gerekçe:** PLAN'ın kendi uyarısı: "topluluk motoru burasıdır (D-002, D-004),
geciktirilmesi pahalıdır." Tema ekosistemi bu projenin dağıtım kanalı; sonradan
eklenecek bir süs değil.
**Sonuç:**
- §3.1 önce ölçüm, sonra karar: Tauri'de sanallaştırılmış liste + CSS animasyon +
  IPC yükü, **Linux/WebKitGTK'da**. Ölçüm `spike/` içinde yapılır — workspace
  dışı, atılabilir, çekirdeğe bağımlılık girmez.
- Ölçüm kabul edilemez çıkarsa Dioxus/yerel Rust GUI tartışılır; bedeli CSS tema
  ekosistemini kaybetmek. Karar ölçümden **sonra**, sayıyla verilir.
- Faz 2 ertelendi, iptal edilmedi. Ona devredilen borçlar duruyor: keyring (D-021),
  gerçek zamanlı izleme (D-025), TLS/ters vekil/transcode doğrulaması (D-022).
- **Faz 2'nin referans eklentisi (§2.2) şimdiden SoundCloud olarak seçildi.**
  Tek gerekçesi katalog değil: abonelik gerektirmeyen tek aday, yani CI'da ve
  başkasının makinesinde çalışabilen tek aday. Qobuz'un katalogu daha temiz ama
  abonelik olmadan ne geliştirilebilir ne test edilebilir; YouTube Music'in bakım
  maliyeti (nsig, PO token) protokolü sınamaya çalışırken platformla boğuşmak
  demek olurdu. Referans eklentinin işi protokolü kanıtlamak, katalog sunmak değil.

---

## D-028 — §3.1 GO/NO-GO: Tauri kabul edildi, ortam şartıyla
**Tarih:** 2026-08-31
**Soru:** PLAN §3.1 — Tauri, Linux/WebKitGTK'da 50.000 satırlık sanallaştırılmış
liste + CSS animasyon + IPC yükü altında kabul edilebilir mi?
**Karar:** **GO.** Ama Linux'ta `GDK_BACKEND=x11` ve
`WEBKIT_DISABLE_DMABUF_RENDERER=1` ayarlanmadan **kabul edilemez**.

**Ölçüm makinesi bilerek zayıf:** Intel HD 6000 (Broadwell GT3, 2015), 4 çekirdek,
8 GB, Wayland, WebKitGTK 2.52.6, Tauri 2 (431 crate, 11 MB ikili).
Eşikler `spike/tauri-gonogo/ESIKLER.md`'de **ölçümden önce** yazıldı; sayıları
görüp eşik koymak ölçüm değil, kararı ölçüme uydurmak olurdu.

**Sayılar (düzeltilmiş ortam, iki bağımsız koşum):**

| Ölçü | GO eşiği | Sonuç |
|---|---|---|
| 50k kaydırma + disiplinli CSS, medyan kare | ≤ 18 ms | **17 / 17 ms** |
| ” p95 | ≤ 25 ms | **21 / 22 ms** |
| ” en kötü kare | ≤ 120 ms | **23 / 26 ms** |
| IPC gidiş-dönüş p95 (1000 örnek) | ≤ 5 ms | **1 / 1 ms** |
| 30 Hz IPC'nin kare maliyeti | ≤ 2 ms | **1 / 1 ms** |
| Rust → JS olay akışı | ≥ 2000/sn | **10.417 / 11.765** |
| Pencere görünene kadar | ≤ 1500 ms | **271 / 267 ms** |
| RSS tepe (50k satır yüklü) | ≤ 250 MB | **185 / 183 MB** |
| 50k kaydırma + **saf** CSS, medyan | ≤ 18 ms | 21 / 21 ms — SINIRDA |

**Gerekçe ve dört bulgu:**

1. **Sanallaştırılmış liste sorun değil.** 50.000 satır, düğüm havuzlu
   sanallaştırma, sürekli kaydırma: 58.8 fps, sıfır takılan kare. Ölçümün en
   kolay geçen kısmı buydu — korkulan yer burası değilmiş.

2. **Varsayılan ortamda CSS animasyonu kare hızını 2.4× düşürüyor** (58.8 → 23.8).
   Bu düzeltilmeseydi NO-GO olurdu.

3. **Suçlu donanım değil, motorun yolu — kontrol deneyiyle ayrıldı.** Aynı
   makinede, aynı sayfada, aynı GPU'da **Firefox 154 dört fazın dördünde de
   58.8 fps** çiziyor (saf CSS dahil). Bu ayrım kararı belirledi: donanım tavanı
   olsaydı PLAN'ın alternatifi (yerel Rust GUI) de kurtarmazdı, çünkü aynı GPU'ya
   çarpardı. Kontrol koşulmasaydı yanlış karar verilirdi.
   Kontrol ayrı bir sayfa değil, **aynı `index.html`** — farklı bir sayfa ölçen
   kontrol karşılaştırılabilir olmazdı.

4. **Tek değişkenli düzeltme yanıltıcıydı.** `WEBKIT_DISABLE_DMABUF_RENDERER=1`
   **tek başına kaydırmayı kötüleştiriyor** (52.6 → 30.3 fps);
   `GDK_BACKEND=x11` tek başına CSS fazını kurtarmıyor (23.8 → 26.3 fps).
   Yalnızca ikisi birden işe yarıyor. Değişkenleri tek tek deneyip bırakan bir
   araştırma "geçici çözüm yok" deyip NO-GO verirdi.

**Sonuç:**
- Faz 3 sürüyor; §3.2 (IPC sözleşmesi) sıradaki iş.
- **Ortam düzeltmesi bir dağıtım işidir, kullanıcı işi değil.** Nasıl
  uygulanacağı (uygulamanın kendisi mi ayarlasın, sarmalayıcı betik mi, sürücüye
  göre koşullu mu) ayrı bir karar — açık.
- **§3.2'nin korkusu bu ölçekte doğrulanmadı.** "Saniyede yüzlerce mesaj =
  takılma" gerçekleşmedi: 30 Hz yoklama kareye 1 ms ekliyor, köprü ~10.000
  olay/sn taşıyor. Toplu gönderim **performans için gerekli değil**.
  Çapadan tahmin yine de doğru tasarım, ama gerekçesi değişti: (a) IPC
  duraksarsa arayüz donmaz, (b) Faz 4'ün oda primitifi zaten aynı tip (D-015).
  §3.2 bu düzeltilmiş gerekçeyle yazılacak — yanlış gerekçeyle savunulan doğru
  tasarım ilk itirazda düşer.
- **§3.3'e giren kısıt:** düzeltilmiş ortamda bile `height` / `box-shadow` /
  `filter` / `background-position` animasyonları 58.8 → 47.6 fps götürüyor;
  `transform` + `opacity` hiç düşürmüyor. Tema sözleşmesi hangi özelliklerin
  canlandırılabileceğini **söylemek zorunda** — söylemezse fark kullanıcının
  makinesinde ortaya çıkar.

**Ölçülmeyen (bilerek yazılıyor):** tek makine ve tek sürücü (Mesa/Broadwell);
Nvidia, AMD, yeni Intel denenmedi. `GDK_BACKEND=x11` XWayland gerektirir,
XWayland'sız kurulum denenmedi. Yeniden boyutlandırma, çoklu pencere, yüksek DPI,
4K ölçülmedi (piksel sayısı ~9× artar). WebKit `performance.now()`'u 1 ms'e
yuvarladığı için ms altı ayrım yok.

Ayrıntı ve tekrar üretme yordamı: `spike/tauri-gonogo/SONUC.md`.
Spike atılabilir; kalması gereken sayılar burada.

---

## D-029 — Ortam düzeltmesini uygulamanın kendisi kurar
**Tarih:** 2026-08-31
**Soru:** D-028 Linux'ta `GDK_BACKEND=x11` + `WEBKIT_DISABLE_DMABUF_RENDERER=1`
gerektiğini ölçtü. Bunu kim kuracak — uygulama mı, sarmalayıcı betik mi,
kullanıcı mı?
**Karar:** **Uygulamanın kendisi**, `main()`'in ilk işi olarak, yalnızca Linux'ta
ve yalnızca **değişken tanımlı değilse**.
**Gerekçe:** Kullanıcının bilmek zorunda olmadığı bir şey. Sarmalayıcı betik
çözümü terminalden doğrudan ikiliyi çalıştıranı dışarıda bırakırdı; koşullu
ölçüm (açılışta kare hesabı yapıp karar verme) açılışa gecikme ve webview
yeniden başlatma titremesi eklerdi.

**Doğrulandı, varsayılmadı.** Sorulacak soru vardı: GDK `GDK_BACKEND`'i
`gtk_init` sırasında, WebKit `WEBKIT_DISABLE_DMABUF_RENDERER`'ı web süreci
doğarken okur — ikisi de Tauri kurulumundan **sonra**. `main()`'in ilk satırı
yeterince erken mi? Ölçüldü: hiçbir dış değişken verilmeden, uygulama kendi
kurarak **23.8 → 55.6 fps** (CSS fazı). Erkenmiş.

**Sonuç:**
- Kullanıcının kendi ayarı **ezilmiyor**: bilerek `GDK_BACKEND=wayland` veren
  birine karışılmıyor. Kurulan ya da atlanan her değişken log'a yazılıyor —
  sessiz ortam değiştirme hata ayıklamayı imkânsız kılar (K9).
- **Açık pürüz — `unsafe` çakışması.** Rust 2024'te `std::env::set_var` `unsafe`.
  Workspace `[workspace.lints.rust] unsafe_code = "forbid"` diyor ve `forbid`
  paket düzeyinde `allow` ile **geçersiz kılınamaz**; `headshell-core` ve `headshell-cli`
  ikisi de `[lints] workspace = true` ile devralıyor. GUI paketi yazılırken üç
  seçenek var: (a) workspace kuralını `deny`ye çevirmek (o zaman geçersiz
  kılınabilir ama koruma zayıflar), (b) GUI paketini `lints.workspace = true`
  demeden bırakmak (diğer kuralları da kaybeder), (c) `unsafe` hiç kullanmayıp
  ortamı kurup **kendini yeniden çalıştırmak** (`exec`). Bu karar §3.2'de,
  GUI paketi gerçekten açılırken verilecek — bugün paket yok.
- **Bu düzeltme tek makinede ölçüldü** (Mesa / Broadwell / Wayland). Başka
  sürücülerde gerekli mi, zararsız mı bilinmiyor. `GDK_BACKEND=x11` XWayland
  gerektirir; XWayland'sız saf Wayland kurulumunda ne olacağı denenmedi.
  Bu yüzden koşulsuz değil, "tanımlı değilse" kuruluyor: kaçış yolu açık kalıyor.

---

## D-030 — Paket düzeni: üç paket, tek yön
**Tarih:** 2026-08-31
**Soru:** GUI paketi nerede yaşasın — workspace içinde mi, ayrı workspace mi,
ayrı depo mu?
**Karar:** Aynı depo, aynı workspace, **üç paket**: `headshell-core`, `headshell-cli`,
`headshell` (GUI). Ayrı paket **gibi davranılır** ama ayrı depo olmak zorunda değil.
**Gerekçe:** Ayrılabilirlik bir yapı özelliği, dosya düzeni değil. Önemli olan
şu: `headshell-cli` ve `headshell`, `headshell-core` olmadan **çalışamaz** — bütün baz işlemler
orada (K1). Ayırmak istendiğinde ayrılabilmesi için bugünden bağımlılık yönünün
tek yönlü olması yeterli.
**Sonuç:**
- Bağımlılık yönü: `headshell-core` ← `headshell-cli`, `headshell-core` ← `headshell`.
  **`headshell-cli` ile `headshell` birbirini hiç görmez.** Biri diğerinden bir şey
  isterse o şey çekirdeğe aittir — Altın Kural'ın paket düzeyindeki hâli.
- Ayrı depo şimdilik hayır: çekirdek hâlâ hızla değişiyor, iki depoyu adımda
  tutmak bu aşamada pahalı.
- Bilinen bedel: `cargo test --workspace` Tauri ağacını da derleyecek.
  Ölçüldü — Tauri'nin tam derlemesi ~70 sn, kilit dosyasında 431 crate.
  Katlanılır; katlanılmaz olursa `default-members` ile ayrılır.

---

## D-031 — Ortam düzeltmesi `unsafe` olmadan: `exec`
**Tarih:** 2026-08-31
**Soru:** D-029'un açık pürüzü. `std::env::set_var` Rust 2024'te `unsafe`,
workspace `unsafe_code = "forbid"` diyor ve `forbid` paket düzeyinde `allow` ile
geçersiz kılınamaz. Kural mı gevşesin, paket mi lint devralımından çıksın?
**Karar:** **Hiçbiri.** Ortam kurulup süreç `exec` ile kendini yeniden başlatıyor.
`CommandExt::exec` güvenli bir çağrı; `set_var`'a hiç gerek kalmıyor.
**Gerekçe:** Tek satır için workspace çapında bir güvenlik özelliğini gevşetmek
orantısız. Paketi lint devralımından çıkarmak da diğer bütün kuralları
kaybettirirdi. `forbid` üç pakette de bozulmadan kalıyor.
**Sonuç:**
- **Döngü koruması yapıdan geliyor, bayraktan değil:** yalnızca **eksik**
  değişkenler kuruluyor; çocuğun gözünde eksik yok, o yüzden ikinci kez
  `exec` etmiyor. Ayrı bir "zaten yeniden başlatıldı" bayrağı gerekmiyor.
- `exec` süreç imajını değiştirir, PID korunur — masaüstü/servis
  bütünleşmesi bozulmaz.
- Doğrulandı: hiçbir dış değişken verilmeden CSS fazı **58.8 fps**, açılış
  352 ms. Yeniden başlatmanın ölçülebilir bedeli yok.
- `exec` yalnızca **başarısızsa** döner; o durumda düzeltmesiz devam edilip
  sebep log'a yazılıyor — sessizce yavaş çalışmak yerine (K9).

---

## D-032 — `tick` döngüsü ve dinleme kaydı çekirdeğe taşındı
**Tarih:** 2026-08-31
**Soru:** Düzenli `tick()` ve biriken dinlemelerin depoya yazılması hangi
katmanda yaşasın — her kabuk kendi mi bağlasın, çekirdek mi versin?
**Karar:** **Çekirdek.** Yeni tip `playback::LiveSession`, `Player` ile
`Session`'ı bağlar; tek `tick()` çağrısı ilerletir, yazar ve bir `TickReport`
döndürür. TUI buna geçirildi.
**Gerekçe:** K1'in testi: TUI aynı dansı bir kez yazdı (tick → take_listens →
record_listens), GUI ikinci, mobil üçüncü kez yazacaktı. Dinlemeyi depoya
yazmayı unutan bir kabuk **sessizce geçmiş kaybeder** — kaybı fark ettiren
hiçbir şey yok.
**Sonuç:**
- **Davranış değişti: dinlemeler artık her turda yazılıyor, çıkışta değil.**
  Eski hâlde `tui.rs` yalnızca döngü bittiğinde yazıyordu; CLI oturumu kısa
  olduğu için sorun görünmüyordu, ama saatlerce açık kalacak bir arayüzde
  çökme ya da `kill` bütün oturumun geçmişini götürürdü. Çoğu turda yazılacak
  bir şey olmaz ve depoya hiç gidilmez.
- **Yazma başarısız olursa kayıtlar atılmıyor, elde tutuluyor** ve sonraki
  turda yeniden deneniyor. `take_listens` kayıtları oynatıcıdan çekip aldığı
  için, tutulmasalar geri alınacakları bir yer yok.
- **Depo hatası `tick`'i `Err` yapmıyor:** ses çalmaya devam ediyor, oturumu
  düşürmek veri kaybını artırırdı. Ama sessiz de kalmıyor — `TickReport`
  `store_error` ve `listens_pending` taşıyor, TUI ikisini de gösteriyor (K9).
- **D-015 korundu:** observer/callback yok, kabuk döngüyü kendi sürüyor.
  **K7 korundu:** kapanış parametresi, generic, ömür sızıntısı yok.
- `track_changed`'in yakalamadığı durum bilerek yazıldı: `RepeatMode::One`
  aynı parçayı baştan başlattığında ne parça ne konum değişir. Kabuk için
  doğru olan da bu — baştan başladığını çapa, yeni dinlemeyi
  `listens_recorded` söylüyor.
- **Bir test kaldırıldı çünkü hiçbir şey kanıtlamıyordu.** "Depo yazamazsa
  kayıt kaybolmaz" iddiasını gerçek bir bozuk depoyla sınamak denendi:
  veri dizini salt-okunur yapıldı, ama SQLite açık dosya tanıtıcısıyla
  yazmayı sürdürdü ve test **yeşil yanıp hiçbir şeyi sınamadı**. Karar saf
  bir fonksiyona (`absorb`) çıkarıldı ve doğrudan sınandı. Koşulu
  sağlanamayan yeşil test, testsizlikten kötüdür — çünkü kapsandığını
  düşündürür.

---

## D-033 — IPC sözleşmesi: çekirdeğin yüzeyi + serde, sürümleme yok
**Tarih:** 2026-08-31
**Soru:** §3.2 — webview ile çekirdek arasındaki sözleşme nasıl tanımlansın,
nasıl sürümlensin, ve webview'deki çapa tahmininin çekirdekten kaymaması nasıl
sağlansın?
**Karar:** Ayrı bir IPC tipi katmanı **yok**. Her komut var olan bir çekirdek
tipini döndürüyor, `serde` ile geçiyor. **Sürüm anlaşması yok.** Kayma
`fixtures/anchor/position_cases.json` ile kilitleniyor.
**Gerekçe:**
- **Çevirmen katmanı iki tipi zamanla kaydırır** ve kaymayı hiçbir şey
  yakalamaz. `--json` çıktısı zaten CLI ile GUI'nin aynı veriyi aldığını
  kanıtlıyordu; ikinci bir şekil uydurmak o kanıtı bozardı.
- **Sürümleme gereksiz çünkü iki taraf da aynı ikilinin içinde.** Webview
  varlıkları uygulamayla paketleniyor, bağımsız güncellenemiyor. Çalışma
  zamanında el sıkışma, yalnızca kendisiyle konuşabilen bir sürece el sıkışma
  öğretmek olurdu. **§3.3'ün tema API'siyle karıştırılmamalı** — o dış bir
  sözleşme ve sürümlenmek zorunda.
**Sonuç:**
- Komut listesi CLI'nin alt komutlarıyla birebir: `search`, `stats`, `sleeve`,
  `import`, `resolve`, `providers`, `provider_test`, `provider_scan`,
  `servers_list`, `server_add`, `server_remove`, `play`, `toggle_pause`,
  `stop`, `next`, `previous`, `jump_to`, `set_shuffle`, `set_repeat`,
  `anchor`, `queue`, `diag`.
- **Olaylar yalnızca bir şey değiştiğinde gönderilir, zamanlayıcıyla değil.**
  GUI'nin Rust tarafı `LiveSession::tick()` döngüsünü sürer; `TickReport`
  `track_changed` / `listens_recorded` / `store_error` / `finished` taşıyorsa
  webview'e geçer, taşımıyorsa hiçbir şey gönderilmez. Aradaki sessizlikte
  pozisyon çapadan tahmin edilir. (D-028 saniyede 10.000 olay taşıyabildiğimizi
  ölçmüştü — yani bu bir performans önlemi değil, gereksiz mesaj göndermeme
  tercihi.)
- `TickReport` `Serialize` kazandı.
- **Açık bırakılan soru §3.3'e devredildi:** temalara IPC yüzeyi açılacak mı?
  Açılırsa sözleşme *dış* sözleşmeye dönüşür ve sürümleme borcu o gün doğar.
  Token seti tasarlanırken cevaplanmalı.

**Kayma koruması ve orada bulunan şey.** Pozisyon webview'de çekirdeğe
sorulmadan tahmin ediliyor, yani formülün ikinci bir kopyası JS'te yaşayacak.
İki kopya zamanla kayar ve kayma kimsenin fark etmediği yerde başlar: ilerleme
çubuğu birkaç yüz milisaniye yalan söyler, kimse şikâyet etmez, sonra **Faz 4'te
aynı formül oda senkronunu sürer.** `fixtures/anchor/position_cases.json`
(12 vaka) iki tarafın da okuduğu tek doğruluk kaynağı; Rust tarafını
`tests/anchor_parity.rs` bağlıyor.

Küme yazılırken bir vaka testi kırdı ve **hatalı olan beklenti çıktı, kod
değil**: `rate = 1.001` ile 100 sn'de çekirdek 100100 değil **100099 ms**
diyor — `100000 × 1.001` ikilik tabanda tam değil (100099.999…) ve `as u64`
kırpıyor. JS `Math.round` kullansaydı 100100 derdi ve iki kopya tam buradan
ayrılırdı. Doğru karşılık `Math.floor(gecen_ms * rate)`; fixture'da yazılı.
Kümenin en değerli vakası bu ve daha JS yazılmadan bulundu.

İkinci bir test kümenin kendisini koruyor: zor vakalar (Buffering, rate 0,
saat geri atlaması, süreye kırpma) silinirse test kırılıyor. Doğruluk kümesi
yalnızca kolay yolu kapsıyorsa kilit değildir.

---

## D-034 — GUI'de çekirdek kendi iş parçacığında yaşar
**Tarih:** 2026-08-31
**Soru:** `headshell` paketi yazılırken çıktı: Tauri her async komutun future'ının
`Send` olmasını istiyor. `LiveSession` bir `Mutex` arkasında paylaşılan durum
olarak tutulabilir mi?
**Karar:** **Hayır.** Çekirdek kendi iş parçacığına yerleşiyor; komutlar oraya
bir kapanış gönderip `oneshot` ile cevabı bekliyor. Kilit yok.
**Gerekçe:** Ölçülen kısıt, tercih değil:
- `Session` **`Send` ama `Sync` değil** — SQLite bağlantısı `RefCell` taşıyor.
  `Mutex<Core>` işe yaramıyor, çünkü kilidi bir `.await` üzerinden taşımak
  `Core: Sync` istiyor.
- `Session::import_archive`'ın döndürdüğü future zaten `Send` değil
  (`Box<dyn ExportArchive>`). Kilit sorunu çözülse bile bu kalırdı.
- Çözüm çekirdeği hareket ettirmemek: **hiçbir çekirdek tipi iş parçacığı
  sınırını geçmiyor**, yalnızca iş kapanışları ve seri hâle getirilebilir
  sonuçlar geçiyor. Çekirdek değişmedi — kısıt kabuğun tarafında karşılandı.
**Sonuç:**
- **Komut başına enum varyantı yok.** Kanal `Box<dyn FnOnce(&mut Core) -> ...>`
  taşıyor; her komut kendi `oneshot`'ını kapatıyor. 23 varyantlık bir mesaj
  tipi, D-033'ün reddettiği çevirmen katmanının başka bir kılığı olurdu.
- **Tik döngüsü de aynı iş parçacığında.** `tokio::select!` ile iş kuyruğu ve
  200 ms'lik zamanlayıcı yan yana; komutlarla `tick()` arasında yarış yok,
  sıraya kanal koyuyor.
- **Bilinen bedel:** uzun bir `import` sürerken oynatma kumandaları sırada
  bekler. Görünmez kalmasın diye uzun komutlar `headshell://busy` olayı gönderiyor
  ve arayüz hangi işin sürdüğünü yazıyor (K9). Kabul edilebilir bulundu:
  ses zaten kendi iş parçacığında çalmayı sürdürüyor.
- **İkili adı `headshell-desktop`.** Paket adı D-030'daki gibi `headshell`, ama
  `headshell-cli` zaten `headshell` adında bir ikili üretiyor ve aynı workspace'te iki
  aynı adlı çıktı çakışıyor. CLI'nin adı kullanıcıya vaat edilmiş
  (`headshell import ...`), o yüzden değişen taraf GUI oldu.
- **Hata zarfı var, veri zarfı yok.** `headshell_core::Error` seri hâle
  getirilemiyor (kaynak zinciri `dyn Error`); komutlar `{ stage, chain }`
  döndürüyor — CLI'nin `stderr`'e bastığının aynısı. D-033'ün yasakladığı
  şey veri tiplerinin ikizini yazmaktı; bu onun kapsamında değil.

---

## D-035 — Olay listesi eksikti: durum değişimi de gönderilmeli
**Tarih:** 2026-08-31
**Soru:** D-033 olayların yalnızca `track_changed` / `listens_recorded` /
`store_error` / `finished` durumlarında gideceğini söylemişti. Bu liste
yeterli mi?
**Karar:** **Değil.** Çapanın **tahmine girdi olan** kısmı (durum, hız, süre,
parça kimliği) değiştiğinde de gönderilmeli. Liste bu maddeyle genişledi.
**Gerekçe — kural masa başında değil, çalıştırınca kırıldı.** İlk sürüm
D-033'ün listesini birebir uyguladı ve arayüz **sessizce dondu**: `play`
sonrası webview bir kez çapa alıyor, o an motor daha tamponu doldurmadığı
için durum `Buffering`. Tampon dolunca motor `Playing`'e geçiyor ama bu
geçiş "kayda değer" sayılmadığı için gönderilmiyor. Webview elindeki
`Buffering` çapasıyla kalıyor ve `Buffering` ilerlemediği için ilerleme
çubuğu 0:00'da donuyor. **Hiçbir hata görünmüyor** — ses çalıyor, arayüz
çalmıyormuş gibi duruyor. Ekranda gerçek bir parça çalınana kadar fark
edilmedi.
**Sonuç:**
- Kural şu şekilde ifade edildi: webview `position_ms + (now - wall_time) ×
  rate` ile pozisyonu **kendisi yürütüyor**; o formülün girdisi ya da bağlamı
  değiştiğinde tahmin yanlışa döner. `position_ms`'in kendisi bilerek listede
  **yok** — tahminin işi zaten o.
- Karar saf bir fonksiyona çıkarıldı (`core_thread::worth_sending`) ve
  sınandı. Testlerden biri doğrudan bu hatayı kilitliyor:
  `the_buffering_to_playing_transition_must_be_sent`.
- **Genel ders:** "yalnızca değişince gönder" kuralında asıl risk fazla mesaj
  değil, **eksik mesaj**. Fazlası performansa mal olur ve ölçülür; eksiği
  arayüzü sessizce dondurur ve hiçbir yerde iz bırakmaz.

---

## D-036 — İsimlendirme dili: dış yüzey İngilizce, iç Türkçe
**Tarih:** 2026-08-31
**Soru:** §3.3'ün tema token setini sunarken çıktı: token adları Türkçe mi
olmalı? Ve arkasındaki asıl soru — bu projede hangi isim hangi dilde yazılır?

**Karar:** Ayrım kod dili değil, **kimin okuduğu**.

- **Tanımlayıcıların hepsi İngilizce.** Fonksiyon, tip, değişken, CSS sınıfı,
  HTML id, JSON anahtarı, fixture dosya adı, tema token'ı.
- **Yorum, doküman ve kullanıcıya görünen metin Türkçe kalır.** `PLAN.md`,
  `DECISIONS.md`, doc comment'ler, CLI yardım metni, arayüz yazıları,
  `ADIM:` ön eki. Bunlar yerelleştirme ekseni, isimlendirme ekseni değil.

**Gerekçe:**
- Çizgi zaten fiilen vardı, yazılı değildi: `headshell-core`'un tamamı ve CLI alt
  komutları İngilizceydi (`PlaybackAnchor`, `Queue::view`, `headshell provider
  scan`); Türkçe olan yalnızca GUI kabuğunun içiydi (`.ust`, `dikkate_deger`).
  Yazılmayan kural altı ay sonra tutarsız uygulanır.
- Tema token seti bu projenin **en dışa dönük** yüzeyi olacak — onu tüketen
  benim yazdığım kod değil, tanımadığım bir tema yazarı. `--headshell-yuzey`
  demek tema yazarlığını Türkçe bilenlerle sınırlardı; hiçbir karşılığı
  olmayan bir daraltma.
- CSS'in kendi sözcükleri İngilizce; `background: var(--headshell-arka2)` iki dili
  tek satırda karıştırıyor ve okurken duraklatıyor.
- Aksan sorunu ayrıca var: `sanatçı`/`sanatci` ikiliği bir dosya adında ya da
  bir JSON anahtarında sessiz bir hata kaynağı.

**Sonuç — bu kararla birlikte yapılan yeniden adlandırma:**
- `crates/headshell` (Rust): `duzelt→fixup`, `baslat→spawn`, `calis→run`,
  `tur→run_tick`, `Onemli→Notable`, `dikkate_deger→worth_sending`,
  `calistir→run_on_core`, `cekirdek_dustu→core_thread_gone`.
  Tanı aşaması `ADIM: ORTAM_DUZELTME` → `ADIM: ENV_FIXUP` (çekirdeğin
  `CONFIG_LOAD`/`IDENTITY_RESOLVE` sözlüğüyle aynı yazımda).
- `crates/headshell/ui`: bütün CSS sınıfları, HTML id'leri ve JS adları
  (`.ust→.topbar`, `.uyari→.toast`, `capa→anchor`, `cagir→call`…).
  CSS değişkenleri de İngilizce ama **`--headshell-` ön eki bilerek yok**: o ön ek
  vaadin kendisi ve onu §3.3 dağıtacak.
- Paylaşılan doğruluk kümesi (`fixtures/anchor/position_cases.json`):
  anahtarlar (`vakalar→cases`, `capa→anchor`, `beklenen_ms→expected_ms`) ve
  vaka adları. Bu dosya iki dilden okunuyor ve kümeyi büyütecek kişinin
  Türkçe bilmesi gerekmemeli. Rust tarafındaki `needle` listesi de çevrildi.
- Ses fixture'ları: `etiketli.flac→tagged.flac`, `bozuk.flac→corrupt.flac`,
  `Baska Sanatci - Ogg Parca.ogg→Other Artist - Ogg Track.ogg` vb.
- `examples/calma_denemesi.rs→playback_probe.rs`.

**Tek bilinçli istisna — gömülü etiketler.** `tagged.flac` ve mp3'ün
etiketleri `Test Artist` oldu ama başlıklar `Sine 440 ünïcode` /
`Mp3 Track ünïcode`. Aksanlar bir dil kalıntısı değil, **sınanan şeyin
kendisi**: etiketler UTF-8 ve bu, o çözme yolunun tek kanıtı. Testte de
böyle yazılı.

**Yan ders — yeniden adlandırma bir testi gerçekten kırdı.** `cli_json`'ın
gapless testi `play "Sanat" --all` diyordu ve dört fixture'ın hepsini
yakalaması, ikisinin dosya adının ikisinin de etiketinin Türkçe olmasına
dayanıyordu. Adlar çevrilince ortak belirteç kayboldu ve test 4 yerine 2
parça gördü. Sorgu `"Artist"` oldu; fixture'lar artık `Test Artist`,
`Other Artist`, `Dir Artist` — ortaklık **kasıtlı ve görünür**, dil
kazasından türemiş değil.

---

## D-037 — Tema token seti: dar küme, sınıf adları sözleşme, IPC yok, manifest + `api`
**Tarih:** 2026-09-01
**Soru:** §3.3'ün KARAR NOKTASI'ı dört soru bırakmıştı: granülerlik, seçici
vaadi, temaya IPC açılıp açılmayacağı, paket biçimi. Yazmadan önce sorulacaktı
çünkü yayınlandıktan sonra geriye dönük uyumluluk borcu doğar.

**Kararlar:**

1. **Granülerlik: dar semantik küme.** Bölgeye özel geçersiz kılma yok —
   Spicetify'ın kırılganlığı tam olarak katmanlı/geniş slot setlerinden
   geliyordu (§3.3'ün kendi gerekçesi). ~15-20 token, durum varyantlarını
   (hover/focus/disabled) da içerir ama bölge başına ayrı değişken yoktur.
2. **Seçici vaadi: sınıf adları.** Önerim CSS custom property'lerle
   sınırlamaktı (sınıf adlarını iç detay olarak D-036 sonrası bırakmak);
   kullanıcı sınıf adlarını sözleşmenin parçası yapmayı seçti. **Sonuç:**
   `crates/headshell/ui/style.css`'teki mevcut sınıf adları (`.topbar`, `.toast`,
   `.queue-item` vb.) artık iç ayrıntı değil, tema yazarının hedefleyebileceği
   stabil bir yüzey. Bu, D-036'nın "class isimleri iç ayrıntı, habersiz
   değişir" varsayımını **geçersiz kılıyor** — CSS'i yeniden adlandırmadan
   önce artık bu bir kırılma sayılır. `style.css` başındaki "burası tema
   sözleşmesi değil" uyarısı bu kararla düşüyor.
3. **Temaya IPC: hayır.** Temalar yalnızca görünümü değiştirir; D-032/D-033'ün
   kazandığı "sürümleme gerekmiyor" iç IPC sözleşmesi bozulmadan kalır.
   **Not:** kullanıcı ayrıca davranış değiştiren bir "mod" fikri önerdi —
   tema paketleriyle birlikte indirilebilen, kendine özel bir bölümü olan bir
   şey. Bu **bilinçli olarak ertelendi**, bugün tasarlanmadı: bir mod'un
   webview içinde JS çalıştırıp IPC çağırması, K5'in sağlayıcı eklentileri
   için şart koştuğu "alt süreç + JSON-RPC, eklenti çökerse çekirdek düşmez"
   modelinden köklü biçimde farklı bir güvenlik sınıfı taşır — aynı webview
   içinde çalışan bir mod bütün arayüzü çökertebilir/dondurabilir. PLAN.md'ye
   ayrı bir KARAR NOKTASI olarak yazıldı (bkz. §3.3 sonu), tasarım o gün yapılır.
4. **Paket biçimi: manifest + `api` sürüm alanı.** Küçük bir manifest
   (`name`, `author`, `api`) + CSS dosyası. Uygulama yüklerken `api` sürümünü
   kontrol eder; uyuşmazsa temayı **sessizce görmezden gelmez, açıkça
   reddedip nedenini söyler** — K9 tanılama kültürü ve D-035'in dersiyle
   ("bu kuralda risk fazla mesaj değil eksik mesaj") aynı çizgide.

**Sonuç — yapılacaklar §3.3'ün geri kalanına taşındı:** somut token listesi,
manifest şeması, `style.css`'in token'lara geçirilmesi, tema yükleme/doğrulama
kodu ve §3.4'ün iki referans teması. Bu karar yalnızca dört soruyu kapatıyor;
uygulama ayrı bir adımda yapılıyor.

---

## D-038 — `:root` dışına taşan tema: reddetme, işaretle
**Tarih:** 2026-09-01
**Soru:** Token seti yazılırken (D-037'nin uygulaması) küçük bir beşinci
soru çıktı: `theme.css` `:root` dışına taşıp doğrudan bir seçiciyi (örn.
`.topbar`) hedeflerse yükleyici ne yapsın — reddet mi, serbest mi bıraksın?
**Karar:** **İkisi de değil — işaretle.** Yükleyici böyle bir temayı
reddetmez, yükler; ama tema seçim listesinde açıkça "genişletilmiş / garantisi
yok" diye etiketler. Sözleşmenin **garanti ettiği** yüzey her zaman yalnızca
`:root`'taki `--headshell-*` token'larıdır — bir `api` sürüm atlamasında yalnızca
bunlar için geriye dönük uyumluluk taahhüt edilir.
**Gerekçe:** Kullanıcı katı bir ikili seçim yerine bir orta yol istedi —
"biri yaratıcılığa engel, biri tutarlı arayüze engel." Reddetmek tema
yazarının en ufak esnekliğini (tek bir öğeye özel küçük bir dokunuş) kapatır;
serbest bırakmak sözleşmeyi fiilen anlamsızlaştırır. Etiketlemek K9'un
"sessizce yutma, söyle" ilkesinin tam karşılığı: risk gizlenmiyor, kullanıcıya
görünür kılınıyor, ama önlenmiyor de. D-035'in dersiyle aynı çizgide —
sorunlu olan sessiz zarar, açık bir bilgi değil.
**Sonuç:** Yükleyici yazılırken (§3.3 kalan işi, henüz başlanmadı) manifest
doğrulamasının yanına bir "kapsam dışı kural var mı" taraması eklenecek —
CSS ayrıştırmadan, yalnızca `:root { ... }` bloğu dışında başka bir seçici
var mı diye bakan basit bir kontrol yeter.

---

## D-039 — Token seti eksikti: `--headshell-color-scheme`
**Tarih:** 2026-09-01
**Soru:** Sorulmadı — §3.4'ün referans temaları yazılırken **ölçüldü**.
D-037'nin dar semantik kümesi (on üç token) açık bir temayı ifade etmeye
yetmiyordu: `daylight` teması on üç token'ın hepsini doğru uyguladığı hâlde
onay kutuları, metin imleci ve kaydırma çubuğu koyu kalıyordu.

**Sebep:** Token'lar yalnızca **bizim çizdiğimiz** renkleri değiştiriyor.
Onay kutusunu, imleci ve kaydırma çubuğunu motor çiziyor ve motorun tek
girdisi CSS'in `color-scheme` özelliği — bir renk değeri değil, "bu arayüz
açık mı koyu mu" cevabı. Hiçbir renk token'ı bunun yerine geçemez.

**Karar:** On dördüncü token, `--headshell-color-scheme` (`dark` | `light`).
`style.css`'te `html { color-scheme: var(--headshell-color-scheme); }` olarak
kullanılıyor.

**`api` artmadı ve bu kasıtlı.** Kural: yeni token **eklemek** sürümü
artırmaz — eski temalar onu yazmıyordu, varsayılanını alırlar ve çalışmaya
devam ederler. Artıran şey var olan bir token'ı kaldırmak ya da anlamını
değiştirmek. `--headshell-color-scheme` bu kuralın ilk örneği; kural PLAN §3.3'e
ve `src/theme.rs`'e yazıldı.

**Asıl kayda değer olan bulgu değil, nasıl bulunduğu.** §3.4 "en az iki
farklı temada API'nin yeterli olduğunu kanıtla" diyor; ölçüt kanıtlamak
değil, **yetmediği yeri bulmak** için oradaydı ve tam olarak onu yaptı.
İkinci tema (`contrast`) da bu yüzden ikinci bir palet değil: yarıçap ve
süre token'larını sıfıra indirerek renk **dışındaki** ekseni sınıyor.
İki palet yazılsaydı "iki tema" ölçütü kâğıt üstünde karşılanır, eksik
token ilk tema yazarının makinesinde ortaya çıkardı — ve suçlanan tema
değil uygulama olurdu (D-028'in aynı gerekçesi).

**Yükleyicinin küçük kararları** (hiçbiri geri alınamaz değil, o yüzden
sorulmadı; kayda geçiyor ki ikinci kez tartışılmasın):
- Seçim `<data_dir>/ui.json`'da tutuluyor. Veritabanı şemasına
  dokunulmadı: tema bir dinleme verisi değil, arayüz tercihi.
- Yerleşik referans temalar `include_str!` ile gömülü ama **ayrıcalıksız** —
  diskteki bir tema gibi aynı doğrulamadan geçiyorlar.
- Aynı adı taşıyan disk teması yerleşiği gölgelemiyor, sebebiyle
  reddediliyor. Sessiz gölgeleme, hangi dosyanın kazandığını tahmin
  ettirirdi (K9).
- Tema komutları çekirdek iş parçacığına **girmiyor**: tema bir çekirdek
  kavramı değil, ve uzun bir `import` sürerken arayüzün temasını
  değiştirememek için bir sebep yok.
- Tanı aşaması `CONFIG_LOAD`. Çekirdeğe yalnızca GUI'nin ihtiyacı olan bir
  `THEME_LOAD` aşaması eklemek, kabuğa ait bir kavramı çekirdeğin tanı
  sözlüğüne sızdırmak olurdu.

---

## D-040 — Eklenti izin modeli: beyan + onay, zorlama sonraya
**Tarih:** 2026-09-01
**Soru:** (PLAN §2.1) Eklentinin ağ/dosya erişimi kısıtlanacak mı?

**Karar:** **Beyan + onay.** Eklenti manifestinde izinlerini bildirir,
kullanıcı ilk yüklemede onaylar, onay kaydedilir. **İşletim sistemi
seviyesinde hapsetme yok** — ve bu, kullanıcıya da böyle söylenir.

**Gerekçe.** Üç şey aynı anda doğru:
1. Eklenti bir alt süreç olarak **kullanıcının bütün yetkisiyle** çalışır.
   Beyan bir güvenlik duvarı değil, bir **sözleşmedir**: "bu eklenti şunları
   yapacağını söylüyor". Bunu güvenlik gibi sunmak, olmayan bir korumaya
   güvendirmek olurdu — sessiz `unwrap_or_default()`'ın güvenlik hâli.
2. Gerçek hapsetme (Landlock, `bubblewrap`) yalnızca Linux'ta var. macOS ve
   Windows'ta karşılığı yok; model orada çöker ve "izinli/izinsiz" ayrımı
   platforma göre anlam değiştirir. Faz 2'nin bitti ölçütü **dil bağımsız
   bir referans eklenti**, işletim sistemi hapsi değil.
3. Sonradan eklenebilir olması, protokolün bugün doğru yerinden bölünmesine
   bağlı — o yüzden asıl iş izin **adlarını** doğru koymak.

**Sonuç:**
- Manifestte `permissions: { net: [host…], fs: [yol…] }`. `net` girdileri
  ana bilgisayar adı (`api.soundcloud.com`), `fs` girdileri yol öneki.
  Adlar sonradan bir Landlock/bwrap kuralına çevrilebilecek biçimde,
  yani **açıklama değil, makine okunur** seçildi.
- Onay `<data_dir>/plugins.json`'da izin kümesinin özetiyle birlikte tutulur.
  Eklenti izinlerini büyütürse özet değişir ve **yeniden onay** istenir;
  küçültürse istenmez.
- Çekirdeğin kendi verdiği tek şey daraltılır: eklenti kendi veri alt dizinini
  (`<data_dir>/plugins/<ad>/`) ve **yalnızca kendi** sırlarını görür (D-042).
- `headshell diag` ve `headshell provider list --json` beyan edilen izinleri **ve**
  zorlanmadığını raporlar. Kullanıcı neye güvendiğini bilir (K9).
- Zorlama geldiğinde `api` sürümü artmaz: manifest alanları aynı kalır,
  değişen şey çekirdeğin onlarla ne yaptığıdır.

---

## D-041 — Faz 2 kapsamı: §2.1 + §2.2, gerisi ayrı tur
**Tarih:** 2026-09-01
**Soru:** (PLAN §2.2) Bu fazda kaç eklenti yazılacak?

**Karar:** Yalnızca **§2.1 (protokol) + §2.2 (Python SoundCloud referansı)**.
§2.3 (AcoustID), §2.4 (torrent), §2.5 (yayın platformları) bu turun dışında.

**Gerekçe:** Faz 2'nin "bitti sayılır" ölçütü zaten tam olarak bu — "Rust
olmayan bir referans eklenti çalışıyor ve çekirdek onu sürüm uyumsuzluğunda
çökmeden reddedebiliyor". İkinci bir sağlayıcı protokole yeni bir şey
kanıtlamaz, yalnızca ilk sağlayıcının hatalarını iki kere yazdırır.
Chromaprint (§2.3) yerel bir C kütüphanesi, `librqbit` (§2.4) tek başına
büyük bir iş; ikisi de protokolün doğruluğuna bağlı ve **protokol
oturduktan sonra** yapılırsa daha ucuz.

**Sonuç:** §2.3/§2.4/§2.5 iptal değil, sıraya alındı. Faz 2 protokol
kapandığında yeniden değerlendirilir; ilk eklenti protokolde bir eksik
gösterirse (D-039'un temalarda yaptığı gibi) o eksik kapanmadan sıradakine
geçilmez.

---

## D-042 — Sırlar: tek kavram, dosya tabanlı, ad alanlı; `keyring` yine yok
**Tarih:** 2026-09-01
**Soru:** (D-021'den devir) Kimlik bilgisi depolaması eklenti izin modeliyle
birlikte yeniden ele alınacaktı. `keyring` eklenecek mi?

**Karar:** **Hayır.** D-021'in gerekçesi hâlâ geçerli (yeni bağımlılık,
başsız Linux'ta kırılgan). Onun yerine **tek bir sır kavramı** tanımlanır:
`<data_dir>/secrets.json`, unix'te `0600`, **ad alanlı** —
`{"plugin:soundcloud": {"client_id": "…"}}`.

**Gerekçe:** Eklentilerin de sırra ihtiyacı var (SoundCloud `client_id`) ve
`servers.json` deseni ikinci kez elle tekrarlanacaktı. İkinci kopya, ilk
kopyanın izin sıkılaştırmasını unutan kopya olur.

**Sonuç:**
- `servers.json` **olduğu yerde kalıyor**: içindeki şey bir sunucu kaydı
  (adres + tür + kullanıcı) ve sır o kaydın bir alanı. Göç etmek Faz 1'i
  çalışan hâlinden oynatmak olurdu; kazancı yok.
- Çekirdek el sıkışmada eklentiye **yalnızca kendi ad alanını** geçirir.
  Bir eklenti başka bir eklentinin sırrını istemez, göremez.
- Sır değerleri log'a ve `headshell diag`'a **girmez**; yerine anahtar adı ve
  "var/yok" yazılır. Tanı raporu kopyala-yapıştır edilen bir metin (K9) —
  içinde token taşıyamaz.
- `keyring` kapısı kapanmadı: sır **okuma** tek bir yerden geçtiği için
  arkasına sonradan bir anahtarlık koymak tek dosyalık bir iş.

---

## D-043 — SoundCloud eklentisi: client_id üç kaynaktan, canlı test varsayılan koşumda
**Tarih:** 2026-09-01
**Soru:** §2.2'nin referans eklentisi `client_id`'yi nereden alacak, ve ağa
bağlı bir eklenti "ağa bağlı test yazma" kuralı altında nasıl sınanacak?

**Karar (S1 — client_id):** **Üç kaynak, bu sırayla.** Kullanıcının sırrı
(`plugin:soundcloud` / `client_id`) → diskteki önbellek
(`<data_dir>/plugins/soundcloud/state/client_id.txt`) → SoundCloud'un web
istemcisinden **keşif**. `health()` hangisinin kullanıldığını raporlar.

**Gerekçe:** Öneri "yalnızca kullanıcı verir" idi (kazıma kırılgandır ve
referans eklentinin işi protokolü kanıtlamak, katalog sunmak değil).
Kullanıcı üçünü birden seçti: kurulum sürtünmesi sıfır olsun ama kendi
anahtarını veren kullanıcının anahtarının arkasından dolanılmasın.
Sıra bu yüzden kasıtlı — sır varsa keşfe hiç gidilmez.

**Sonuç:**
- 401/403 geldiğinde: kaynak *keşif/önbellek* ise anahtar bir kez tazelenip
  yeniden denenir; kaynak *sır* ise **tazelenmez**, kullanıcıya kendi
  anahtarının reddedildiği söylenir. Kullanıcının verdiği şeyi sessizce
  değiştirmek, ona yanlış yerde hata aratmak olurdu.
- Keşif dokümante bir uç nokta değil ve haber vermeden bozulabilir.
  Bozulduğunda ne olacağı **yazılı**: açık hata + "kendi client_id'ni ver".
- Keşif el sıkışmada değil **ilk gerçek çağrıda** yapılır: el sıkışmanın
  zaman aşımı 5 sn ve protokol "ağa çıkmayın" diyor.

**Karar (S2 — test yolu):** **Canlı testler varsayılan koşuma dahil.**
`crates/headshell-core/tests/plugin_soundcloud.rs` gerçek SoundCloud'a bağlanır ve
`cargo test --workspace` ile koşar.

**Gerekçe:** Önerim "sahte sunucu + `--ignored` arkasında canlı test" idi;
gerekçe, testin kırmızı yanmasının *bizim* kodumuzun bozulduğu anlamına
gelmesi gerektiğiydi. Kullanıcı bu itirazı gördü ve tersini seçti: eklentinin
bozulduğu gün *o gün* öğrenilsin. Karar kullanıcınındır ve bedeli kabul
edilmiştir — SoundCloud düştüğünde paket kırmızı yanar.

**Bunun üzerine yazılan kural — `CLAUDE.md`'nin "ağa bağlı test yazma"
cümlesi güncellendi.** Yeni hâli: *ağa bağlı test yazılabilir; ulaşamamak
başarısızlık değildir.* Ayrım K9'un kendi ayrımı:
- **Ulaşamamak** (DNS/TCP yok) → test kendini atlar, sebebini `stderr`'e
  yazar. Ses aygıtı testlerinin yordamının aynısı.
- **Ulaşıp beklenmeyeni almak** → test düşer. "Ulaşamadım" ile "hayır dedi"
  farklı tanılar, farklı çözümler.

**Kapsam kendiliğinden cevaplandı:** api 1'in metotları `search` +
`resolve_source` ile sınırlı, `browse` için tel biçimi yok. Eklenti
`SEARCH | STREAM` beyan ediyor.

**Ölçüm (2026-09-01, 200 parçalık örnek):** parçaların **%99'unda**
`progressive` (düz HTTP MP3) varyantı var, **%1'i** yalnızca HLS sunuyor.
HLS çözücü yazılmadı; o %1 için açık hata dönülüyor. `policy: SNIP` olan
4 parça 30 sn önizleme — başlığa `[önizleme]` ekleniyor, çünkü api 1'de
bunu taşıyacak alan yok ve kullanıcı çalarken şaşırmamalı.

---

## D-044 — Yeni gönderilen iş anında "bitmemiş" sayılır (canlı testin bulduğu kusur)
**Tarih:** 2026-09-01
**Soru:** Karar değil, D-043'ün canlı sürüşünün ortaya çıkardığı kusur ve
düzeltmesi. Kayda geçiyor çünkü aynı aile ikinci kez tekrarlandı (D-035).

**Kusur:** `headshell play` SoundCloud parçasını kuyruğa alıyor, sonra
`kaydedilen dinleme: 0` deyip **anında** çıkıyordu. Hata yok, uyarı yok,
`headshell diag` temiz. Ses hiç çalmıyordu.

**Sebep:** `AudioEngine::open()` cpal akışını hemen başlatıyor; geri çağrı
boş tampon + `idle` görüp `Stopped` basıyor. Ardından `play_source`
kaynağı açıyor (`open_source`) ve işi kuyruğa koyup `idle = false` yapıyor —
ama **durumu düzeltmeyi ilk geri çağrıya bırakıyordu.** Arada kalan pencerede
durum `Stopped` ve `Session::play`'in döngüsü tam da ona bakıyor:
"başlamadan bitti".

**Pencere neden şimdi görüldü:** yerel dosyada `open_source` birkaç
milisaniye, HTTP akışında **saniyeler** (4 MB indiriyor). Kusur Faz 1'den
beri oradaydı ve yalnızca uzak kaynakta görünüyordu.

**Düzeltme:** `play_prepared` işi kuyruğa koyarken durumu da **anında**
`Buffering` yapıyor. `idle` için zaten yapılan şeyin (kodda yorumu da vardı)
`state` için yapılmamış hâliydi. `Buffering` "hattın beklemesi"dir (D-016) ve
anlatılmak istenen tam olarak odur.

**Regresyon:** `a_freshly_queued_track_is_never_reported_as_stopped` —
motoru açıyor, geri çağrının `Stopped` basmasını **bekliyor** (ön koşulu
`assert` ediyor), sonra iş gönderip **uyumadan** durumu okuyor.

**Ders — bu üçüncü kez:** D-035'te GUI donmuştu (eksik durum olayı),
D-022'de "erişilemedi" ile "doğrulanamadı" birleşmişti, burada CLI sessizce
çıktı. Üçü de *eksik* sinyal; fazlası ölçülür, eksiği iz bırakmaz. Ve üçünü
de sahte bir sunucu değil **gerçek bir çalıştırma** buldu.

---

## D-045 — §2.3-2.5 turu: kapsam, sıra ve Chromaprint yolu
**Tarih:** 2026-09-01
**Soru:** D-041 §2.3/§2.4/§2.5'i ertelemiş ve "ayrı bir tur" demişti. O tur
şimdi açılıyor: neyi kapsayacak, hangi sırayla, ve parmak izi nereden gelecek?

**Turdan önce ölçülen gerçek — zincirin ortası ölü.** `session.rs`'in
`default_lookup()`'ı her zaman `OfflineLookup` döndürüyor; `impl
MetadataLookup` yalnızca `OfflineLookup` ve `StaticLookup` için var. Yani
bugün içe aktarılan her kayıt ya ISRC'den (export'ta varsa) ya da
`LocalKey`'den kimlik alıyor: K6 zincirinin **2. ve 3. halkası hiç
çalışmıyor** ve `authoritative_ratio()` ölçülen değil sabit bir sayı.
Bu bulgu §2.3'ün ne olduğunu değiştirdi — PLAN §2.3 yalnızca "AcoustID"
diyordu.

**Karar (S1 — §2.3 kapsamı): önce MusicBrainz, sonra AcoustID.**
Gerçek bir `MetadataLookup` (MusicBrainz; ISRC sorgusu + kayıt araması,
rate-limit'e uyan, `http-client` feature'ı arkasında) yazılır ve
`fixtures/identity/cases.json` üzerinde doğruluk oranı **ilk kez gerçekten
ölçülür**. Ardından AcoustID zincirin 4. halkası olarak eklenir.

**Gerekçe:** AcoustID yalnızca elde ses dosyası varken çalışır. İçe aktarılan
geçmişin dosyası yok — o kayıtların ezici çoğunluğu için tek otorite
MusicBrainz'dir. Etki sırası PLAN'ın yazdığının tersi; sıra bu yüzden
değişti, kapsam değil.

**Karar (S2 — Chromaprint): `rusty-chromaprint` crate'i.**
Öneri `fpcalc` alt süreciydi (bağımlılık ağacına tek satır eklemez, K5'in
zaten kullandığı sınır). Kullanıcı saf Rust'ı seçti: PCM zaten symphonia'dan
geliyor ve kullanıcıdan hiçbir kurulum istenmiyor.
**Kabul edilen bedel:** yeni bağımlılık (FFT dahil) ve parmak izinin `audio`
feature'ına bağlanması — `audio` kapalıyken 4. halka düşer, ve bu K9 gereği
sessizce değil "bu derlemede parmak izi yok" diye raporlanır.

**Karar (S3 — tur genişliği): üçü de bu turda.** §2.3 + §2.4 (torrent) +
§2.5 (yayın platformları). Öneri "yalnızca §2.3" idi (üç ayrı bağımlılık
kararı ve üç ayrı canlı test yüzeyi aynı anda açılıyor); kullanıcı Faz 2'nin
tümüyle kapanmasını seçti.

**Sıra:** §2.3 → §2.4 → §2.5. Her biri kendi kapısından geçer (`test`,
`clippy`, `fmt`) ve kendi commit'ini alır; tur sonunda §2.6 tablosu
güncellenir.

**Bu turda hâlâ açık, sırası gelince sorulacak iki alt karar:**
1. **§2.4 torrent nerede yaşar** — PLAN "`librqbit`" diyerek çekirdeği ima
   ediyor, ama K5 "sağlayıcılar alt süreç eklentisidir" diyor. Çelişki
   kod yazılmadan karara bağlanacak; ağaç boyutu o noktada ölçülüp sunulacak.
2. **§2.5 hangi platform(lar)** — EK tablosunda `STREAM` verilen üç aday var
   (SoundCloud yazıldı, Qobuz abonelik ister, YouTube Music bakımı en pahalı
   olan). Kaç tane ve hangisi, §2.4 bittiğinde sorulacak.

### D-045 eki — §2.3'ün MusicBrainz yarısı: canlı koşumun bulduğu üç kusur

**Tarih:** 2026-09-01. `crates/headshell-core/src/identity/musicbrainz.rs` +
`tests/identity_musicbrainz.rs`. Zincirin 2. ve 3. halkası artık çalışıyor;
`headshell --online resolve "..."` gerçek MusicBrainz'e bağlanıyor.

Üçü de **gerçek yanıt üzerinde** ortaya çıktı; hiçbirini sentetik katalog
gösteremezdi. Bu, D-044'ün dersinin dördüncü tekrarı.

**1. Canlı kayıt başlıkta değil, notta işaretli.** `variant_markers` yalnızca
başlığa bakıyordu. Gerçek katalogda `Radiohead — Creep` araması 191 kayıt
döndürüyor, ilk sayfanın çoğu canlı ve **hiçbirinin başlığında "live"
yazmıyor** — hepsi düpedüz `Creep`, ayrım `disambiguation` alanında. Ölçülen
sonuç: 1994 Astoria kaydı, süresi stüdyoya 12 sn yakın olduğu için **1.00
güvenle "tam isabet"**. Düzeltme: `Candidate.disambiguation` alanı,
`fuzzy::similarity`'ye `context_b` parametresi.
- Bunun açtığı ikinci soru: iki *farklı* canlı kayıt birleşmeli mi? Hayır
  (D-010). `Creep (Live at Glastonbury)` ile `live, 1994-05-27: Astoria`
  aynı işareti taşır, aynı performans değildir. Kural: kullanıcının verdiği
  ayırt edici kelime (`glastonbury`) adayın metninde karşılık bulmuyorsa
  ceza. Tek yönlü — adayın fazladan bildiği ayrıntı çelişki değil.

**2. Aynı sorgu iki koşumda iki farklı MBID verdi.** Beraberlik gerçek
katalogda istisna değil kural, ve `max_by` eşitlikte sunucunun gönderdiği
sıraya teslim oluyordu; o sıra sabit değil. Kimlik katmanında bu, aynı
parçanın yarın başka bir kanonik kimlik alması demekti. Düzeltme:
belirlenimci sıralama — skor, sonra `tiebreak_rank` (notu olmayan kayıt
varsayılandır; süresi bilinen tercih edilir), son çare MBID sırası.

**3. Beraberlik "%100 güven" diye raporlanıyordu.** 25 eşdeğer aday arasından
belirlenimci ama **keyfi** bir seçim yapılırken çıktı tam isabet iddia
ediyordu. Düzeltme: `Resolution.tied_candidates`; beraberlikte yöntem `Mbid`
olamaz ve güven tam isabet eşiğinin altına kırpılır. CLI bunu ayrı bir satırda
söylüyor, `diag` `identity.tied_candidates` olarak sayıyor.

**Değişen dışa açık imzalar** (§0.1'in "API imzası değişecek" tetikleyicisi;
üçü de Faz 2 içinde, `uniffi` hattı henüz kurulmadı):
- `Candidate` → `disambiguation: Option<String>` alanı,
- `Resolution` → `tied_candidates: usize` alanı (`serde(default)`, eski
  kayıtlar tekil sayılır),
- `fuzzy::similarity` → 7. parametre `context_b: Option<&str>`.

**Doğruluk kümesi: %97.2 → %100 (72/72).** Küme 69'dan 72 vakaya, katalog
20'den 22 girdiye çıktı; yeni sınıf `mb_disambiguation` gerçek MusicBrainz
biçimini taklit ediyor (başlık düz, ayrım notta). `ACCURACY_FLOOR` 0.97'de
bırakıldı: %100'e sabitlemek her yeni zor vakada testi kırar. **Kümenin
kolaylaştığı anlamına gelmiyor — zorlaştırılması gereken bir borç.**

**CLI bağlaması:** `--online` genel bayrağı, **varsayılan kapalı**. Seçim
çekirdekte (`session::LookupMode` + `lookup_for`), CLI yalnızca bayrağı
kipe çeviriyor (Altın Kural). Varsayılanın kapalı olması bilinçli: bir
export'u içe aktarmak kimseyi sessizce ağa bağlamamalı, ve MusicBrainz
saniyede bir istek kabul ettiği için binlerce parçalık bir `import --online`
saatler sürer.

**Canlı testler varsayılan koşumda** (D-043'ün kararının aynısı):
`tests/identity_musicbrainz.rs`, 6 test, ağ yoksa sebebini yazıp atlıyor.
`http-client` kapalı derlemede de atlıyor ve **bunu söylüyor**.

---

## D-046 — §2.3'ün AcoustID yarısı: anahtar nereden, halka zincire nereden
**Tarih:** 2026-09-02
**Soru:** D-045 parmak izi yolunu (`rusty-chromaprint`) seçmişti ama iki şeyi
açık bırakmıştı: AcoustID'nin istediği istemci anahtarı nereden gelecek, ve
zincirin 4. halkası `TrackRef`'in dosyası yokken nereye bağlanacak?

**Karar (S1 — anahtar): gömülü varsayılan + kullanıcı geçersiz kılması.**
Sıra: önce sır deposu (`identity:acoustid` / `api_key`, D-042'nin altyapısı),
yoksa derlemeye gömülü anahtar. Kullanıcının koyduğu **her zaman** kazanır.
Reddedilen seçenek "yalnızca sır deposu" idi: kutudan çıkar çıkmaz çalışmayan
bir 4. halka pratikte hiç çalışmayan bir halkadır.
**Ödenen bedel:** gömülü anahtar depoda görünür ve kötüye kullanılırsa AcoustID
onu iptal edebilir. Bu yüzden kullanıcı geçersiz kılması aynı turda yazıldı —
anahtar düşerse kimse kilitlenmiyor.
**Bugünkü durum:** `EMBEDDED_API_KEY` **boş** ve bilerek boş. Uydurulmuş bir
dize koymak, ilk canlı çağrıda "geçersiz anahtar" olarak dönerdi ve kusuru
anahtarda değil parmak izinde arattırırdı. `acoustid.org/new-application`
adresinden proje adına bir anahtar alınıp oraya yazılana kadar halka yalnızca
kullanıcının kendi anahtarıyla çalışır ve anahtarsız çağrı **ne yapılacağını
söyleyerek** reddedilir.

**Karar (S2 — bağlantı): ayrı giriş noktası, `Resolver::resolve_file(path)`.**
`TrackRef`'e `source_path` alanı eklenmedi. Sebep: o alan dışa açık bir tipin
imzasını değiştirir (§0.1 tetikleyicisi), her `TrackRef` üreten yeri
dokundurur ve **dosyası olmayan** import kayıtlarına ömür boyu boş bir alan
taşıtırdı. `resolve()` ve `TrackRef` hiç değişmedi.
**Ödenen bedel:** iki giriş noktası; çağıran hangisini kullanacağını bilmeli.

**Sıra korunuyor (K6).** `resolve_file` önce dosyanın **kendi etiketlerinden**
üç metin halkasını dener; yalnızca sonuç `LocalKey`'e düşerse sese sorar.
Parmak izi en pahalı halka (dosyanın tamamı çözülür) ve ilk üçü çalıştığında
gereksizdir — `a_tagged_file_never_reaches_the_fingerprint_link` bunu ölçüyor.

**İki başarısızlık ayrı tutuluyor (K9).** Parmak izinin **üretilememesi**
(dosya çok kısa, paketler bozuk) hata değil: sebebi loglanır ve zincir metin
tarafının bulduğu yerel anahtarla biter. AcoustID'ye **sorulamaması** ise
propagate edilir — anahtarı ayarlanmamış bir kurulumu "hiçbir şey eşleşmiyor"
diye raporlamak, kusuru dosyada arattırırdı.

**Değişen dışa açık imzalar:**
- `identity::FingerprintCandidate` (yeni), `identity::FingerprintLookup` (yeni trait),
- `Resolver::with_fingerprint_lookup`, `Resolver::resolve_file` (yeni),
- `Session::resolve_file`, `Session::fingerprint_lookup_for` (yeni),
- `net::HttpRequest::post_form` (yeni) — parmak izi base64'te binlerce karakter
  tutuyor ve URL'ye sığmıyor; kesilen bir URL "eşleşme yok" gibi görünürdü.
- `net::RateLimiter` musicbrainz'den `net`'e taşındı (AcoustID'nin de kotası var).

**CLI:** `headshell resolve --file <yol>`. `--online` kapalıyken zincir üç halkayla
biter ve bu bir kusur değil bir yapılandırmadır.

**Feature:** `fingerprint` (D-045'te tanımlandı) artık `headshell-cli`'de **açık**.
CLI her çekirdek yeteneğinin sınandığı yüzey; ağacı büyütmesi bilinçli bedel.

### D-046 eki — canlı koşumun bulduğu iki şey

**1. Kendi kusurum: geçersiz anahtar `400` ile geliyor, `200` ile değil.**
İlk sürüm gövdeyi durum kodundan **sonra** okuyordu, bu yüzden gerçek servisin
reddi `ADIM: NETWORK_REQUEST` diye raporlanıyordu — kullanıcıyı ağını kontrol
etmeye gönderen bir tanı, oysa yapması gereken şey anahtarını düzeltmek.
D-023'ün `401`/`403` için kurduğu ayrımın aynısı. Düzeltme: gövde önce
ayrıştırılır; AcoustID'nin kendi cevabıysa `IDENTITY_RESOLVE`, değilse (proxy
sayfası, bakım ekranı) durum koduna teslim edilir. **Sahte istemci bunu
gösteremezdi** — testim `200` varsayıyordu ve yeşildi. D-044'ün dersinin
beşinci tekrarı.

**2. Benim kusurum değil, ama D-045'in "kapandı" dediği kusur açık:
MusicBrainz araması koşumlar arası kararsız.**
`the_same_query_always_yields_the_same_canonical_id` canlı koşumda yine düştü
ve iki farklı MBID gösterdi. D-045 seçimi **küme içinde** belirlenimci yaptı;
ölçülen şey kümenin kendisinin sabit olmadığı. `mb_stability_probe` sondası:
aynı `Radiohead — Creep` araması art arda iki kez 25 aday döndürüyor ve bazı
koşumlarda **ortak aday sayısı sıfır**. MusicBrainz aramayı birden çok indeks
kopyasından sunuyor; bir kopya içinde sıra sabit, kopyalar arasında top-25
tamamen farklı. Yani belirlenimci sıralama bu sorunu **çözemez**: sıralanacak
küme her seferinde başka.
**Sonuç:** süresi ve ISRC'si olmayan belirsiz bir sorgu, hangi kopyanın
cevapladığına bağlı bir kanonik kimlik alıyordu. Kimlik katmanı için kabul
edilemez.

**Karar (S3 — belirsizlikte otorite iddia edilmez).** Ayırt edici kanıt yoksa
MBID döndürülmüyor; zincir yerel anahtarla bitiyor. Reddedilen iki seçenek:
*sayfalama* (tüm eşleşmeleri çekip kümeyi sabitlemek — belirsiz her sorgu ~9
saniye sürerdi, toplu çözümleme pratik olmaktan çıkardı) ve *eser (work)
düzeyinde kimlik* (kavramsal olarak en doğrusu ama K6'nın "kanonik = kayıt
MBID" tanımını değiştirir; ertelendi).
**Ölçülen sonuç:** `Radiohead — Creep` (süresiz) artık her koşumda
`local:b51521e93103eefa` — aday kümeleri **hiç kesişmediği** koşumda bile aynı.
`tied_candidates` (2 ya da 7) kanıtın ne kadar zayıf olduğunu kayıtta tutuyor:
"aday yok" ile "aday ayırt edilemedi" aynı kimliği üretiyor ama aynı tanı değil.

### D-046 eki (2) — kuralın açtığı ikinci kusur: süre kanıtı çöpe gidiyordu

S3 uygulandıktan sonra `Şebnem Ferah — Sil Baştan` **süresi verilmiş olduğu
hâlde** otorite kaybetti. Sonda sebebi gösterdi: üç aday — süreleri 309, 313 ve
315 sn, sorgu 309 sn — **üçü de tam 1.0000 skor alıyordu.**

Sebep `fuzzy::similarity`'de: taban skor metin tam uyduğunda zaten 1.0, süre
bonusu `clamp`'te yutuluyor, ve süre farkı üç bant (≤3 sn / 3–15 sn / ≥15 sn)
olarak okunduğu için bandın içindeki 0 sn ile 4 sn ayırt edilmiyor. Yani süre
gerçekten bilindiği hâlde kimlik seçiminde **kullanılmıyordu**.

**Düzeltme skorlamada değil, eşitlik bozmada.** Skor formülüne dokunulmadı
(doğruluk kümesi ona göre ayarlı); süre farkı sıralamaya `tiebreak_rank`'ten
sonra, MBID sırasından önce üçüncü ölçüt olarak eklendi ve `count_tied` artık
skoru, rütbeyi **ve** süre farkını paylaşanları sayıyor. Bilinmeyen süre
`u64::MAX`: "yakınlık iddiasında bulunamıyorum", en sona düşer.

**Ölçülen etki — bu bir düzeltme, bir denge değil:**
- Doğruluk kümesi **72/72 = %100** (değişmedi).
- `Şebnem Ferah — Sil Baştan` (süreli) artık `mbid` yöntemiyle
  `e0a22727-1fcf-4e3a-81a3-b65623b2c53e` veriyor — **ISRC halkasının aynı
  sorgu için verdiği kaydın ta kendisi.** Düzeltmeden önce `1eaab31f…`
  seçiliyordu, yani zincir *yanlış kaydı* seçiyordu ve bunu kimse ölçmemişti.
- `Radiohead — Creep` (süreli, 238 sn) tekil kazananla çözülüyor (`berabere 1`).

**Kalan risk — ve gerçekleşti.** Süreli sorgunun tekil kazanana ulaşması,
kazanan adayın o kopyanın ilk 25'inde bulunmasına bağlı. `Radiohead — Creep`
(238 sn) koşumların çoğunda `berabere 1` ile çözülüyor ama bazı koşumlarda
238 sn'lik kayıt kümede yok ve zincir yine yerel anahtara düşüyor. Yani süre
kanıtı **kararlılığı garanti etmiyor, yalnızca çoğu zaman sağlıyor.**

Bu, canlı testi kırdı ve testin yanlış şeyi ölçtüğünü gösterdi:
`a_live_take_does_not_win_over_the_studio_take` "her zaman bir aday dönmeli"
diyordu. Değişmez o değil — değişmez **canlı kaydın kazanamaması.** Kazanan
çıkmaması da o değişmezi bozmuyor ve D-046'nın kuralı gereği doğru davranış.
Test artık iki kabul edilebilir sonucu da tanıyor ve hangisinin olduğunu
yazıyor; reddettiği tek şey canlı bir kaydın seçilmesi.

**Bu turda kapanmayan:** kümenin kendisini sabitlemek (sayfalama) ya da kimliği
eser düzeyine taşımak. İkisi de belirsiz sorguların otorite almasını sağlardı;
ikisi de ayrı birer karar.

### D-046 eki (3) — gerçek anahtarla koşum: eşleşme yolu hiç çalışmıyormuş

**Tarih:** 2026-09-02. Kullanıcı AcoustID anahtarını verdi ve halka ilk kez
**gerçek anahtarla** koştu. Üç şey ölçüldü, üçüncüsü bir kusurdu.

**1. Ürettiğimiz parmak izi geçerli.** AcoustID 15 sn'lik sentetik fixture'ın
parmak izini kabul etti ve 0 aday döndürdü — sentetik ses için doğru sonuç.
Sıkıştırma, URL-güvenli base64 ve form gövdesi doğru.

**2. "Kabul etti" boş bir iddia değil — kontrol edildi.** Servis her dizeyi
kabul etseydi 1. maddedeki test hiçbir şey ölçmezdi ve sıkıştırıcımız bozulsa
bile yeşil kalırdı. Bilerek bozulmuş bir dize gönderildi: `code 3, invalid
fingerprint`. Kontrol artık kalıcı bir test.

**3. Kusur: `duration` alanı ondalık geliyor, `Option<u32>` yazılmıştı.**
Gerçek yanıt `"duration": 309.0` gönderiyor. `serde_json` ondalık bir değeri
`u32`'ye çözemez — yani **ilk gerçek eşleşme, eşleşmeyi ayrıştıramadan bir
JSON hatasıyla düşecekti.** Zincirin 4. halkası "çalışıyor" görünüyordu ve
hiç çalışmamıştı: eşleşme *bulunmayan* yol sınanmıştı, *bulunan* yol değil.

Kusuru gizleyen şey elle yazılmış fixture'dı: `"duration": 238` (tam sayı)
koymuştum, çünkü şemayı ölçmeden varsaydım. **Bu, D-044'ün dersinin altıncı
tekrarı** ve ilk kez sahte veri *kendisi* kusurun kaynağıydı — sahte sunucu
değil, sahte **gövde**.

**Düzeltmeler:**
- Alan `Option<f64>`, dönüşüm `duration_secs_to_ms` üzerinden. Anlamsız değer
  (negatif, `NaN`, sonsuz, 24 saatten uzun) `None` — süre artık eşitlik
  bozucu olduğu için (bkz. ek 2) uydurma bir süre kimliği yanlış kayda bağlar.
- Fixture artık elle yazılmıyor: `fixtures/identity/acoustid_lookup.json`
  canlı servisten alındı ve birim testler onu `include_str!` ile okuyor.
- **Donmuş fixture'ın yalana dönmesi ayrı bir kusur sınıfı ve o da kapatıldı:**
  `the_committed_fixture_still_matches_what_the_service_sends` canlı yanıtı
  çekip fixture'la *alan alan tip* karşılaştırması yapıyor. Değerler
  değişebilir (katalog yaşıyor), şekil değişemez. Tespitin kendisi de
  doğrulandı — fixture'ın süresi tam sayıya çevrildiğinde test tam o alanı
  adıyla gösterip düşüyor.

**Anahtar hakkında:** kullanıcının anahtarı `.env`'de (gitignore'da) duruyor ve
testlerde `HEADSHELL_ACOUSTID_KEY` olarak kullanıldı. `EMBEDDED_API_KEY` **hâlâ
boş** — o anahtarı kaynağa gömmek onu herkese açık hâle getirir ve bu, sırlar
dosyasında tutulan kişisel bir anahtar için kullanıcının ayrıca vereceği bir
karardır. Sorulmadan yapılmadı.

---

## D-047 — §2.4 torrent: alt süreç eklentisi, localhost akışı, Torznab araması
**Tarih:** 2026-09-02
**Soru:** PLAN §2.4 "`librqbit`" diyor ve bunu çekirdeğin içine koyuyormuş gibi
okunuyor; K5 ise "sağlayıcılar alt süreç eklentisidir" diyor. Çelişki kod
yazılmadan kapatılacaktı (D-045'in açık bıraktığı iki alt karardan biri).

**Karardan önce ölçülen ağaç bedeli** (`cargo tree -e normal`, benzersiz crate):

| | crate |
|---|---|
| `headshell-core` bugün (`fingerprint` açık) | 77 |
| `librqbit` 9.0.1 tek başına (`--no-default-features`) | 223 |
| çekirdeğe eklenirse **yeni** gelen | **+179** (ortak yalnızca 35) |

Yani çekirdek 77 → 256, **3,3 kat**. Ve bu ağaç `uniffi` ile mobile de gider —
K7'nin ve "ağaç küçük kalmalı (mobil binary boyutu)" kuralının doğrudan konusu.

**Karar (S1 — nerede yaşar): ayrı workspace crate'i, alt süreç eklentisi.**
`crates/headshell-plugin-torrent`, `librqbit` kullanan bağımsız bir Rust ikilisi,
çekirdekle §2.1'in JSON-RPC protokolü üzerinden konuşuyor. `headshell-core`'un
ağacı 77'de kalıyor. Çelişki K5 lehine kapandı; PLAN §2.4'ün "`librqbit`"
tavsiyesi geçerli, **yeri** değişti.

Dürüst olmak gerekirse bedelsiz değil: workspace tek `Cargo.lock` paylaştığı
için o 179 crate kilide giriyor ve `cargo test --workspace` onları derliyor.
Değişmeyen şey `headshell-core`'un **kendi** bağımlılık ağacı — mobil bağlamanın
taşıyacağı olan da o. Ölçü `cargo tree -p headshell-core` ile her zaman doğrulanabilir.

**Karar (S2 — ses nasıl teslim edilir): 127.0.0.1'de sıralı HTTP akışı.**
Eklenti `librqbit`'in `ManagedTorrent::stream(file_id)` akışını (`AsyncRead +
AsyncSeek`, parça önceliğini okuma konumuna göre ayarlıyor) yalnızca yerel
arayüze bağlı küçük bir HTTP/1.1 sunucusundan sunuyor ve `resolve_source`
`HttpStream` döndürüyor. **Protokolde tek satır değişmedi.**

Alternatif "tam indir, sonra `LocalFile` döndür" idi: basit ama `headshell play`
dakikalarca bloke olurdu ya da protokole ilerleme bildirimi eklemek gerekirdi.

K3 ihlali değil: röle edilen bir şey yok, akış kullanıcının kendi makinesinde
kendi çektiği veriden okunuyor. Sunucu `127.0.0.1`'e bağlanıyor ve yol içinde
süreç ömrü kadar yaşayan rastgele bir jeton taşıyor — aynı makinedeki başka
bir süreç adresleri deneyerek bulamasın diye.

**Karar (S3 — kapsam): çalma + arama.** Öneri "yalnızca çalma" idi (arama her
indeks için ayrı bir kazıyıcı demek, ve bakımı SoundCloud eklentisinden
pahalı). Kullanıcı aramayı da istedi.

**Karar (S3b — arama nereden): Torznab (Prowlarr/Jackett).** İtirazın kendisini
ortadan kaldıran yol bu: tek standart XML API, tek ayrıştırıcı. Hangi
indekslerin sorgulanacağını kullanıcı kendi Prowlarr/Jackett'ında seçer; bir
site bozulduğunda **bizim kodumuz değil** onların indeks tanımı güncellenir.
Depoda hiçbir siteye özel kazıyıcı durmuyor.

Bedeli kullanıcının bir kurulum yapması ve bu bedel K9 uyarınca gizlenmiyor:
Torznab yapılandırılmamışsa `search` sessiz boş küme değil, "yapılandırılmamış
— `headshell secret set plugin:torrent torznab_url ...`" diyen açık bir hata döner.
"Bulamadım" ile "bakmadım" ayrı tanılardır.

**Torznab bir *release* döndürür, bir parça değil** — ve bu, tel biçimindeki
`WireTrack`'e doğrudan uymaz. api 1'i büyütmeden çözüldü, iki adım:
1. `search "<sorgu>"` → release'ler; her birinin `id`'si infohash.
2. `search "<infohash>"` → o torrent'in içindeki ses dosyaları; `id`'ler
   `<infohash>/<dosya sırası>`.

Tek ses dosyası olan bir release'te `resolve_source("<infohash>")` doğrudan
çalar. Birden çok dosya varsa **tahmin etmez**: hangi dosyaların olduğunu ve
infohash'i aratmayı söyleyen bir hata döner (K9 — "hangisi olduğunu bilmiyorum"
sessizce ilk dosyayı seçmekten iyidir).

**Yeni bağımlılık:** `roxmltree` (Torznab RSS ayrıştırma) ve `reqwest` — ikisi
de workspace kilidinde zaten var (`roxmltree` resvg'den, `reqwest` librqbit ve
Tauri'den), yani kilide yeni bir isim eklemiyorlar. `headshell-core`'a hiçbiri
girmiyor.

### D-047 eki — inşanın bulduğu üç şey

**Tarih:** 2026-09-02.

**1. Ad ayrıştırmada sıra yanlıştı (üç kusur, tek sebep).** Yayım adından
sanatçı/başlık çıkarırken önce yılı, sonra gürültüyü, en son sanatçıyı
ayırıyordum. Sonuç: `Van Halen — 1984`'te yıl alınınca başlık boşalıyor ve
ayırma başarısız oluyor, **sanatçı da kayboluyordu**; scene adlarında
(`Portishead.Dummy.1994.FLAC`) yıl sondaki `FLAC`'in arkasında kaldığı için
hiç bulunmuyordu; ve Sigur Rós'un `( )` albümü "içi boş parantez" olduğu için
gürültü sayılıp siliniyordu. Doğru sıra: **önce sanatçı, sonra gürültü, en son
yıl.** "Hepsi gürültü" iddiası artık en az bir kelime gerektiriyor.

**2. Kusur: bütçe `add_torrent`'ı kapsamıyordu — ve bu üretimde de vardı.**
Zaman aşımını yalnızca `wait_until_initialized`'ın etrafına koymuştum. Oysa bir
magnet'te üstveriyi çözen `add_torrent`'ın kendisi: peer bulunamazsa orada
**süresizce** bekliyor. Soğuk bir magnet'te eklenti çekirdeğin 20 sn'lik çağrı
zaman aşımına düşüyor, kullanıcı "eklenti takıldı" görüyor ve sebebini hiç
öğrenemiyordu — yani K9'un tam olarak yasakladığı şey. Bütçe artık ikisini
birden sarıyor ve dolduğunda peer/tracker durumunu açıklayan bir hata dönüyor.

Bunu **yalnızca gerçek koşum gösterdi.** Birim testleri yeşildi; kusur ancak
akış sunucusuna gerçek bir HTTP isteği gidince ortaya çıktı, ve o istek
katalogda kayıt olmadığı için çıplak bir magnet üretmişti. Sahte bir oturum
"hemen döndü" derdi. D-044'ün dersinin yedinci tekrarı. Regresyon testi:
`a_source_with_no_peers_gives_up_within_the_budget_and_says_why`.

**3. Uçtan uca test peer kullanmıyor — ve bu bilinçli.** Test bir torrent
üretip verisini indirme dizinine koyuyor; `librqbit` karma doğrulayıp tamam
sayıyor. Böylece sınanan şey bizim kodumuz oluyor: üstveriden dosya listesi,
akış açma, `Range` yanıtlama, jeton denetimi. Peer'a bağlı bir test ağın hâline
göre bazen geçerdi ve D-043'ün ayırmak istediği iki başarısızlığı karıştırırdı.
**Sınanmayan şey açıkça şudur: peer'lardan indirme.** O `librqbit`'in kendi
test kümesinin işi.

**Kapanmayan konu — izin sözlüğü "rastgele peer" diyemiyor.** `plugin.json`
yalnızca iki DHT giriş noktası beyan ediyor; oysa bir torrent istemcisi
önceden bilinemeyen tracker'lara ve peer adreslerine, ayrıca kullanıcının
verdiği Torznab adresine bağlanır. D-040'ın sözlüğü ("ana bilgisayar listesi,
`*` yok") bunu ifade edemiyor. Beyanı eksik bırakıp `description`'da söylemeyi,
olmayan bir kısıtlama varmış gibi göstermeye tercih ettim. Sözlüğün
genişletilmesi ayrı bir karar ve sorulmadı.

**Ölçülmemiş bir bedel ölçüldü: disk.** D-047 "workspace tek `Cargo.lock`
paylaşıyor, o 179 crate kilide girer ve `cargo test --workspace` onları
derler" diyordu ama sayı vermemişti. Sayı şu: bu makinede `target/` 45 GB'ye
çıktı ve **disk doldu** — koşum bir derleme hatasıyla değil,
`No space left on device` ve linker'da `Bus error` ile düştü. `target/debug/
incremental` tek başına 12 GB'ydi (saf önbellek, silinince hiçbir çıktı
kaybolmaz). Silindikten sonra tur temiz geçti.

Bu bir kusur değil, ölçülmüş bir bedel: torrent eklentisi `headshell-core`'un
ağacını büyütmüyor (77'de kaldı, `cargo tree -p headshell-core` ile doğrulandı)
ama **workspace'in derleme yükünü** büyütüyor. Geliştirici makinesinde
`CARGO_INCREMENTAL=0` ya da düzenli `cargo clean` gerekebilir; CI'da tek bir
`--workspace` koşumu için disk ayırırken bu hesaba katılmalı.

## D-048 — §2.5 YouTube Music: arama InnerTube'dan, akış yt-dlp'den, format 140
**Tarih:** 2026-09-09
**Soru:** Faz 2'de kalan tek bölüm §2.5 idi ve açık alt karar "hangi platform,
kaç tane" idi. EK tablosunda `STREAM` verilen üç adaydan SoundCloud D-043'te
yazıldı; geriye Qobuz ve YouTube Music kaldı.

**Karar (S1 — platform): YouTube Music, tek eklenti.** Qobuz abonelik ister;
sende abonelik yoksa eklenti **canlı koşturulamaz** — ve D-044'ten D-047'ye
kadar her turun tek ortak dersi "kusuru yalnızca gerçek koşum gösterdi" oldu.
Sınanamayan bir eklenti yazmak o dersin tersini yapmak olurdu. YouTube Music
abonelik istemeyen tek aday.

Bu turun bir **protokol** turu olmadığı baştan kabul edildi: SoundCloud (Python)
ve torrent (Rust) ile protokol iki dilde ve iki farklı ses teslim biçiminde
zaten kanıtlanmıştı. Bu bir **ürün değeri** turu.

### Yazmadan önce ölçülenler

PLAN'ın EK bölümü "buradaki API bilgileri doğrulanmadı, bir eklenti yazılmadan
önce ilgili satır yeniden sınanır" diyor. Dört ölçüm yapıldı ve **üçü PLAN'ın
ima ettiği yolu değiştirdi.**

**Ölçüm 1 — yt-dlp'nin kendi araması yetmiyor.** EK "yol yt-dlp" diyor. Ama
`yt-dlp --flat-playlist -J "music.youtube.com/search?q=..."` sonuçları şöyle:

| gelen alan | durum |
|---|---|
| `title` | var |
| `id` | var |
| sanatçı | **yok** |
| süre | **yok** |
| albüm | **yok** |

Üstelik liste kirli: 20 sonucun içinde kanal sayfaları, çalma listeleri,
"10 hours ... for sleep with rain", "slowed + reverb" ve "30min Loop" var.
K6'nın 3. halkası (bulanık eşleşme) **sanatçı + başlık + süre** istiyor;
`duration_ms: 0` dönen bir sağlayıcı o halkaya hiç giremez.

**Ölçüm 2 — InnerTube'un "Songs" süzgeci tam da isteneni veriyor.**
`POST music.youtube.com/youtubei/v1/search`, `WEB_REMIX` istemci bağlamı,
`params: EgWKAQIIAWoKEAoQCRADEAQQBQ==` (şarkı süzgeci). Anahtar **gerekmiyor**
(HTTP 200, 676 KB). 20 satırın hepsi şarkı ve her satırda sanatçı, albüm,
süre (`4:11`) ve `videoId` var.

**Karar (S2 — arama nereden): InnerTube, yt-dlp değil.** İş bölümü şu:
InnerTube üstveriyi verir, yt-dlp sesi çözer. İkisi de kendi güçlü olduğu işi
yapıyor.

**Ölçüm 3 — `bestaudio` çalınamaz.** `headshell-core`'un symphonia feature'ları:
`mp3, flac, vorbis, isomp4, aac`. Yani **ne webm kabı ne opus çözücüsü var.**
yt-dlp'nin `bestaudio` seçimi format 251'i (opus/webm, 136 kbps) veriyor —
indirilir, çalınmaz. Mevcut ses formatları:

| id | kap | kodek | kbps | çekirdek çözebilir mi |
|---|---|---|---|---|
| 139 | m4a | mp4a.40.5 | 49 | evet |
| **140** | **m4a** | **mp4a.40.2 (AAC-LC)** | **130** | **evet** |
| 249/250/251 | webm | opus | 52/69/136 | **hayır** |

**Karar (S3 — format): `140/bestaudio[ext=m4a]`, `bestaudio` değil.** Kalite
kaybı var (130 kbps AAC yerine 136 kbps opus) ve bedeli bilinerek ödeniyor:
çalınabilen düşük kalite, çalınamayan yüksek kaliteden iyidir. Symphonia'ya
opus/webm geldiği gün bu satır tek kelimeyle değişir.

**Ölçüm 4 — ve bu turun asıl bulgusu: YouTube düz GET'i kısıtlıyor.**
Aynı `videoplayback` adresi, aynı dosya (4.054.677 bayt), üç istek biçimi:

| istek | kod | hız |
|---|---|---|
| düz GET | 200 | **32 KB/s** |
| `&range=0-` sorgu parametresi | 200 | 7,7 MB/s |
| `Range: bytes=0-` başlığı | 206 | **8,0 MB/s** |

**250 kat.** Düz GET 130 kbps'lik bir parçayı 2× gerçek zamanda indiriyor —
teknik olarak çalıyor ama hattaki en küçük dalgalanma sesi kesiyor, ve sebebi
hiçbir yerde görünmüyordu.

**Karar (S4 — kısıtlama nasıl aşılıyor): `Range: bytes=0-` başlığı.**
Protokolde bunu taşıyan alan zaten var (`source.headers`) ve çekirdeğin
`UreqClient::open_stream`'i 206'yı 2xx sayıp `Content-Range`'in verdiği
`Content-Length`'i okuyor — **tek satır değişmedi.** Sorgu parametresi biçimi
de çalışıyor ve neredeyse aynı hızda; başlık seçildi çünkü standart mekanizma
o ve adresi kurcalamıyor.

Bunun bir koruma önlemini aşmakla ilgisi yok (D-026 / ASLA YAPMA): ortada
şifre yok, DRM yok; sunucunun kendi desteklediği ve `206` ile cevapladığı
standart bir HTTP başlığı gönderiliyor.

### Karar (S5 — yt-dlp kütüphane değil, alt süreç)

`pip install yt-dlp` bir depo bağımlılığı olurdu; eklenti onun yerine
`yt-dlp`'yi **alt süreç** olarak çağırıyor ve `-J` çıktısını okuyor. Üç sebep:

1. Depoda hiçbir Python bağımlılığı yok — SoundCloud eklentisinin kuralı
   (yalnızca standart kütüphane) korunuyor.
2. Bozulduğunda kullanıcının gördüğü mesaj **yt-dlp'nin kendi mesajı** olur
   (K9). "Bir şey olmadı" değil, "Sign in to confirm you're not a bot".
3. yt-dlp'yi kullanıcı kendi paket yöneticisiyle günceller. YouTube'un
   bozduğu şeyi biz değil yt-dlp tamir eder — ve bu, EK'in "bakım maliyeti en
   yükseği" uyarısına verilen cevabın ta kendisi.

yt-dlp aranma sırası: `HEADSHELL_YTDLP` (yol) → `PATH`'te `yt-dlp` →
`python3 -m yt_dlp`. Hiçbiri yoksa `health` `reachable: false` diyor ve
`search`/`resolve_source` nasıl kurulacağını yazan bir hata döndürüyor —
sessiz boş sonuç değil.

### İzin beyanı yine eksik, yine bilerek

D-047'nin kapanmayan konusu burada tekrar çıktı. Ses adresi
`rr6---sn-u0g3jxaa-n5fz.googlevideo.com` gibi **her çözümde değişen** bir ana
bilgisayarda duruyor; D-040'ın sözlüğü joker kabul etmiyor (`*` yok).
`music.youtube.com` ve `www.youtube.com` beyan edildi, `*.googlevideo.com`
`description`'da anlatıldı. Olmayan bir kısıtlama varmış gibi göstermektense
beyanı eksik bırakmayı yine tercih ettim — ama artık bunu **iki** eklenti
yapıyor, yani sözlüğün genişletilmesi tekil bir sıkıntı değil.

### D-048 eki — inşanın bulduğu iki şey

**Tarih:** 2026-09-09.

**1. Kusur: arama sonuçları alaka sırasında değildi.** InnerTube'un ağacı derin
ve haber vermeden değişiyor, bu yüzden satırları yolu ezberleyerek değil
`musicResponsiveListItemRenderer` anahtarını **arayarak** topluyorum. Gezinme
bir yığınla yazılmıştı ve `stack.pop()` çocukları ters sırada veriyordu — yani
belge sırası tamamen bozuluyordu.

Görünen sonuç şuydu: `nujabes aruarian dance` sorgusunda ilk beş sonucun içinde
**aranan parça hiç yoktu**; onun yerine "Island", "Luv (sic)", "Horizon" ve iki
tane gitar coverı geliyordu. Hepsi geçerli parça, hepsi doğru biçimlenmiş —
yalnızca yanlış beş tane. Bir birim testi bunu göremezdi çünkü kontrol edeceği
her alan doluydu. Düzeltme tek kelime: çocuklar yığına **ters** basılıyor.

Bu, K6'nın 3. halkasını da doğrudan ilgilendiriyor: `resolve` sağlayıcıdan
gelen ilk adayları puanlıyor ve doğru aday listeye hiç girmezse zincir onu
hiçbir zaman göremez.

**2. Test yanlış şeyi iddia ediyordu — ve bunu da canlı koşum gösterdi.**
Sırayı sınamak için önce "aynı sorguyu 3 ve 12 limitiyle sor, kısa liste uzun
listenin başında aynı sırada durmalı" yazdım. Düştü. Sebep bizde değildi:
**YouTube aynı sorguya iki çağrıda aynı sırayı vermiyor.**

Ölçüm (aynı sorgu, beş ardışık koşum):

| sıra | kararlılık |
|---|---|
| 1. sonuç | **5/5 aynı** |
| 2. sonuç | 4/5 |
| 3. sonuç | 4/5 |

Test, kararlı olan tek şeyi iddia edecek şekilde yeniden yazıldı: tam eşleşen
bir sorguda **ilk sonuç aranan parça olmalı**. Bu, 1. maddedeki kusuru da
yakalıyor (kusurluyken ilk sonuç "Island"dı) ve servisin garanti etmediği bir
şeyi garanti saymıyor. D-045'in MusicBrainz'de öğrendiği dersin aynısı:
koşumlar arası kararsız bir servise kararlılık yazdıran test, kendi kodunu
değil servisi sınar.

**Ayrıca düzeltildi:** hata mesajını `err.to_string()` ile arayan assert
yanlış yere bakıyordu. `Error`'un `Display`'i yalnızca `ADIM: PROVIDER_CALL`
yazıyor; sebep zinciri `chain_text()`'te ve CLI ile GUI'nin kullanıcıya
gösterdiği de o. Test artık oraya bakıyor ve **iki şeyi birden** doğruluyor:
mesaj yt-dlp'nin kendi cümlesini taşıyor **ve** hangi aşamada olduğunu
söylüyor.

**Uçtan uca kanıt:** `headshell play "hopeless_0taku_guitar Aruarian Dance Guitar
with Rain"` — 69 saniyelik parça 88 saniyede baştan sona çaldı (aradaki ~19 sn
yt-dlp'nin çözümü ve ilk tamponlama), çıkış kodu 0, ve dinleme kaydı
veritabanına yazıldı. §2.5'in aradığı kanıt buydu.

## D-049 — Eklenti bağımlılık sözleşmesi: root isteyen eklenti yoktur
**Tarih:** 2026-09-09
**Soru:** §2.5 biterken kullanıcı sordu: bir eklenti sistemde kurulu bir
araca yaslanıyorsa o eklenti "çalışıyor" sayılabilir mi? "Her eklenti kendi
başına bağımlılıklarını getirmek veya sistemde root yetkisi almadan kuracak
bir script yazmak zorunda. Her işletim sistemi için tonlarca sıkıntı çıkmaz
mı diğer türlü?"

Soru D-048'in yt-dlp'sinden çıktı ama **tek bir eklentinin sorunu değil.**
Ölçüldüğünde ortaya çıkan şey şu: üç eklentinin üç ayrı bağımlılık sözleşmesi
var ve hiçbiri yazılı değil.

| eklenti | bugünkü gerçek sözleşme | kullanıcıdan istediği |
|---|---|---|
| `soundcloud` | `PATH`'te `python3` | Linux/macOS'ta genelde hazır; **Windows'ta yok** |
| `ytmusic` | `python3` **+ yt-dlp** | paket yöneticisi → **root** |
| `torrent` | `exec` hedefi (`./headshell-plugin-torrent`) **depoda yok** | `cargo build --release`, yani **Rust araç zinciri** |

Yani en ağır bağımlılığı olan eklenti, bu turda yazılan değil: torrent bir
ikiliyi çalıştırabilmek için önce derleyici kurduruyor.

**Karar: hiçbir eklenti kullanıcıdan root yetkisi ya da sistem çapında bir
kurulum isteyemez.** Bir eklenti ya bağımlılıklarını **kendisi getirir**, ya
da onları **root'suz kuran bir yordam** sunar. "Şunu paket yöneticinle kur"
bir kurulum yordamı değildir; her dağıtım ve her işletim sistemi için ayrı
bir destek yüzeyi açar ve o yüzeyi eklenti yazarı değil biz taşırız.

**Kuralın kaçmaması gereken yer.** D-048 bilerek "yt-dlp'yi kullanıcı kendi
paket yöneticisiyle günceller"e yaslanmıştı: YouTube yt-dlp'yi düzenli olarak
bozuyor ve tamiri yt-dlp yapıyor. Bu kural "depoya bir kopya dondur" diye
okunursa o bakım yükünü **biz devralırız** — EK'in "bakım maliyeti en
yükseği" uyarısının anlattığı şey tam olarak budur. Kuralın üç şartı birden
sağlanmalı: **root yok + her işletim sistemi + güncel kalabilir.**

**Ölçülen iyi haber:** tanılama tarafı zaten ayakta. Eksik bağımlılık sessizce
"sonuç yok"a dönüşmüyor; `headshell provider test` her iki durumda da KULLANILAMIYOR
diyor ve sebebini yazıyor (eksik `exec` için `PLUGIN_HANDSHAKE` + işletim
sisteminin hatası, eksik yt-dlp için `health`'in cümlesi). K9 raporlama
düzeyinde karşılanıyor; kırık olan **kurulabilirlik**, görünürlük değil.

**Ölçülen kötü haber, ve bu turun kendi kusuru:** `plugins/ytmusic/main.py`
eksik yt-dlp'de `pacman -S yt-dlp` diyor. Arch dışında **yanlış tavsiye** —
kullanıcının dediği "her işletim sistemi için tonlarca sıkıntı"nın kendi
kodumuzdaki örneği. Mesaj işletim sisteminden bağımsız hâle getirildi
(bkz. D-049 eki).

### Kararın kapsamadığı, sorulacak olan

Kural ne olduğunu söylüyor, **nasıl** olduğunu değil. Dördü de ayrı birer
karar ve hiçbiri bu turda alınmadı:

1. **Bir eklenti çalışma zamanında çalıştırılabilir dosya indirebilir mi?**
   yt-dlp tek dosyalık bir zipapp olarak dağıtılıyor (~3 MB, `pip` gerekmez);
   eklentinin protokolce zaten yazma izni olan `data_dir`'ine indirip kendini
   güncel tutması kuralın üç şartını da sağlar. Ama bu, "eklenti ağdan ikili
   çekip çalıştırıyor" demektir ve **izin sözlüğü bunu ifade edemiyor** —
   D-040'ın açığına üçüncü kez basılıyor (torrent'te rastgele peer, ytmusic'te
   değişken `googlevideo.com`, burada indirilen ikili).
2. **İndirme yoksa: root'suz kurulum betiği kim yazar, hangi dilde?**
   Betik Python olamaz — Python'un kendisi bağımlılıklardan biri.
3. **Torrent eklentisi nasıl dağıtılacak?** Platform başına önceden derlenmiş
   yayın çıktısı mı, yoksa "kaynaktan derle" mi kalacak? Bugünkü hâli kuralı
   en ağır ihlal eden şey.
4. **`python3` varsayılmaya devam edecek mi?** Linux ve macOS'ta savunulabilir,
   Windows'ta değil.

**Bir de sorulacak bir ekleme var:** manifeste makine okunur bir `requires`
alanı. Bugün eksik bağımlılık ancak süreç başlatıldıktan sonra (`health`) ya da
başlatılamayınca (`PLUGIN_HANDSHAKE`) anlaşılıyor; beyan edilmiş bir gereksinim
listesi `headshell plugin list`'in daha süreç açmadan "eksik: yt-dlp" demesini
sağlardı. api'yi kırmaz — D-039/§2.1'in kuralı gereği **eklemek sürümü
artırmaz.**

## D-050 — Eklenti motoru: çalışma zamanı host'un işi, eklentinin değil
**Tarih:** 2026-09-09
**Soru:** D-049 kuralı koydu ("eklenti root isteyemez") ama nasıl uygulanacağı
açıktı. Kullanıcı yönü verdi: *"Programın kendi eklenti motoru olsun ve
çalışacak scriptler onun üzerinden geçsin. Python dersen al Python olsun,
derlenirken Python'ın gerekli olduğu söylenir yeter. Her eklenti ayrı ayrı
paketler veya uğraştıracak şeyler getirmesin... onlar öyle elleri uzun
olmasın."*

Doğru yer burası ve bunu bir ölçüm doğruluyor: **`python3` bugün dört
eklentinin üçünün gereksinimi ve hiçbir yerde yazılı değil** — ne `README`'de,
ne `CONTRIBUTING`'de, ne `Cargo.toml`'da. Yani projenin fiilen bir çalışma
zamanı gereksinimi zaten var; eksik olan onu **sahiplenmek**.

**Karar (S1 — motor): `headshell`'un tek bir eklenti motoru olur, çalışma zamanı
Python'dur ve bu `headshell`'un kendi gereksinimi olarak bir kez ilan edilir.**
Derleme/kurulum belgesinde yazar. Eklentiler o motorun üstünde koşan
betiklerdir. Bir eklentinin "hangi Python", "kurulu mu", "nasıl kurulur"
sorularıyla işi olmaz — bunlar host'un soruları.

Yorumlayıcı **gömülmüyor.** Kullanıcının cümlesi zaten bunu söylüyor
("gerekli olduğu söylenir yeter") ve gömmenin bedeli ağır olurdu: ikili boyutu
ve mobilde CPython taşıma derdi. Sistem Python'u yeter, yeter ki **ilan
edilsin ve eksikse açıkça söylensin.**

**Karar (S2 — paketler): motorun kendine ait, ayrılmış bir ortamı olur.**
Düğüm buradaydı: "Python var" demek yt-dlp'yi getirmiyor — yt-dlp yorumlayıcı
değil, bir **paket**. Çözüm eklentiye bırakılmıyor:

- Eklenti `plugin.json`'da ne istediğini **beyan eder** (`requires`).
- Kurulumu **motor yapar**, veri dizinindeki kendi özel ortamına.
- Sisteme dokunulmaz, root istenmez, kullanıcının Python kurulumu kirlenmez.
- Eklentinin kendisi hiçbir şey kurmaz, indirmez, `pip` çağırmaz. D-049'un
  "elleri uzun olmasın" şartı tam olarak budur.

Bu, D-049'un üç şartını birden sağlıyor: **root yok** (özel ortam kullanıcının
veri dizininde), **her işletim sistemi** (tek yol, dağıtıma özel komut yok),
**güncel kalabilir** (paket motorca güncellenir; D-048'in yt-dlp gerekçesi
korunur — YouTube bozduğunda tamiri hâlâ yt-dlp yapar, biz değil).

**Karar (S3 — torrent): işlevi çekirdeğe taşınır, D-047'nin *yeri* geri
alınır.** — **İPTAL: bkz. D-056 (2026-09-19).** Bu alt karar hiç uygulanmadı;
torrent eklenti olarak kalıyor ve D-047 yürürlükte. Aşağısı iptal edilen
gerekçedir, S1/S2/S4 etkilenmedi. Sebep tutarlılık: torrent bir Rust ikilisi, betik değil, motordan
geçemez — ve bugünkü hâli D-049'u en ağır ihlal eden şey (kullanıcıya
`cargo build --release` yaptırıyor).

D-047'nin ölçümü hâlâ geçerli ve göz ardı edilmiyor: `librqbit` `headshell-core`'un
ağacına **+179 crate** ekliyor (77 → 256, 3,3 kat) ve o ağaç `uniffi` ile
mobile gidecek. Bu yüzden taşımanın **şekli** feature kapısı:

```toml
torrent = ["dep:librqbit"]   # varsayılan kapalı
```

Depo bunu zaten üç kez yaptı (`audio` D-016, `http-client` D-020,
`fingerprint` D-045): ağır bir yeteneği açık bir kapının arkasına koymak.
Masaüstü derlemesi kapıyı açar ve kullanıcı hiçbir şey derlemez; mobil ve
sunucu derlemeleri açmaz ve **`cargo tree -p headshell-core` yine 77 der.**
Koşulsuz taşıma da mümkündü ve reddedilmedi — ölçülmüş bir bedeli sebepsiz
ödemek olurdu.

K5 ihlali değil: K5 *eklentilerin* alt süreç olmasını şart koşuyor, torrent
ise eklenti olmaktan çıkıp yerleşik bir sağlayıcı oluyor — `local`, Subsonic
ve Jellyfin gibi (Faz 1).

**Karar (S4 — K5 duruyor):** "eklentiler herhangi bir dilde yazılabilir"
değişmiyor. Alt süreç + JSON-RPC sınırı herkese açık kalır; motor **tek yol
değil, desteklenen ve kurulum gerektirmeyen yol** olur. Depoda dağıtılan her
eklenti motordan geçer. EK'in Tencent satırındaki gerekçe ("o bölgedeki biri
eklentiyi kendi yazabilir — biz protokolü veririz, listeyi değil") böylece
bozulmuyor.

### Bu kararın değiştirdiği işler

Hiçbiri bu turda yapılmadı; hepsi §2.8'in kapsamı:

1. **Motor yazılacak:** Python bulma, sürüm kontrolü, özel ortamın kurulması,
   `requires` çözümü, ve eksiklik durumunda **hangi adımda ne eksik** diyen
   tanı (K9). Eksik bağımlılık bugün ancak süreç açıldıktan sonra anlaşılıyor.
2. **`plugin.json`'a `requires` alanı** — `api` kırılmaz, eklemek sürümü
   artırmaz (§2.1'in kuralı).
3. **`ytmusic`** kendi yt-dlp arayışını bırakır (`HEADSHELL_YTDLP` → `PATH` →
   `python3 -m yt_dlp` üçlüsü silinir), `requires: ["yt-dlp"]` der ve motorun
   verdiğini kullanır.
4. **`soundcloud`** ve `echo` motora taşınır — ikisi de stdlib, `requires` boş.
5. ~~**`torrent`** eklenti olmaktan çıkar; `crates/headshell-plugin-torrent`
   çekirdeğe feature'lı bir sağlayıcı olarak gider, `plugins/torrent/` kalkar.~~
   **İPTAL — D-056.** Yapılmadı ve yapılmayacak.
6. **`python3` `README`/`CONTRIBUTING`'de gereksinim olarak ilan edilir.**

### Sorulmayan, açık kalanlar

- **Özel ortam nasıl kurulur?** `venv` + `pip` sistem Python'una yaslanır ama
  her dağıtımda `pip` gelmiyor (Debian'da `python3-venv` ayrı paket). Motorun
  bunu nasıl çözeceği ve `pip` de yoksa ne diyeceği ayrı bir karar.
- **Paket doğrulama.** Motor ağdan paket çekiyorsa sürüm sabitleme, karma
  doğrulama ve bunun izin sözlüğünde nasıl görüneceği açık — D-040'ın
  açığına dördüncü kez basılıyor.
- **Çevrimdışı kurulum.** Ağ yokken motor ne yapar; "kurulmadı" ile
  "kurulamadı" ayrı tanılar (K9).

---

## D-051 — Belge sahipliği: her olgu tek dosyada yaşar
**Tarih:** 2026-09-09
**Soru:** Temizliğe başlarken ölçüldü: değişmez kurallar `CLAUDE.md`,
`PLAN.md §2` ve `CONTRIBUTING.md`'de **üç kez** yazılıydı; workspace ağacı,
kod konvansiyonları ve komutlar da ikişer kez. Kopyalar kaymıştı ve kayma
sessiz değildi — birbirini yalanlıyorlardı:

| çelişki | CLAUDE.md diyordu | gerçek |
|---|---|---|
| K7 | "trait object olmasın" | D-006 bunu gevşetti: `Arc<dyn Trait>` ve `async fn` serbest |
| faz numaraları | Faz 3 = odalar | PLAN: Faz 3 = GUI, Faz 4 = odalar, 5 = sosyal, 6 = mobil |
| şu anki faz | "Şu an Faz 0" | Faz 0–3 kapandı, §2.8 açık |
| workspace ağacı | olmayan `sync/` listeleniyor | `net/`, `sleeve/`, `session.rs`, `headshell-plugin-torrent`, `plugins/` hiç yok |
| CLI yüzeyi | 8 komut | gerçekte `sleeve`, `scan`, `server`, `library` dahil daha fazlası |

En tehlikelisi K7'ydi: **ihlal edilemez denen bir kuralın geçersiz yazımı**,
her oturumda okunan dosyada duruyordu. README'nin yol haritası da Faz 5'i
"Mobil" sanıp Sosyal Graf'ı tamamen atlamıştı.

**Karar: her olgunun tek bir sahip dosyası vardır. Sahip olmayan dosya o
olguyu tekrar etmez, sahibine işaret eder.**

| olgu | sahip |
|---|---|
| çalışma protokolü, değişmez kurallar (K1–K10), faz planı, ASLA YAPMA, sözlük | `PLAN.md` |
| workspace ağacı, komutlar, CLI test yüzeyi, kod konvansiyonları, tanılama pratiği, test düzeni | `CLAUDE.md` |
| katkıcı süreci (üç kapı, doğruluk kümesi, lisans) | `CONTRIBUTING.md` |
| bir kararın gerekçesi | `DECISIONS.md` |

**Gerekçe — neden kurallar CLAUDE.md'de değil:** kural metni gerekçesiyle
birlikte anlam taşır ("K7 neden gevşetildi?" sorusunun cevabı kuralın
yanındadır). Gerekçeli metin uzundur, uzun metin her oturumda okunan dosyaya
sığmaz, sığdırmak için kısaltılınca da kayar. Kayan şey zaten buydu.

**Gerekçe — neden ağaç ve komutlar PLAN.md'de değil:** `CLAUDE.md` her
oturumda otomatik okunur, `PLAN.md` okunmaz. Ajanın her gün ihtiyaç duyduğu
operasyonel bilgiyi okunmayan dosyaya koymak, 87 KB'lık bir dosyayı her
oturumda açtırmak demektir. Sahiplik "önemliye göre" değil, **kullanım
sıklığına göre** bölündü.

**İki dosyada birden duran tek şey:** K1'in (Altın Kural) tam metni ve
K1–K10 başlık indeksi. İndeks bir başlık listesidir, kayacak gövdesi yoktur;
K1 ise kod yazarken en sık ihlal edilen kural olduğu için özetin içinde
duruyor ve "tam metin PLAN.md §2" diye işaretli.

**Uygulandı:** `CLAUDE.md` yeniden yazıldı (kurallar → indeks + çapa, ağaç
gerçeğe çekildi, CLI yüzeyi tamamlandı, faz durumu tamamen çıkarıldı);
`PLAN.md §3` konvansiyon/ağaç/komut kopyalarını bırakıp çapaya döndü;
`CONTRIBUTING.md`'nin kural ve konvansiyon kopyaları çapaya döndü;
`PLAN.md` K10 ile `README.md` yol haritasının faz numaraları düzeltildi.

**Kayma için tek panzehir:** faz durumu artık **yalnızca** PLAN.md'nin faz
başlıklarındaki `TAMAM` / `YAPILACAK` işaretlerinde. Başka hiçbir dosya
"şu an hangi fazdayız" cümlesi kurmaz.

---

## D-052 — K7'nin sınırı: "dışa açılan" uniffi'nin ihraç ettiğidir
**Tarih:** 2026-09-09
**Soru:** D-051'in belge temizliği bitince K7 kodda denetlendi ve üç ayrı
bulgu çıktı. Hepsi "lifetime/generic var" diyordu ama üçü aynı şey değildi:

| bulgu | ne | karar |
|---|---|---|
| `PlayOptions<'a>` (`session.rs`) | dışa açılan record'da lifetime | **ihlal — düzeltildi** |
| 24 public yapıcıda `impl Into<String>` / `impl AsRef<Path>` | ergonomik generic | **kural dışı — kalıyor** |
| `ProviderFuture<'a,T>`, `HttpFuture<'a>`, `LookupFuture<'a,T>` | dyn-uyumlu async trait | **bilinen borç — izleniyor** |

**Karar 1 — "dışa açılan imza" = `uniffi`'nin ihraç edeceği yüzey.**
`Session` metodları, o imzalardan geçen tipler ve callback interface olarak
modellenen trait'ler. Bir tip bu yüzeyden geçiyorsa lifetime taşıyamaz.

**Gerekçe:** `uniffi` yalnızca işaretlenmiş öğeye bakar. `PlayOptions` bir
record olarak ihraç edilecek ve `uniffi` bir record alanında `&'a str`'i
ifade edemez — bu gerçek bir engel. Ama `ProviderTrackId::new(id: impl
Into<String>)` ihraç edilmek zorunda değil: Faz 6'da yanına
`#[uniffi::constructor] fn create(id: String)` eklenir, mevcut Rust
çağıranları kırılmaz. Geniş okuma 24 imzayı `String`'e çevirip her çağrı
yerine `.to_owned()` ektirirdi; kazanç yok, ergonomi kaybı var.

**Karar 2 — `PlayOptions` sahipli `String` taşır.** `query: &'a str` →
`query: String`, `Copy` düştü. Bedeli komut başına tek bir kısa metin kopyası.

**Karar 3 — kural artık kodda denetleniyor.** `crates/headshell-core/tests/
k7_surface.rs`: public bir `struct`/`enum`/`type` lifetime aldıysa ya da
public bir imza closure parametresi alıyorsa test düşer, `ADIM: K7_SURFACE`
ile hangi dosya:satır olduğunu söyler (K9).

**Bu gerçek `uniffi` scaffolding üretimi değildir** ve yerine geçtiğini
iddia etmiyor. Gerçek kontrol çekirdekteki ~60-80 tipi
`#[derive(uniffi::Record)]` ile işaretlemeyi ister; o iş Faz 6'ya ait (K10)
ve aşağıdaki borç yüzünden ilk günden kırmızı yanardı. Ucuz süzgeç önce
gelir.

**Bilinen borç:** `uniffi` bir trait metodunun dönüşünde
`Pin<Box<dyn Future + Send + 'a>>` ifade edemez. `Provider`, `HttpClient`,
`MetadataLookup` ve `FingerprintLookup` bugün böyle yazılmış — kaza değil,
dyn-uyumlu async trait'in makrosuz tek yolu (D-006 `Arc<dyn Trait>`'i
serbest bıraktığı için gerekli). Faz 6'da dördü de `uniffi`'nin kendi async
makinesine göre yeniden yazılacak. Test bunları `BOXED_FUTURE_ALIASES`
listesinde tutuyor; **liste bir borç kaydıdır, muafiyet değil** — yeni ad
eklemek borcu büyütür, önce sorulur.

**Yan bulgu — denetimin kendisi kusurluydu.** İlk yazımda test modülü
ayıklaması ilk `#[cfg(test)]`'ten sonrasını topluca kesiyordu; test
modülünden *sonra* tanımlanan her public tip denetimin dışında kalıyordu.
Enjekte edilen ihlal yakalanmayınca çıktı. Süslü parantez sayan blok
atlamaya çevrildi ve iki ihlal sınıfı da enjeksiyonla doğrulandı: yeşil
olduğu için değil, kırmızı yakabildiği için güveniliyor.

## D-053 — CI: depoda hiç yoktu, üç kapı artık makinede koşuyor
**Tarih:** 2026-09-09
**Soru:** PLAN.md'nin Faz 6 bölümünde cevaplanmamış bir KARAR NOKTASI
duruyordu: *"CI'da `uniffi` scaffolding üretimi denensin… ne zaman
eklenecek? Öneri: hemen."* Sorulunca "şimdi" denildi.

Ölçüldüğünde asıl eksik ortaya çıktı: **depoda hiç CI yoktu.** Üç kapı
(`fmt`, `clippy`, `test`) CONTRIBUTING.md'de yazılıydı ve yalnızca elle
koşulursa koşuyordu.

**Karar:** `.github/workflows/ci.yml` — push ve PR'da üç kapı. K7 yüzey
denetimi (D-052) üçüncü kapının içinde koşuyor.

**Sistem bağımlılıkları:** `cpal` ALSA'ya, Tauri kabuğu WebKitGTK'ya
bağlanıyor; `libasound2-dev` + `libwebkit2gtk-4.1-dev` ve arkadaşları
olmadan `--workspace` derlenmiyor.

**Ağ testleri (D-043) CI'da koşar.** Ulaşamamak başarısızlık değil, o yüzden
ayrı bir "atla" düğmesi eklenmedi: CI'da yalnızca SoundCloud ve MusicBrainz
gerçekten koşar; AcoustID anahtarsız, `ytmusic` yt-dlp'siz, `torznab`
yapılandırmasız oldukları için kendilerini atlar. Kırmızı yanan bir ağ
testi "ulaşamadım" değil, "ulaşıp beklenmeyeni aldım" demektir (K9) — ve o
zaten bilinmesi gereken şeydir.

**İkinci iş `core-alone`, ve bugün kırmızı.** `--workspace` koşumunda
`headshell-cli` ile `headshell`, `headshell-core`'un `audio`/`http-client` feature'larını
açıyor ve varsayılan derlemedeki ölü kodu gizliyor. Mobil (Faz 6) çekirdeği
bu feature'lar olmadan derleyecek. Bugün üç kusur var: `net::network_err`
ölü, `net::fake::last_request` ölü, `playback/player.rs:228`'de karşılanmayan
bir lint beklentisi. Bu yüzden iş `continue-on-error: true` ile **rapor**,
kapı değil. Üçü düzeltilince o satır kaldırılmalı.

**Doğrulanmamış:** CI hiç koşmadı — bu commit'in kendisi ilk koşum olacak.
Sistem bağımlılığı listesi ve `ubuntu-24.04` üzerindeki WebKitGTK sürümü
(`4.1`) yerel makinede değil, yalnızca okunarak seçildi.

---

## D-054 — Borç temizliği: belgenin iddia ettiği ile kodun yaptığı ayrışmıştı
**Tarih:** 2026-09-18 · **Durum:** UYGULANDI (2026-09-18)

**Soru:** "Olmayan özellikler, yanlış planlar, yanlış yazılmış özellikler"
temizlensin. Bu bir karar sorusu değil, bir **ölçüm** sorusuydu: hangi iddia
tutmuyor?

**Yöntem:** üç kapı koşuldu (üçü de zaten temizdi), sonra her belge iddiası
koda karşı tek tek sınandı — CLI alt komutları `main.rs`'e, çıktı örnekleri
`output.rs`'e, TUI maketi `tui.rs`'in çizimine, sır deposu `secrets.rs`'e,
faz durumları koda. Kodda tek bir `TODO`/`FIXME`/`unimplemented!` yoktu;
borcun tamamı belgelerdeydi ve bir kısmı **yanlış**, olmayan değil.

### 1. CI'nın maskelenmiş kırmızısı kapandı — iş artık kapı

D-053'ün `core-alone` işi `continue-on-error: true` ile rapor olarak
duruyordu. Üç kusur da düzeltildi:

- `net::network_err` — yalnızca `ureq_client` (`http-client`) ve testlerdeki
  sahte istemci çağırıyor. `#[cfg(any(feature = "http-client", test))]`
  eklendi; varsayılan derlemede artık **yok**, susturulmuş değil.
- `net::fake::last_request` — tek çağıranı `fingerprint` arkasındaki AcoustID
  testleri. `#[allow(dead_code)]` + gerekçe: ölü değil **koşullu**. Feature
  adını buraya yazmak, genel bir test yardımcısını tek özelliğe bağlardı.
- `playback/player.rs` — `#[expect(unused_variables)]` hiç karşılanmıyordu,
  çünkü hemen altındaki `let _ = (source, item);` lint'i zaten susturuyor.
  Beklenti kaldırıldı; iki kemerden biri sökülmüş oldu.

Ölçerken **dördüncü** bir kusur çıktı: `--features http-client` tek başına
açıldığında (`audio` kapalı) `tests/remote_http.rs`'in `Resp::bytes` ve
`fixture` yardımcıları ölüyor. Borcun biriktiği yer feature *birleşimi*
değil, tek başına açılan feature'mış. CI işi bu yüzden `core-features`'a
dönüştü: varsayılan + her feature tek tek + hepsi birden.

### 2. README'nin dört yanlış iddiası

- **"Diskte şifrelenmiş anahtar/token olarak saklanır" — yanlıştı.**
  Hiçbir şey şifrelenmiyor. Uzak sunucuda parolanın kendisi gerçekten diske
  yazılmıyor (Subsonic'te `salt` + `md5(parola+salt)`, Jellyfin'de erişim
  anahtarı türetiliyor) ama saklanan token o sunucuya erişim için parolanın
  yerine geçiyor ve `servers.json`'da düz metin duruyor, unix'te `0600`.
  Eklenti/AcoustID sırları ayrı dosyada (`secrets.json`), aynı şekilde.
  Bu bir kusur değil, D-042'nin bilinçli kararı; kusur onu **şifreliymiş gibi
  anlatmaktı.** Bir güvenlik vaadinin yanlış olması, olmayan bir özelliği
  anlatmaktan kötüdür: kullanıcı diskini paylaşırken buna göre karar verir.
  Metin ne koruduğunu ve neyi korumadığını söyleyecek şekilde yeniden yazıldı.
- **`headshell stats` örnek çıktısı uydurmaydı** — emoji başlıklar, yüzdeler ve
  `─────` çubukları. `output.rs` böyle bir şey basmıyor. Örnek, biçimleyicinin
  hizalamasıyla birebir üretilip değiştirildi.
- **TUI maketi gerçek çizimle uyuşmuyordu** — satır başına süre ve albüm
  sütunu, `Kuyruk (3/10)` sayacı, `[Tekrar: TÜMÜ]` rozetleri. Gerçek kuyruk
  satırı yalnızca `sanatçı - başlık`. Maket `draw_*` fonksiyonlarına göre
  yeniden çizildi.
- **`headshell provider scan` "artımlı" diye anlatılıyordu.** Bayraksız hâli tam
  tarama; artımlı olan `--if-stale` ve o da dizin damgasına bakıyor, yerinde
  yeniden etiketlenen dosyayı görmüyor. CLAUDE.md'de aynı komut
  `--incremental` diye yazılıydı — öyle bir bayrak hiç olmadı.

Ayrıca: depo adresi `kullanici-adi/headshell` yer tutucusuydu; `python3`/`yt-dlp`
gereksinimi hiçbir kullanıcı belgesinde yazmıyordu (PLAN §2.8'in 6. maddesi
bunu kendisi itiraf ediyordu); komut tablosunda `provider remove/servers`,
`plugin disable/enable/forget`, `secret list/remove` eksikti.

### 3. `headshell provider search` diye bir komut yok

`plugins/torrent/README.md` iki adımlı arama akışını `headshell provider search`
ile anlatıyordu. `ProviderCommand` = `list | test | scan | add | remove |
servers`; `search` hiç yazılmadı. Eklentinin araması `headshell play` üzerinden
çalışıyor (`Session::queue_from_search`, katalog boşsa akıtabilen bütün
sağlayıcılara sorar).

**Ve burada gerçek bir CLI eksiği ortaya çıktı:** `output::play` yalnızca
`sanatçı - başlık` basıyor, sağlayıcı parça kimliğini basmıyor. Torrent'in
yayım→dosya seçimi (`<infohash>/<sıra>`) kimliği görmeyi gerektiriyor, yani
belgelenen akış insan çıktısından **izlenemiyor**, `--json | jq` şart.
Düzeltilmedi: çıktıyı değiştirmek snapshot testlerini kırar ve bu bir ürün
kararıdır. PLAN §2.6'ya açık borç olarak yazıldı, README `--json` yolunu
gösteriyor.

### 4. Bayatlamış plan maddeleri

- **§0.3'ün biçim örneği `D-007` numarasını kullanıyordu** ve depoda
  bambaşka bir konuda (diag hata zinciri) gerçek bir D-007 vardı. Örneği
  arayan okur yanlış kararı buluyordu. Örnek `D-NNN`'e çevrildi ve
  DECISIONS.md'nin gerçekten kullandığı alan biçimine uyduruldu.
- **Kapanmış Faz 0'da üç karar noktası hâlâ "Sor." diyordu.** İkisinin cevabı
  koddaydı: şema yazıldı ve `user_version` ile sürümlendi; doğruluk tabanı
  `ACCURACY_FLOOR = 0.97` (bugünkü ölçüm 72/72 = %100). Üçüncüsü —
  Last.fm/ListenBrainz içe aktarma — "bu fazda mı, Faz 2'de mi" diye
  soruyordu ve **iki faz da kapandı, hiçbirinde yazılmadı.** Soru bayat: artık
  "hangi fazda" değil "yapılacak mı" sorusudur. Sahipsiz olarak işaretlendi.
- **Beş yerde K5 yerine K4 yazılmıştı.** "Alt süreç + JSON-RPC" K5'tir; K4
  Spotify kuralı. D-050'nin kendi metni iki paragraf arayla önce doğru (K5)
  sonra yanlış (K4) yazıyordu.

### 5. Tel değeri arayüz metni sanılıyordu (D-036)

`RepeatMode`'un `Display`'i `off`/`all`/`one` basıyor — JSON ve IPC için
doğru. Ama hem TUI'nin kuyruk başlığı hem de masaüstü arayüzünün düğme ipucu
o dizeyi **kullanıcıya** gösteriyordu. D-036: tanımlayıcı İngilizce, arayüz
yazısı Türkçe. Her iki kabuğa kendi etiket eşlemesi eklendi (`kapalı/tümü/
tek`); tel değeri değişmedi ve bir test bunu kilitliyor.

**Sonuç:** 433 test geçiyor, 7'si kendini atlıyor ve sebebini yazıyor.
Üç kapı + `core-features` temiz.

**Yapılmayanlar (bilerek):** `EMBEDDED_API_KEY` hâlâ boş (D-046), eklenti
motoru hâlâ yazılmadı (D-050), izin sözlüğü hâlâ joker kabul etmiyor (D-040),
`play` insan çıktısı hâlâ kimlik basmıyor. Dördü de PLAN §2.6'da açık borç
olarak duruyor — bu tur onları **saymak** için açıldı, kapatmak için değil.

---

## D-055 — Eklenti motoru: sabitlenmiş eser, `pip` yok, ve dört ayrı tanı
**Tarih:** 2026-09-19 · **Durum:** UYGULANDI (2026-09-19)

**Soru:** D-050 motoru kararlaştırdı ama özel ortamın **nasıl** kurulacağını
açık bıraktı (PLAN §2.8'in karar noktası): `venv` + `pip` sistem Python'una
yaslanıyor, her dağıtımda gelmiyor, ve ağdan paket çekiliyorsa sürüm
sabitleme + karma doğrulama + izin sözlüğü soruları cevapsızdı.

### 1. Ortam: sabitlenmiş tek dosyalık eser (S1)

Soruyu keskinleştiren bir ölçüm: **bu makinede sistem `pip`'i yok**
(`python3 -m pip` → "No module named pip") ama `ensurepip` var, yani
`python3 -m venv` 1,4 sn'de 13 MB'lık bir ortam kurup içine `pip 26.2.1`'i
kendisi koyabiliyor. "pip yok" ile "venv kuramam" aynı şey değil.

Ama Debian `python3-venv`'i ayrı paketliyor ve orada ikisi birden düşüyor —
motorun kullanıcıya sunabileceği root'suz bir çıkış yolu kalmıyor. D-049'un
tam kaçındığı yer burası.

**Karar: `venv`/`pip` yok. Eklenti manifestinde sabitlenmiş bir eser beyan
eder, motor onu indirir ve sha256'sını doğrular.**

```json
"requires": [{
  "name": "yt-dlp", "version": "2026.08.19",
  "url": "https://github.com/.../yt-dlp",
  "sha256": "1fa6733c…"
}]
```

Dört alanın dördü de zorunlu, `url` `https://` olmak zorunda, karma tutmazsa
dosya **yerine konmaz**. Sabitleme ve doğrulama olmadan "root istemeyen
kurulum" yalnızca yeri değişmiş bir güven sorunu olurdu.

Beyanı **yüklemede** doğruluyoruz, kurulumda değil: sürümsüz ya da karmasız
bir `requires` ile eklenti hiç listelenmiyor. Kurulum anına bırakılsaydı kusur
ancak kullanıcı komutu yazınca çıkardı.

**Kabul edilen bedel:** yalnızca tek dosya olarak dağıtılan şeyler kurulabilir.
`requests` isteyen gelecek bir eklenti bu yolu kullanamaz. Bugünkü bütün
eklentilerin toplam ihtiyacı bir tane — yt-dlp, ki zaten zipapp.

**Şemaya `kind` alanı bilerek konmadı.** "İleride pip'li türü de ifade
edebilsin" diye bir ayrım eklemek, tek varyantlı bir enum yazmak olurdu.
Alan eklemek `api`'yi kırmıyor (§2.1); ihtiyaç doğduğunda eklenir.

### 2. Yorumlayıcı: bir kez bulunur, ve seçim sessizce değiştirilmez

Eklenti `"exec": ["python3", "./main.py"]` yazıyor; çıplak `python3`/`python`
adını motor çözüyor (3.9+ şartıyla). `HEADSHELL_PYTHON` → yoksa `python3` → `python`.

**Ölçerken bir kusur çıktı ve düzeltildi:** ilk yazımda `HEADSHELL_PYTHON` yalnızca
aday listesinin başına konuyordu. Yanlış gösterildiğinde motor sessizce
`python3`'e düşüp "python3 (3.14.7)" diye rapor veriyordu — kullanıcı kendi
seçiminin uygulandığını sanırdı. Artık açık seçim **ayrı** değerlendiriliyor
ve başarısızlığı nihai: geri düşülmüyor, sebep söyleniyor.

### 3. Dört ayrı tanı (K9)

Bir eserin "hazır olmaması" tek bir şey değil, ve dördü dört ayrı şey
gerektiriyor:

| durum | ne demek | kim düzeltir |
|---|---|---|
| `kurulu değil` | hiç kurulmadı | kullanıcı — `headshell plugin install <ad>` |
| `karma tutmuyor` | diskte var, doğrulanmıyor | kullanıcı — yeniden kur |
| `kurulamadı` | ağa çıkılamadı | kimse — yarın tekrar dene |
| **`YETİM`** | kaynak 404/410 dedi | **eklenti yazarı** — adres ölmüş |

Yetim kavramı kullanıcının önerisiydi ("linkler eskiyince yetim bırakalım
olur mu?") ve doğru yere oturuyor: sabitlenmiş bir adres bir gün mutlaka
ölür, ve o gün kullanıcıya "ağını kontrol et" demek yanlış tavsiye olur.

**Yetimlik diske yazılmıyor.** Bir GitHub kesintisi 404 değil 5xx döndürür,
ama yazılsaydı tek bir kötü an bir eklentiyi kalıcı olarak yetim damgalardı.
Tanı ölçüldüğü anda söylenir, hatırlanmaz.

**Kurulu bir eserin kaynağı ölürse hiçbir şey olmaz:** dosya diskte, karması
tutuyor, çalışmaya devam eder. Yetimlik yalnızca henüz kurmamış bir kullanıcı
için bir sorun — yani yeni sürüm yayımlamak eklenti yazarının sorumluluğu.

### 4. İzin sözlüğü: ayrı satır, karışmıyor (D-040'a dördüncü basış)

**Karar: motorun indirmesi eklentinin `permissions.net` listesine girmez.**
Onay ekranında ayrı bir `motor` satırında görünüyor — ne indirileceği, nereden
ve hangi sha256 ile.

Gerekçe: indirmeyi eklenti değil motor yapıyor. Aynı listeye karışsaydı
kullanıcı "bu eklenti github.com'a bağlanıyor" diye okurdu ve bu **yanlış
bilgi** olurdu. D-040'ın joker açığı bu turda kapanmadı ama **büyümedi** de.

### 5. Tetik: ayrı bir komut

`headshell plugin install <ad>`. `approve` sırasında indirmek onayı pahalı yapardı;
ilk kullanımda indirmek `headshell play`'i beklenmedik bir indirmeyle geciktirirdi.
Ayrı komut açık, betiklenebilir, ve ikinci kez koşturmak ücretsiz (kurulu
eser için ağa hiç çıkılmıyor — ölçüldü: 1,27 sn → 0,19 sn).

Komut `--online` **beklemiyor**. Bayrak örtük ağ erişimini engellemek için
var ("bir export'u içe aktarmak kimseyi sessizce ağa bağlamaz"); burada
indirme komutun kendisi, yan etkisi değil.

### 6. Bağımlılık: `sha2` eklendi (sorularak)

Ölçülen bedel **+8 crate** (51 → 59 benzersiz; `cfg-if` ağaçta zaten vardı)
ve bu ağaç `uniffi` ile mobile gidiyor. `0.10` seçildi, `0.11` değil: Tauri
kabuğu zaten `0.10.9`'u kilitliyor, workspace'te ikinci kopya olmuyor.

Feature kapısının arkasına **konmadı.** Kapalı bir kapı "karma tutmuyor"
tanısını koyamamak demekti, ve o tanının susması doğrulamanın var olma
sebebini yok ederdi.

Elle SHA-256 yazmak da masadaydı (~90 satır, NIST vektörleriyle kilitlenir,
sıfır bağımlılık) ve reddedilmedi — kullanıcı bakımı başkasında olan crate'i
seçti.

### Ölçülen sonuç

Gerçek koşum: motor yt-dlp 2026.08.19'u (3.072.469 bayt) indirdi, karmasını
yayımlanan `SHA2-256SUMS` ile birebir doğruladı, çalıştırma biti verdi, ve
`ytmusic` onunla canlı YouTube Music'ten ses çaldı. Dört tanının dördü de
elle kışkırtılıp doğrulandı (404 → yetim, 503 → ulaşılamadı, bozuk dosya →
karma tutmuyor, yanlış sha → yerine konmadı).

**450 test geçiyor**, 7'si kendini atlıyor ve sebebini yazıyor. Üç kapı ve
`core-features`'ın altı birleşimi temiz.

### Yapılmayan (bilerek)

**`torrent` çekirdeğe taşınmadı** (D-050 S3). Bir Rust ikilisi olduğu için
motordan geçemez ve kullanıcıya hâlâ `cargo build --release` yaptırıyor —
D-049'u ihlal eden tek şey artık bu. Taşıma kendi başına bir tur:
`librqbit`'in +179 crate'i, `torrent = ["dep:librqbit"]` kapısı,
`crates/headshell-plugin-torrent`'ın sökülmesi. PLAN §2.8'in 5. maddesi açık.

---

## D-056 — Torrent çekirdeğe taşınmıyor: D-050 S3 iptal
**Tarih:** 2026-09-19 · **Durum:** UYGULANDI (2026-09-19)

**Soru:** Kullanıcı D-055'in hemen ardından: *"torrent'in çekirdeğe girme
mevzusunu kaldıralım. hatta direkt torrent kısmı çok yazılmamış ise şimdilik
`//TODO:AFTER FIRST RELEASE`'de kalsın."*

**Karar: D-050 S3 iptal. Torrent eklenti olarak kalıyor; D-047 yürürlükte.**

### Şartlı kısım ölçüldü ve tutmadı

Kullanıcının ikinci cümlesi bir şarta bağlıydı — "çok yazılmamış ise". Ölçüm:

| | satır |
|---|---|
| `crates/headshell-plugin-torrent/src/` | 2.335 |
| testleri | 647 |
| test sayısı | 56 |

Yazılmamış değil: D-047 §2.4'te canlı Torznab'a karşı arayan, `librqbit` ile
sıralı indirip `127.0.0.1` üzerinden akıtan, çalışan bir eklenti. Yani
**silinecek bir şey yok** ve şart karşılanmıyor. Bu yüzden ilk cümle
(koşulsuz iptal) uygulandı, ikincisi kodu silmek olarak değil **kalan borcu
ertelemek** olarak uygulandı.

### D-050 S3 neden alınmıştı, neden geri alınıyor

Alınma gerekçesi tutarlılıktı: torrent bir Rust ikilisi, betik değil, eklenti
motorundan (D-055) geçemez — ve bugünkü hâli D-049'u en ağır ihlal eden şey.
Çözüm olarak çekirdeğe `torrent = ["dep:librqbit"]` kapısıyla taşınacaktı.

Geri alma gerekçesi **iptal edilen şeyin bedeli**: D-047'nin ölçtüğü
mimari karşılığını vermişti (`headshell-core` ağacı 77'de kaldı, mobil temiz),
eklenti çalışıyordu, ve taşıma 3 bin satırı sökmek demekti. İlk sürümden
önce çalışan bir mimariyi bir *tutarlılık* uğruna sökmenin karşılığı yok.

**D-049 ihlali bu kararla çözülmedi, ertelendi.** Bunu saklamıyoruz: torrent
hâlâ kullanıcıya `cargo build --release` yaptırıyor ve D-055'ten sonra kuralı
ihlal eden **tek** eklenti bu.

### Açık kalan soru artık mimari değil, dağıtım

Taşıma iptal olunca geriye D-049'un asıl sorusu kalıyor: bir Rust ikilisi
kullanıcıya derleyici kurdurtmadan nasıl ulaşır? İki yol var ve ikisi de
seçilmedi — platform başına önceden derlenmiş yayın çıktısı (sürüm, imza ve
CI işi getirir), ya da "kaynaktan derle"nin kalması (bugünkü hâl).

**`TODO: AFTER FIRST RELEASE`.** İşaret üç yerde: `plugin/lib.rs`'in modül
başlığı, `plugins/torrent/README.md`'nin en üstü, PLAN §2.8 madde 5.

Bu, depoya bilerek konan **ilk** `TODO`. D-054 "kodda tek bir
`TODO`/`FIXME`/`unimplemented!` yoktu" diye ölçmüştü ve bu tercih sürüyor:
işaret bir *eksik uygulamayı* değil, **ertelenmiş bir kararı** gösteriyor ve
yanında neyin neden ertelendiği yazılı.

---

## D-057 — Masaüstü kabuğu: paketleme açıldı, Faz 2 yüzeyi arayüze geldi
**Tarih:** 2026-09-19 · **Durum:** UYGULANDI (2026-09-19)

**Soru:** Kullanıcı: *"masaüstü arayüzüne bir el atar mısın? deploya hazır
olsun, biraz daha kullanıcı dostu olsun."* — Üç şey belirsizdi: kapsam (yalnız
cila mı, IPC yüzeyi de mi), dosya diyaloğu için bağımlılık, ve paketleme
kimliğinin sabitlenmesi.

**Karar (üçü de kullanıcının cevabı):**

1. **Kapsam: cila + paketleme + Faz 2 yüzeyi.** Arayüz eklenti ve sır
   yönetimini kazanıyor, `sleeve` kartı görünür oluyor.
2. **`tauri-plugin-dialog` eklendi** — kabuğa ait bir bağımlılık, `headshell-core`
   ağacına girmiyor.
3. **`tune` / `dev.tune.desktop` sabitlendi.** PLAN §1'in "proje adı açık"
   satırı duruyor; sabitlenen paketleme kimliği.
   *(Bu madde kasten eski adıyla duruyor: ad **D-058** ile `headshell` oldu.
   Defterin geri kalanındaki `tune-core` gibi yollar yeni ada göre
   yenilendi, ama adın kendisinden söz eden bir karar, verildiği günkü adla
   okunmazsa D-058 anlamsız kalır.)*

### Arayüz Faz 2'yi hiç görmemişti

Faz 3 yazıldığında Faz 2 ertelenmişti (D-027). Sıra sonradan tersine döndü ve
kimse geri dönüp kabuğa bakmadı: SoundCloud ve YouTube Music eklentileri
çalışıyordu ama **arayüzden kurulamıyor, onaylanamıyordu**. Bir sır (AcoustID
anahtarı, Torznab jetonu) girmenin tek yolu CLI'ydi. Masaüstü kullanıcısı için
bu, özelliğin olmaması demekti.

Aynı boşluğun ikinci örneği daha sessizdi: `sleeve` komutu IPC'de **kayıtlı**
olduğu hâlde `app.js` onu hiç çağırmıyordu. Faz 0.5'in bütün ürünü — projenin
dağıtım kancası — masaüstünde görünmüyordu ve hiçbir test bunu söylemiyordu.

### Eklenti durumu: seçmeyen kopya kaymaz

CLI `status_text()` ile üç şey arasında öncelik seçiyor: manifest sorunu,
motorun kurması gereken eser (D-055), onay durumu (D-040). Tek satıra sığmak
zorunda olduğu için seçiyor.

Arayüzde o sıkışıklık yok, o yüzden **seçim mantığı kopyalanmadı**: üçü de
yan yana gösteriliyor. Kopyalanan mantık zamanla kayar; kopyalanmayan kayamaz.
`--headshell-*` token'ları ile aynı ilke (D-033'ün çapa formülü istisnası bilerek
tektir ve doğruluk kümesiyle kilitli).

### `bundle.active: false` üç şeyi gizlemişti

Paketleme kapalıyken hiçbiri görünmüyordu:

* İkonlar 32×32, **103 baytlık** tek renkli bir yer tutucuydu.
* `.desktop` girdisinin kategorisi, açıklaması, lisans dosyası yoktu.
  *(Düzeltme, 2026-09-20: ilk ikisi kapandı, **lisans kapanmamıştı.**
  `bundle.licenseFile` yazılıydı ama Tauri onu yalnızca Windows/macOS
  paketleyicilerine veriyor; üretilen `.deb` ve `.rpm` hiçbir lisans dosyası
  taşımıyordu — Debian politikasının istediği `/usr/share/doc/<paket>/copyright`
  dahil. `bundle.linux.{deb,rpm}.files` ile üçü birden eklendi: `copyright`,
  `LICENSE-MIT`, `LICENSE-APACHE`. Paketin içi açılıp doğrulandı.)*
* Sürüm iki yerde yazılıydı — `tauri.conf.json` `0.0.1` derken workspace
  `0.0.1-beta`'daydı. `version` alanı **kaldırıldı**; Tauri onu `Cargo.toml`'dan
  okuyor, kaynak tek.

İkon artık `icon.svg`'den üretiliyor (`icons/README.md`) ve varsayılan temanın
paletini kullanıyor — ama temanın parçası değil, kullanıcı tema değiştirince
değişmiyor.

Paketleme `release.yml` ile iki tetikte koşuyor: `v*` etiketi (taslak sürüme
yükler) ve elle çalıştırma. PR'da koşmuyor — on dakikalık bir iş, üç kapı
zaten her PR'da.

Yerel koşum `.deb` ve `.rpm` üretti; içinde ikonlar, `AudioVideo;Audio;Music;`
kategorili `.desktop` girdisi ve doğru sürüm var. **AppImage düştü** ve sebebi
yapılandırma değil host: linuxdeploy kendi eski `strip`'iyle geliyor ve Arch'ın
`.relr.dyn` bölümlü kütüphanelerini tanımıyor. AppImage bu yüzden CI'da ayrı
bir adım — maskelenmiyor, düşerse iş kırmızı yanıyor; ayrım yalnızca bir
AppImage arızasının deb/rpm'i de götürmesini engelliyor. Ubuntu runner'da
sorun beklenmiyor ama **doğrulanmadı**.

### Arayüzün sessiz kırılması artık testli

Webview'de tip denetimi yok: `$("playQuery")` yazım hatası `null` döndürür,
açılışta patlar ve **pencere boş kalır** — `cargo test` bunu görmezdi.
`tests/ui_contract.rs` dört bağı tutuyor: aranan her `id` sayfada var, kenar
çubuğu ile `Ctrl`+sayı kısayolu aynı panelleri adlandırıyor, D-037/2'nin sınıf
adları ulaşılabilir, ilan edilen her token kullanılıyor.

Test yazılırken ilk hâli `.panel`'de düştü: sınıf `style.css`'te hiçbir kural
taşımıyor ama DOM'da duruyor ve bir tema onu hedefleyebiliyor. Ölçüt "CSS'te
kuralı var mı" değil, **ulaşılabilir mi** olarak düzeltildi.

### Bir hatayı yalnızca ekran görüntüsü yakaladı

Kısayol penceresi her açılışta **açık** geliyordu. İşaretleme doğruydu
(`<div id="helpSheet" class="sheet" hidden>`), JavaScript doğruydu, komutlar
doğruydu: `.sheet { display: flex }` UA stylesheet'in `[hidden] { display:
none }` kuralından daha özgül olduğu için `hidden` özniteliği hiçbir şey
yapmıyordu. Aynı kusur `importSummary`'de de vardı (`.summary` da `flex`).

Ne derleyici, ne clippy, ne de IPC'ye bakan bir test bunu görebilirdi — hata
CSS özgüllüğündeydi. Uygulamayı gerçekten açıp bakmak yakaladı.

Düzeltme tek kural (`[hidden] { display: none !important }`) ve yanında bir
regresyon testi. `!important` bilinçli: gizlenmiş bir öğeyi bir tema geri
getirememeli.

### Kullanıcı dostuluğun karşılığı

* Boş kuyruk artık boş bir liste değil, üç adımlı bir **başlarken** kartı.
* İçe aktarma "tanı" sekmesinden çıkıp kendi sekmesine taşındı — ilk
  yapılacak iş en tanısal sekmenin altında duruyordu. Raporu da ham JSON değil
  sayılarla özet; ham hâli katlanmış duruyor.
* Klavye: boşluk, ok tuşları, `/`, `Ctrl`+1…8, `Esc`, `?`. Keşfedilmeyen
  kısayol yok sayıldığı için `?` bir liste açıyor.
* Uyarılar kapatılabiliyor; tanı raporu panoya kopyalanabiliyor.
* Boş durumlar (sağlayıcı yok, sunucu yok, sır yok, dinleme yok) artık ne
  yapılacağını yazıyor — "bakmadım" ile "bulamadım" ayrımı arayüzde de geçerli.

---

## D-058 — Proje adı: `tune` yer tutucusu `tonearm` oldu
**Tarih:** 2026-09-20 · **Durum:** UYGULANDI (2026-09-20)

**Soru:** İlk sürüm hattı açıldı (kullanıcı: *"ilk sürüm hattı"*). Etiket
atılması adı fiilen sabitler — indirilen paketin adı, `.desktop` girdisi, veri
dizini, depo adresi. PLAN §1 ise ilk günden beri *"Hâlâ açık: proje adı
(`tune` yer tutucu)"* diyordu. Ad şimdi mi kapanacak, yoksa 1.0'a mı kalacak?

**Karar:** Şimdi. Ad **`tonearm`**, GitHub deposu da `enaimami/tonearm`.

### Neden `tonearm`

Pikap kolu plağı seçmez — ne koyarsan onu okur. Projenin tek cümlelik tezi
zaten bu: ses nereden gelirse gelsin (yerel dosya, Subsonic, SoundCloud,
YouTube Music, torrent) üstteki katman aynı kalır. Ad, mimariyi anlatıyor.
Türkiye'de pikap meraklısının zaten "tonarm" demesi ikinci bir kazanç.

Müsaitlik ölçüldü, hatırlanmadı: `crates.io` boş, GitHub'daki en büyük
çakışma 2★ bir vinil gürültü simülasyonu, `enaimami/tonearm` boş. Elenen
adaylar ve sebepleri: `earmark` (895★ Elixir markdown), `phono` (2743★
Phonograph — aynı alan, karışır), `deepcut` (428★ Thai tokenizer), `stylus` /
`cadence` / `groove` / `motif` (crates.io dolu ya da yerleşik ses yazılımı
adı), `sidetrack` (hem 62★ kütüphane hem olumsuz çağrışım). Finale kalan
`refrain`, `longplay` ve `dubplate` ölçümde temizdi; seçim metafora göre
yapıldı.

### Neden bugün, neden dün değil

Ad değişimi 110 dosyaya ve ~950 satıra dokundu. Bunların bir kısmı **sözleşme**:

* tema token'ları `--tune-*` → `--tonearm-*` (D-037'nin 14 token'ı),
* tanı raporunun JSON anahtarı `tune_version` → `tonearm_version`,
* ortam değişkenleri `TUNE_*` → `TONEARM_*` (11 tane),
* paketleme kimliği `dev.tune.desktop` → `dev.tonearm.desktop`,
* veri dizini `~/.local/share/tune` → `~/.local/share/tonearm`.

Hiçbiri yayınlanmamıştı. Bir etiket atıldıktan **sonra** aynı değişiklik,
kurulu her kullanıcının kütüphanesini sahipsiz bırakır ve yazılmış her temayı
kırar. Bedeli sıfır olduğu tek an buydu — D-057'nin paketlemeyi açmış olması
bu anı başlattı ve etiket onu kapatacaktı.

Geliştirme makinesindeki veri dizini elle taşındı (kütüphane, sırlar, eklenti
kayıtları korundu); `tonearm stats` taşımadan sonra aynı sayıları veriyor.

### Deftere dokunma ölçüsü

Bu dosyadaki eski kayıtlarda `tune-core` gibi **yollar ve tanımlayıcılar**
yeni ada göre yenilendi: karar defteri okunmak için var, olmayan bir dizini
gösteren kayıt okunamaz.

Tek istisna **D-057'nin 3. maddesi**, çünkü o madde adın kendisi hakkında bir
karardır (*"`tune` / `dev.tune.desktop` sabitlendi"*). Verildiği günkü adla
okunmazsa bu karar anlamsız kalır. Orada eski ad duruyor ve yanında bir not
var. Ayrım şu: *bir adı kullanan* kayıt yenilenir, *bir ad hakkında olan*
kayıt dondurulur.

### Yanında kapanan borç

MusicBrainz istemci kimliği `https://github.com/kullanici-adi/tune` yer
tutucusunu gönderiyordu — D-054 bunu işaretlemiş, kimse kapatmamıştı. Artık
gerçek depo adresini gönderiyor. MB'nin kuralı ulaşılabilir bir iletişim
adresi istiyor; e-posta yerine depo seçildi, çünkü e-posta ikilinin içinde
açık metin olarak dağıtılır.

**455 test, üç kapı temiz.** Ad değişimi davranış değiştirmedi: testler
yeniden adlandırmadan önce ve sonra aynı sayıda geçti.

> **Bu kayıt donduruldu (D-065).** Ad sonradan `headshell` oldu ve bu kaydın
> içindeki `tonearm` geçişleri **yenilenmedi** — çünkü bu kayıt bir adı
> *kullanmıyor*, bir ad *hakkında*. Yeni ada göre yazılsaydı "ad `tonearm`
> seçildi" cümlesi anlamsızlaşırdı. Kuralın kendisi bu kaydın kendi içinde:
> *bir adı kullanan kayıt yenilenir, bir ad hakkında olan kayıt dondurulur.*
>
> Buradaki müsaitlik ölçümünün eksik olduğu da D-065'te yazılı: crates.io ve
> GitHub bakılmış, **AUR ve Codeberg bakılmamıştı.**

## D-059 — `--tui` terminali ses aygıtından önce sınar; CI'nın hiç yeşil olmadığı böyle görüldü
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** `headshell play --tui`, terminali olmayan bir ortamda hangi hatayı
vermeli? Test (`the_tui_refuses_to_start_without_a_terminal`) terminal reddini
bekliyordu; CI ses kartı hatası alıyordu ve düşüyordu.

**Karar:** Sıra çevrildi. `--tui` yolunda **önce** terminal sınanır
(`tui::require_terminal`), sonra ses çıkışı açılır.

### Neden bu sıra

`--tui` iki kaynak ister: bir terminal ve bir ses çıkışı. Terminal bedava
sınanır, ses çıkışı bir donanım kaynağı açar. Ters sırada, ses kartı olmayan
bir makinede kullanıcı `--tui` yazdığı hâlde ALSA hatası görüyordu — yanlış
tanı. K9'un istediği, hatanın kullanıcının yaptığı şeyi anlatmasıdır.

Sınama `enable_raw_mode`'un kendisiyle yapılıyor, ayrı bir `is_terminal`
ölçütüyle değil: iki ölçüt birbirinden kayarsa "sınamada geçti, açarken
düştü" doğar. Ham kip hemen geri veriliyor, ekran değiştirilmiyor — bu yüzden
yavaş bir sağlayıcı aramasının çıktısı alternatif ekranda kaybolmuyor.

### Asıl bulgu: CI hiç yeşil olmamıştı

D-053 CI'yı kurdu. O günden beri **iki koşum** oldu (`a2cf5b6`, `c84d75a`) ve
**ikisi de kırmızı**. Commit mesajları "455 test, üç kapı temiz" diyordu;
ölçüm geliştirme makinesinde yapılmıştı ve orada ses kartı var. Bu tek test,
ses kartı olan her makinede geçiyor, olmayan her makinede düşüyordu.

Ders, kuralın kendisinden değil ölçüldüğü yerden geliyor: "üç kapı temiz"
iddiası, kapıların **nerede** koşturulduğu söylenmeden eksiktir.

### Nasıl doğrulandı

Geliştirme makinesinde ses kartı olduğu için arıza doğrudan üretilemiyordu.
`ALSA_CONFIG_PATH=/dev/null` runner'ın ses kartsızlığını taklit ediyor ve
CI'nın çıktısını birebir veriyor. İki yönde de ölçüldü:

| Koşul | Düzeltmesiz | Düzeltmeli |
|---|---|---|
| Ses kartı var | ok | ok |
| Ses kartı yok (CI'nın hâli) | FAILED | ok |

Yan kazanç: test CI'da **artık gerçekten koşuyor**. Eskiden ses aygıtına
takılıp terminal reddine hiç varamıyordu — yani iddiasını hiç sınamıyordu.

### Yanında açılan borç

`playback_local::a_real_file_plays_and_the_position_advances` bu makinede
kararsız (6 koşumda 3 düşüş): `snd_pcm_avail_delay → I/O error (5)`. Tek ses
çıkışı HDMI ve boştaki hatta yazmak aralıklı EIO veriyor. `engine_for` iki
durumu atlıyor — aygıt yok, aygıt açılamadı — ama bu üçüncüsü: aygıt açıldı,
sonra altından çekildi. CI'da aygıt hiç olmadığı için orada atlanıyor.
Ayrı bir tur; bu kararın kapsamında değil.

## D-060 — Eklenti motoru: geçici indirme adı koşuma özgü olmalı
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** D-059 CI'nın ilk kırmızısını kapattı ve altından ikincisi çıktı:
`plugin_ytmusic` iki testi düşüyordu. Sebep neydi ve nereye ait?

**Karar:** Ürün yarışı. `Engine::install` geçici dosyayı **koşuma özgü** bir
adla yazıyor: `<eser>.<pid>-<nanosaniye>.indiriliyor`.

### Yarış

İndirme iki adımlıydı — önce `.indiriliyor` geçici dosyası, sonra `rename`
(D-055). Geçici ad sabitti: `<eser>.indiriliyor`. Aynı eseri aynı anda kuran
iki koşum aynı dosyayı yazıyor; biri `rename` ile alıp götürünce öteki
`make_executable`'ın `metadata` çağrısında ENOENT alıyor.

Bu bir test kusuru değil. Gerçek kullanımda da iki `headshell plugin install`
aynı anda koşarsa aynı şey olur. Testte görünmesinin sebebi `plugin_ytmusic`'in
beş testinin paralel koşması ve hepsinin aynı sabit önbellek dizinini
paylaşması — yani testin yaptığı şey gerçekçiydi, kusurlu değil.

`.indiriliyor` **son ek olarak** kalıyor: yarım kalmış indirmeyi tanıyan
denetim ona bakıyor. Ayrıca `make_executable` ve `rename` hata yollarında
geçici dosya artık siliniyor — yarıda kalan bir kurulum ortalıkta dosya
bırakmamalı.

### Neden dört çekirdekte hiç görülmedi

Geliştirme makinesinde temiz önbellekle üç koşumun üçü de geçti; CI'nın iki
çekirdeğinde düştü. Zamanlamaya bağlı bir arıza "yok" demek değildir —
**ölçülmedi** demektir (K9'un ayrımı burada da geçerli).

Regresyon testi bu yüzden zamanlamaya bırakılmadı:
`installing_the_same_artifact_concurrently_does_not_collide` sekiz iş
parçacığıyla aynı eseri aynı dizine kuruyor. Deterministik ölçüldü —
düzeltme geri alındığında üç koşumun **üçü de** düştü, düzeltmeyle geçti.

**456 test, üç kapı temiz** (455 + bu regresyon testi).

### Açık kalan

`plugin_ytmusic`'in ikinci testi CI'da `PROVIDER_CALL` ile düşüyordu. Eser
kurulamadığı için mi, yoksa YouTube CI adreslerini engellediği için mi —
ayrım ancak bu düzeltmeden sonraki koşumda görülür. D-043'e göre ikincisi
meşru bir kırmızıdır ve ayrı bir karar gerektirir.

## D-061 — YouTube bot duvarı: çerez sırdan geçer, çerezsiz koşum atlar
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** D-060'ın açık bıraktığı soru cevaplandı. `plugin_ytmusic`'in akış
çözümü CI'da düşüyordu ve sebep şuydu:

```
yt-dlp: ERROR: [youtube] qYcoJpqCha4: Sign in to confirm you're not a bot.
```

YouTube veri merkezi adreslerine bot duvarı çıkarıyor. Bu D-043'ün hangi
tarafı — "ulaşamadım" mı, "ulaşıp beklenmeyeni aldım" mı?

**Karar (kullanıcı):** Çerez verilir. `plugin:ytmusic` ad alanındaki `cookies`
sırrı yt-dlp'ye çerez dosyası olarak geçirilir; CI'da sır
`YTMUSIC_COOKIES` → `HEADSHELL_TEST_YTMUSIC_COOKIES` yolundan gelir.

### Neden sırdan, ortam değişkeninden değil

Eklentiye çerez **sır deposundan** geçiyor (D-042), test de onu oraya yazıp
öyle veriyor. Böylece sınanan yol kullanıcının yaşadığı yolun aynısı:
`headshell secret set plugin:ytmusic cookies`. Testin kendine özel bir arka
kapısı olsaydı, sınanan şey ürün olmazdı.

Eklenti çerezi `0600` bir geçici dosyaya yazıp çıkışta siliyor — yt-dlp
çerezi yalnızca dosyadan okuyabiliyor, ama bir hesap oturumunun diskte
kalıcı kopyası bırakılmıyor.

### Çerezsiz koşum neden atlıyor

Fork'lardan gelen PR'lara GitHub secret vermiyor. Çerez orada hep boş gelir
ve ısrar edilse test kalıcı olarak kırmızı kalırdı — CI'nın kırmızısı o anda
anlamını yitirir, ki tek işi o.

Ayrım şöyle kuruldu:

* **Çerez verilmişse** duvarı aşmak bizim işimizdir; aşamamak **düşme**
  sebebidir. `master`'daki sertlik budur.
* **Çerez verilmemişse** ölçülebilir bir şey yoktur: servis bakmamıza izin
  vermedi, ürün hakkında hiçbir şey söylemedi. Test atlar ve sebebini
  `stderr`'e yazar (D-043).

Eşleşme **dar**: yalnızca bot duvarının kendi imzası (`not a bot`). Başka
her ret hâlâ düşürüyor — yoksa gerçek bir regresyon bu kapının arkasına
saklanırdı. Geniş bir "PROVIDER_CALL hatası varsa atla" kuralı, D-043'ü
uygulamak değil iptal etmek olurdu.

### Bedeli açıkça yazılıyor

Çerez bir hesap oturumudur. Depoya yazma yetkisi olan herkes onu sızdırabilir
ve aynı depodan açılan PR'lar secret görür. yt-dlp'nin kendi belgesi hesabın
sınırlanabileceğini söylüyor. Bu yüzden çerez **atılabilir bir hesaptan**
alınır, kişisel hesaptan değil; ve süresi dolduğunda test yine kırmızı yanar
— o kırmızı "ürün bozuldu" demez, "çerez öldü" der.

### Nereden geldiği

Sebep ancak testin hata mesajı düzeltilince görüldü: test `{err}` yazıyordu
ve `Error`'ın `Display`'i tasarım gereği yalnızca `ADIM: {stage}` basıyor.
Üç aday, üç aşama adı, sıfır sebep — tanıyı yutan bir hata mesajı, üzerine
K9 kurulmuş bir projede. `chain_text()`'e geçince sebep ilk koşumda çıktı.

**456 test, üç kapı temiz.**

## D-062 — Aynı yarış ikinci bir yerde: paylaşılan önbelleğe atomik yerleştirme
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** D-061'den sonraki CI koşumu yine düştü ama **yeni bir hatayla**:
bot duvarı değil, `yt-dlp kurulu değil`. Eser az önce kurulmuştu; neden
kurulu görünmüyor?

**Karar:** `cached_ytdlp` paylaşılan önbelleğe artık atomik yerleştiriyor:
önce koşuma özgü bir `.kuruluyor` adına kopyalıyor, sonra `rename`.

### Zincir

`std::fs::copy` atomik değil. Paralel koşan öteki test, önbellekteki
**yarım yazılmış** dosyayı `cached.exists()` ile görüp hazır sayıyordu; onu
kendi veri dizinine kopyalıyor, motorun karma denetimi tutmuyor, eser
`ready_paths`'e hiç girmiyor ve eklenti "yt-dlp kurulu değil" diyordu.

Hata mesajı doğruydu — eser gerçekten hazır değildi. Yanıltıcı olan, onu
kuran adımın başarılı görünmesiydi.

### Bu, D-060'ın aynısı

D-060 motordaki sabit geçici adı düzeltti. Aynı desenin ikinci bir kopyası
üç fonksiyon yukarıda, test yardımcısında duruyordu ve o tur gözden kaçtı:
düzeltme yazılırken "başka nerede atomik olmayan bir yerleştirme var" diye
bakılmadı. Bir yarış bulunduğunda sorulacak soru "burayı düzelttim mi"
değil, **"aynı deseni paylaşan başka kaç yer var"**.

Kural olarak: paylaşılan bir dizine konan her dosya `rename` ile konur.
`rename` aynı dosya sisteminde atomiktir — dosya ya yoktur ya tamdır; yarım
hâli hiçbir okuyucuya görünmez.

### Doğrulama sınırı — açıkça

Bu yarış geliştirme makinesinde hiç tetiklenmedi (temiz önbellekle üç koşum,
üçü de geçti), yani düzeltme **yerelde kanıtlanmadı.** Kanıt okumada:
`copy` atomik değil, `rename` atomik. D-060'ın regresyon testi gibi
deterministik bir sınama buraya yazılmadı — sınanacak şey testin kendi
yardımcısı ve onu sınayan bir test, sınadığı şeyin altına düşerdi.

Gerçek sınav CI. Bu, D-059/D-060'ın dersini bir kez daha söylüyor:
**"yerelde geçti" bir ölçüm sonucudur, kapsamı ölçüldüğü yer kadardır.**

**456 test, üç kapı temiz** (CI'nın ses aygıtsız koşulunda ölçüldü;
`playback_local` bu makinede HDMI hattındaki EIO yüzünden kararsız, D-059).

## D-063 — MSI sürümü ayrı yazılır, ama kaymasına izin verilmez
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** §3.5'in ikinci borcu kapatılırken — macOS ve Windows paketlemesi
ilk kez elle koşturuldu — Windows düştü:

```
Error failed to bundle project: `optional pre-release identifier in app
version must be numeric-only and cannot be greater than 65535 for msi target`
```

Windows Installer ön-yayın etiketi kabul etmiyor; `0.0.1-beta` MSI hedefinde
geçersiz. Sürüm şeması nasıl değişsin?

**Karar (kullanıcı):** `bundle.windows.wix.version = "0.0.1"`. `Cargo.toml`
`0.0.1-beta` kalıyor, görünür her yerde beta yazısı korunuyor. Kayma bir
testle imkânsız kılınıyor.

### D-057 ile gerilim, ve nasıl kapatıldığı

D-057 sürümün **tek yerde** yaşamasına karar vermişti: `tauri.conf.json`'dan
`version` alanı o gün bilerek kaldırıldı, "iki yerde yazılsaydı biri kayardı"
gerekçesiyle. `wix.version` o ikinci yeri geri getiriyor.

Gerekçe geçerli olduğu için kural iptal edilmedi, **zorunlu kılındı**:
`crates/headshell/tests/bundle_contract.rs` `wix.version`'ın Cargo sürümünün
ön-yayın eki atılmış hâline eşit olduğunu ve üç alanının da Windows'un
sınırları içinde (ilk iki alan ≤255, sonrakiler ≤65535) kaldığını denetliyor.
İki yerde yazılı, ama ayrışamaz: ayrışırsa kapı kırmızı yanar. Ölçüldü —
`wix.version` kasten `0.0.2` yapıldığında test düştü ve neyin neyle
ayrıştığını yazdı.

Seçenek "`-beta` ekini tamamen düşür" idi ve reddedildi: paket sürümünün
kendisi beta olduğunu söylemeli, bu bilgi yalnızca README'de kalmamalı.

### Kuru koşum neden işe yaradı

PLAN §3.5 "ilk etiket atılmadan önce `workflow_dispatch` ile elle koşulmalı"
diyordu ve bu tam olarak karşılığını verdi. Etiketle gidilseydi `draft` işi
`needs: bundle` yüzünden atlanırdı (koşumda `skipped` göründü), sürüm hiç
oluşmazdı ve etiketi silip yeniden atmak gerekirdi.

Aynı koşum §3.5'in **birinci** borcunu da kapattı: AppImage Ubuntu runner'da
sorunsuz üretiliyor. Arch'ta linuxdeploy'un eski `strip`'i yüzünden yerelde
denenemiyordu ve "beklenen sorun değil ama doğrulanmadı" diye yazılmıştı —
artık doğrulandı.

### Ailenin beşinci üyesi

D-059, D-060, D-061, D-062 ile aynı kalıp: kural doğruydu, ölçüm o platformda
hiç yapılmamıştı. `bundle.active` uzun süre `false`'tu; D-057 onu açtı ama
Windows hiç denenmedi. `bundle_contract.rs` bu ölçümü paketleme gününden
alıp her kapı koşumuna taşıyor.

**458 test, üç kapı temiz** (456 + iki yeni denetim).

## D-064 — `wrapped` özelliği `sleeve` oldu: marka başkasının
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** Paylaşılabilir yıl kartı özelliğinin adı ilk günden beri `wrapped`'dı
(D-004, Faz 0.5). Kullanıcı ilk sürüm hattında durdurdu: *"wrapped'ın ismini
baştan geçirelim çünkü direkt wrapped olarak bırakmak yasal sorun yaratacak."*

**Karar:** Özelliğin adı **`sleeve`**.

### Neden bir sorun

**Wrapped** Spotify'ın yıl sonu özelliğinin markası ve bu proje aynı alanda —
üstelik Spotify export'unu içe aktarıyor, yani yan yana görülecek. Aynı tuzağın
komşuları da elendi: Apple'ın **Replay**'i, YouTube Music'in **Recap**'i,
YouTube **Rewind**. Güvenli olan, markalaşmış bir ada benzemek değil işi tarif
etmek.

### Neden `sleeve`

Plak kabı: başkasına gösterdiğin, üstünde bilgi yazan şey. Üretilen artefakt
zaten paylaşılmak için var olan bir kart görseli, yani metafor işin kendisini
anlatıyor — ve [[headshell]] ile aynı aileden kalıyor.

### Atıf kuralı (kullanıcı düzeltmesi)

Başka bir şirketin markasına metinde atıf yapılırken **sahibiyle ve işaretiyle**
yazılır: çıplak "Wrapped" değil, **Spotify Wrapped®**. Çıplak marka adı, o adı
kendi özelliğinmiş gibi kullanıyormuş izlenimi verir; sahibiyle yazmak atfı
açık eder. Başkasının ürününe adıyla atıf yapmak serbesttir — sorun onu kendi
özelliğinin adı yapmaktır.

Depoda kalan beş "Wrapped" geçişinin hepsi bu biçimde ve hepsi kasıtlı.

### Körlemesine değiştirmenin yakalanan hatası

167 geçiş vardı ve hepsi bizim değildi. Düz bir bul-değiştir iki yeri
bozdu, ikisi de **Spotify'ın sınırını** anlatan cümlelerdi:

* `sleeve/data.rs` — "sağlayıcının 12 aylık hafızasından ayrıldığı yer",
* `docs/index.html` — karşılaştırma tablosunun **Spotify sütunundaki** satır.

İkisinde de "Sleeve yılda bir kez sunulur" gibi kendi ürününe iftira atan
cümleler doğmuştu. Ders: bir ad değiştirilirken her geçiş üç kategoriye
ayrılır — bizim özelliğimiz, başkasının ürünü, belirsiz. Üçüncüsü açık hale
getirilir, ikincisi dokunulmaz.

### Sözleşme yüzeyi

Değişen şeyler yalnızca iç isimlendirme değil: CLI alt komutu (`headshell
sleeve`), `--json` anahtarları, tanı sayaçları (`sleeve.*`), tanı aşaması
(`ADIM: SLEEVE_RENDER`), masaüstü IPC komutu, HTML id'leri ve CSS sınıfı.
Hiçbiri yayınlanmamıştı — taslak sürüm duruyor, kurulu kullanıcı yok. D-058'in
mantığı burada da geçerli: bedelin sıfır olduğu an bu andı.

Snapshot testi değişimi yakaladı — sayaç anahtarları alfabetik yazıldığı için
`sleeve.*`, `stats.*`'ın önüne geçti. Yeniden üretildi ve farkın yalnızca ad
ve sıra olduğu, tek bir değerin oynamadığı doğrulandı.

**458 test, üç kapı temiz.**

## D-065 — Proje adı `headshell`: D-058'in ölçümü eksikmiş
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** İlk sürüm taslağı hazırdı ve sıradaki iş AUR paketiydi. Ad orada
ölçülünce çıkan şey bir tesadüf değildi: **AUR'da `tonearm` dolu** — 1.5.0-1,
*"Unofficial native GTK4 / Adwaita music streaming client for TIDAL"*
(codeberg.org/dergs/Tonearm), aktif bakımda, `tonearm-git` sürümü de var.

Yani aynı alanda — Linux masaüstü müzik istemcisi — aynı ad, aynı dağıtım
kanalı.

**Karar:** Ad **`headshell`**. GitHub organizasyonu `headshell`.

### D-058 neden kaçırdı

D-058 müsaitliği *"hatırlanmadı, ölçüldü"* diye yazmıştı ve haklıydı — ama
**iki kanal ölçülmüştü**: crates.io ve GitHub. AUR ve Codeberg'e bakılmamıştı,
ve çakışma tam olarak oradaydı. D-058'in "en büyük çakışma 2★ bir vinil
gürültü simülasyonu" cümlesi bu yüzden yanlış çıktı.

Ders bu oturumun genel dersiyle aynı (D-059…D-063): **ölçüm, ölçüldüğü
kanalın dışını kapsamaz.** "Ad boş" demek "baktığım yerlerde boş" demektir ve
nerelere bakıldığı yazılmadıkça iddia eksiktir.

Bu tur beş kanal ölçüldü — crates.io, AUR, Codeberg, GitHub depo araması,
GitHub kullanıcı/organizasyon ad alanı — ve 20'den fazla aday tarandı.
`headshell` dördünde tamamen boş; GitHub'daki tek çakışma 1★ bir repo.

### Neden `headshell`

Kolun ucundaki, iğneyi taşıyan sökülebilir kafa parçası. **Hangi iğneyi
takarsan tak, bağlantı aynıdır** — Shure takarsın, Ortofon takarsın, kol da
plak da değişmez.

`tonearm` "plağı seçmem" diyordu; `headshell` aynı fikrin sağlayıcı tarafına
bakan hâli: "her iğne bana takılır". Tez değişmedi, metafor bir adım daha
yerine oturdu. İkonu değiştirmek bile gerekmedi — onu çizerken commit'e
yazılan *"mil, kol, kafa, ve dış oluğa inen bir iğne"* cümlesindeki **kafa**
zaten headshell'in kendisiydi.

### D-058 donduruldu

D-058 bir adı *kullanmıyor*, bir ad *hakkında*. Kendi koyduğu kurala göre
dondurulmuştur: içindeki `tonearm` geçişleri yenilenmedi, çünkü "ad `tonearm`
seçildi" cümlesi yeni ada göre yazılsaydı anlamsızlaşırdı. Yanına eksik
ölçümü söyleyen bir not eklendi.

### Sözleşme yüzeyi

1087 geçiş, 111 dosya. Değişenler arasında sözleşme olanlar: crate adları
(`headshell-core`, `-cli`, `-plugin-torrent`), ikili adları (`headshell`,
`headshell-desktop`), ortam değişkenleri (`HEADSHELL_*`), tema token'ları
(`--headshell-*`, D-037'nin 14 token'ı), tanı JSON anahtarı
(`headshell_version`), paketleme kimliği (`dev.headshell.desktop`), veri
dizini (`~/.local/share/headshell`), MusicBrainz User-Agent ve CSS sınıfı.

Hiçbiri yayınlanmamıştı: taslak sürüm hâlâ taslak, kurulu kullanıcı yok.
D-058'in "bedelin sıfır olduğu tek an" argümanı ikinci kez geçerliydi ve bu
sefer pencere kapanmadan kullanıldı.

Snapshot testi yine yakaladı: `headshell_version` alfabetik olarak `os`'un
önüne geçti, 11 snapshot yeniden üretildi, farkın yalnızca ad ve sıra olduğu
doğrulandı.

**458 test, üç kapı temiz.**

## D-066 — AUR: üç PKGBUILD, dört paket; kaynaktan derleme de var, derlenmiş de
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** İlk sürümün Arch tarafı nasıl dağıtılacak? D-065 tam bu iş
başlarken çıkmıştı (ad AUR'da ölçülünce dolu bulundu) ve paket yarım kaldı.

**Karar:** `packaging/aur/` altında **üç PKGBUILD, dört paket**:

| PKGBUILD | Paket(ler) | Nereden |
|---|---|---|
| `headshell/` | `headshell` + `headshell-cli` | Etiket arşivi, kaynaktan derlenir |
| `headshell-bin/` | `headshell-bin` | Sürümün `.deb`i |
| `headshell-cli-bin/` | `headshell-cli-bin` | Sürümün CLI arşivi |

**Yalnızca kaynak paket split.** İki ikili tek `cargo build`'den çıkıyor;
ayrı PKGBUILD'ler olsaydı ikisini birlikte kuran kullanıcı ~500 crate'i iki
kez derlerdi. `-bin` tarafında paylaşılan bir derleme **yok** — iki paket
hiçbir iş paylaşmıyor, yalnızca dosya kopyalıyor. Split package'in varlık
sebebi paylaşılan derlemedir; olmayan sebebi taklit etmenin iki bedeli vardı:
yalnızca CLI kuran kullanıcı 10 MB'lık `.deb`i de indiriyordu, ve namcap
`splitpkgmakedeps` hatası veriyordu (kural: global `makedepends` alt
paketlerin `depends`'ini kapsamalı — `-bin` tarafında o bağımlılıklar
yalnızca çalışma zamanına ait, derleme için gerekmiyor). Ayrılınca ikisi de
düştü.

### Neden `-bin` de var

Kaynak paket Tauri kabuğunu derliyor; bedel dakikalarla ölçülüyor.
`release.yml` zaten her platformun ikilisini üretiyor ve CLI arşivinin
**dosya adında sürüm yok** — o sabit ad tam olarak bunun için konmuştu,
AUR satırı her sürümde aynı kalsın diye. Yani `-bin` paketi bu depoda
yazılmamış ama ima edilmiş bir karardı; şimdi yazıldı.

`.AppImage` kullanılmıyor: 85 MB ve kendi kütüphanelerini taşıyor. Arch
paketi sistem kütüphanelerine bağlanmalı, o yüzden masaüstü ikilisi
`.deb`in içinden çıkarılıyor.

### `cargo tauri build` kullanılmıyor

O bir bundler, `.deb`/`.rpm`/`.AppImage` üretir. Arch paketini `makepkg`
zaten kuruyor; bundler'ı çağırmak hem işi tekrarlar hem `tauri-cli`'yi yapım
bağımlılığı yapardı. Düz `cargo build` yetiyor, karşılığında `.desktop`
girdisi ve ikonlar elle kuruluyor.

### `.desktop` girdisi depoya taşındı

İki PKGBUILD de `packaging/headshell.desktop`'u kuruyor; `-bin` paketi bunun
için kaynak arşivini de indiriyor (1,2 MB — ikonlar ve lisanslar da oradan).
Elle yazılmış iki kopya olsaydı ayrışırlardı: bu defterde aynı arızanın
D-062 ve D-063'te iki kaydı var.

`StartupWMClass=headshell-desktop` **ölçüldü, uydurulmadı**: taslak sürümdeki
`.deb` açıldı ve Tauri'nin ürettiği girdi okundu — pencere sınıfı ikili
adından türüyor. Yarım kalan taslakta `headshell` yazıyordu; öyle kalsaydı
çalışan pencere başlatıcı ikonuyla eşleşmezdi. İkon adları da aynı sebeple
`headshell-desktop.*`.

### Sürüm tek satırda

`pkgver=0.0.1_beta`, yukarı akış yazımı `_pkgver=${pkgver//_/-}` ile
türetiliyor (Arch sürümünde `-` pkgrel ayıracı). Yarım kalan taslakta sürüm
iki yerde yazılıydı — `pkgver` ve `_srcdir` — ve bu D-063'ün MSI tarafında
yakalanan hatasının aynısıydı.

### Paket metinleri İngilizce

`pkgdesc` ve `.desktop` `Comment`'ı İngilizce. D-036 "metni kullanıcı okur →
Türkçe" diyor ve `.deb`in açıklaması gerçekten Türkçe (tauri.conf.json'dan
geliyor). AUR paketleri bilerek ayrılıyor: orada okuyan kitle uluslararası.
**Bilinen ve kabul edilen tutarsızlık**, kaçırılmış değil.

### Torrent eklentisi pakete girmiyor

Motor eklentileri `~/.local/share/headshell/plugins/<ad>/` altında arıyor ve
`plugin.json`'daki `exec` o dizine göreli; `/usr/bin` altındaki ikiliyi
görmez. Sistem çapında eklenti dizini bir çekirdek kararı — PLAN §2.8 madde 5
ve D-049, ilk sürümden sonra.

### Sıra: önce etiket, sonra PKGBUILD

`-bin` paketi sürüm **taslaktan çıkana kadar çalışmaz**: taslak sürümün
varlıkları anonim indirilemez, yalnızca depoya erişimi olan görür. Kaynak
paketin böyle bir borcu yok, etiket arşivi taslaktan bağımsız.

`.SRCINFO` bu depoda tutulmuyor: AUR deposunda yaşıyor ve orada
`makepkg --printsrcinfo` üretiyor. Buraya bir kopyası konsaydı PKGBUILD
değişince sessizce bayatlardı.

### Paketler gerçekten derlendi — namcap üç bulgu verdi

Bu makine Debian; `makepkg` yok. Paketler bir `archlinux:base-devel`
konteynerinde derlendi (`podman`), ve etiket henüz düzeltmeleri taşımadığı
için `source=` çalışma ağacından üretilen bir etiket-eşi arşivle beslendi.
Yöntem `packaging/aur/README.md`'de yazılı — tekrarlanabilir olması lazımdı,
çünkü namcap'in söyledikleri tahminle bulunamazdı:

1. **`gcc-libs` gereksiz.** Dört pakette de yazılıydı; namcap "included, but
   may not be needed" diyor — `base` üyesi ve örtük olarak zaten karşılanıyor.
   Çıkarıldı.
2. **`-bin` paketleri `-debug` paketi üretiyordu** ve yukarı akıştan gelen
   ikilileri yeniden `strip`'liyordu. Semboller burada değil, derlendikleri
   yerde anlamlı → `options=('!strip' '!debug')`.
3. **`x86_64` düz yazılmıştı.** Mimariye özgü kaynaklar `source_x86_64` /
   `sha256sums_x86_64` dizilerine taşındı, CLI arşivinin adındaki mimari
   `$CARCH`'a çevrildi.

Sonuç: üç PKGBUILD'de de `namcap` temiz. Paketlerde kalan uyarılar bilgi
niteliğinde — "implicitly satisfied" bağımlılıklar (`glib2`, `dbus`, `cairo`,
`libsoup3`, `gdk-pixbuf2`; hepsi `webkit2gtk-4.1` + `gtk3` üzerinden geliyor)
ve `!strip`'in kendi sonucu olan "ELF file is unstripped".

Kaynak paketin tam derlemesi (`makepkg -s`, Tauri + ~500 crate) **koşulmadı**
— başlatıldı ve makineyi meşgul etmemek için durduruldu. PKGBUILD'i
`makepkg --printsrcinfo` ayrıştırdı, `namcap` temiz geçti ve `package_*()`
işlevlerinin dosya yolları sahte bir `$pkgdir`'e koşturularak doğrulandı; ama
`build()`/`check()` gerçek bir Arch makinesinde bir kez koşmalı.

### Yan bulgu: `enaimami/headshell` 404

Paketin `url`'si yazılırken ölçüldü — D-065 depoyu `headshell`
organizasyonuna taşımış, ama `README.md` (iki yer) ve `packaging/copyright`
eski adresi taşımaya devam ediyordu ve o adres yönlendirmiyor, **404
veriyor**. `copyright` dosyası taslak sürümdeki `.deb` ve `.rpm`in içinde
ölü bir adres olarak duruyordu. Üçü de düzeltildi.

### Yan bulgu: PLAN §3.5'in iki borcu zaten kapanmış

"AppImage doğrulanmadı" ve "macOS/Windows elle koşulmadı" borçları, koşumlar
ölçülünce kapanmış çıktı: `workflow_dispatch` koşumu 35571122203 üç
platformda da yeşil, etiket koşumu 35597289680 da öyle, taslak sürümde dokuz
varlık var. PLAN bunu bilmiyordu — metin ölçüme göre güncellendi. Kalan
gerçek eksik: `.dmg` yalnızca arm64.

**458 test, üç kapı temiz.**

## D-067 — `Makefile`: kısayol katmanı, kural katmanı değil
**Tarih:** 2026-09-21 · **Durum:** UYGULANDI (2026-09-21)

**Soru:** Komutlar üç yerde yazılı (CLAUDE.md, CONTRIBUTING.md, ci.yml) ve
elle yazılıyor. Bir `Makefile` bunları tek yerden koşulur yapar mı, yoksa
dördüncü bir doğruluk kaynağı mı olur?

**Karar:** `Makefile` var, ama **kural koymuyor** — mevcut komutları koşuyor.
Üç kapının tanımı PLAN.md §0.4'te, komut yüzeyi CLAUDE.md'de kalıyor;
çelişirse onlar geçerli ve bu Makefile'ın başına yazıldı.

Hedef adları İngilizce, yazı Türkçe (D-036). Sürüm `Cargo.toml`'dan okunuyor,
Makefile'a ikinci kez yazılmıyor — Arch'ın `_` yazımı da ondan türetiliyor
(D-063'ün aynı dersi).

**Ne kazandırıyor:** `make gates` kapıları **ci.yml'nin sırasıyla** koşuyor
(fmt → clippy → test; en ucuz olan en önce düşsün — CLAUDE.md'nin listesi
alfabetikti, CI'nınki kasıtlıydı). `make core-features` ci.yml'nin ikinci
işini yerelde koşturuyor: çekirdek tek başına, her feature tek tek, hepsi
birden (D-054). O iş bugüne kadar yalnızca CI'da koşuyordu.
`make aur-test PKG=…` bir AUR paketini Arch konteynerinde uçtan uca derleyip
`namcap`'liyor.

### `LC_ALL=C` bir süs değil

`make help` hedefleri kendi kaynağından `grep` ile topluyor ve ilk yazımı
sessizce eksik listeliyordu: 16 hedeften 13'ü görünüyordu. Eksik üçünün ortak
yanı — `cli`, `clippy`, `diag` — adında **`i` geçen** tek üç hedef olmaları.

Sebep `[a-zA-Z0-9_-]` aralığı: karakter aralıkları yerelin harmanlama
düzenini kullanıyor ve `tr_TR.UTF-8`'de `i` ile `ı` ayrı harfler, `a-z`
aralığı `i`'yi dışarıda bırakıyor. Kalıp `LC_ALL=C` ile koşuyor artık.

Hata bir süre görünmedi çünkü etkileşimli kabukta `grep` bir kabuk
fonksiyonuna sarılıydı ve 16 satırı da buluyordu; `make`in çağırdığı
`/usr/bin/grep` 13 buluyordu. **Aynı komut iki kabukta iki sonuç verdiğinde
ölçtüğün şey komut değil, ortam.**

Bu projenin geliştiricisi Türkçe yerelde çalışıyor, yani tuzak bir kez daha
kurulabilir: kabuk betiklerinde harf aralığı yazarken ya `LC_ALL=C` ya da
`[[:alnum:]]` kullan.

### Ek: `aur-test` konteynere sabitlenmişti

İlk yazımda hedef doğrudan `podman` çağırıyordu ve bir Arch makinesinde
`podman: Böyle bir dosya ya da dizin yok` ile düşüyordu — oysa orada
konteyner **gereksiz**, `makepkg` zaten var. Konteyner bu Makefile'ın yazıldığı
Debian kutusunun bir ihtiyacıydı ve o ihtiyaç hedefin tanımına sızmıştı.

Artık motor otomatik seçiliyor (`makepkg` varsa yerel, yoksa konteyner) ve
`ENGINE=` ile zorlanabiliyor. Eksik araç varsa hedef ne eksik olduğunu ve
hangi komutun kuracağını söylüyor — K9.

Aynı turda ikinci bir hata: eksik araç mesajları çift tırnak içinde ters
tırnak kullanıyordu, yani `updpkgsums`'u basmak yerine **çalıştırıyorlardı**.

---

## D-068 — Tanıtım sayfası GitHub Pages'ten yayımlanıyor (şimdilik yer tutucu)

**Tarih:** 2026-09-23 · **Durum:** UYGULANDI (2026-09-23)

**Soru:** `docs/index.html` aylardır depoda duruyordu ama hiçbir yerde
yayımlanmıyordu — yani kimse görmüyordu. Bir site için ayrı bir depo, ayrı
bir üretici (SSG) ve ayrı bir dağıtım hattı mı kurulmalı?

**Karar:** Hayır. GitHub Pages **`master` dalının `docs/` klasöründen**
doğrudan yayımlanıyor. Üretici yok, npm yok, ek iş akışı yok — sayfa zaten
tek dosyalık düz HTML ve `crates/headshell/ui` ile aynı hatta duruyor
(bundler yok, orada da yoktu).

**Gerekçe:** Bir yer tutucu için dağıtım hattı kurmak, yer tutucudan pahalı.
`docs/` kaynağı yayımlanan şeyin ta kendisi olduğu sürece sayfa bayatlamaz:
düzeltme aynı commit'te gider, ayrı bir depoya kopyalanmayı beklemez.

**Sonuç:**
- Adres: <https://headshell.github.io/headshell/>
- `docs/` bundan sonra **yayımlanan bir yüzey**. Oraya konan her dosya
  herkese açıktır; `eklenti-yazma.md` de aynı kökten servis ediliyor.
- Aynı turda üç ölü bağlantı düzeltildi: "Kaynak Kod" düğmesi ve iki lisans
  bağlantısı depoya değil hiçbir yere gidiyordu (`href="https://github.com"`,
  `href="LICENSE-MIT"` — ikincisi site kökünden 404).
- Sayfa yer tutucu: sürüm rozeti `v0.0.1-beta` diyor ve indirme bağlantısı
  yok. İlk etiketten sonra yeniden yazılacak.

**Ek — açma işi depoya yazılamadı.** İlk denemede Pages'i bir iş akışı
(`actions/configure-pages`, `enablement: true`) kendisi açsın istendi ki
ayar depodaki bir dosyada yazılı kalsın. Olmadı: hem yerel token hem de
iş akışının `GITHUB_TOKEN`'ı `Create Pages site` çağrısında 403 veriyor
(`Resource not accessible by integration`) — Pages izni ikisinde de yok.

İş akışı geri alındı. Kaynak, depo ayarlarından **bir kez** seçiliyor:
Settings → Pages → *Deploy from a branch* → `master` / `/docs`. Bundan
sonrası kendiliğinden: `docs/`'a giden her commit yayımlanıyor, ne iş
akışı ne CI dakikası harcanıyor.
