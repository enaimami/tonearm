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

## D-004 — Wrapped ve topluluk hedefi, faz sırası
**Tarih:** 2026-08-28
**Karar:** Wrapped açık bir hedef. Aralık sezonu takvimi belirliyor.
**Sonuç:**
- **Yeni Faz 0.5 eklendi:** paylaşılabilir Wrapped kartı üretimi.
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
`tune library search` de `stats` gibi `--min-ms` alıyor ki iki yüzey aynı eşikle
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
*Artı:* kullanıcı sezgisine uyar, Wrapped'da parça sayısı bölünmez.
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

## D-011 — Wrapped kartı rasterizasyonu
**Tarih:** 2026-08-29
**Soru:** `tune wrapped --out kart.png` PNG'yi nasıl üretecek? (PLAN 0.5.3 karar noktası)
**Karar:** **resvg, opsiyonel `render-png` feature'ı arkasında.** Çekirdek her zaman
SVG üretir; PNG dönüşümü yalnızca feature açıkken derlenir. CLI feature'ı açar,
mobil bağlamalar açmaz.
**Gerekçe:** Bağımlılık ağacı küçük kalmalı (K7/mobil); resvg ağacı (tiny-skia,
fontdb, rustybuzz) büyük. Feature kapalıyken ağaç hiç büyümüyor, açıkken bitti
ölçütü (`--out kart.png`) karşılanıyor. SVG her zaman üretiliyor olduğu için
GUI/mobil istemezse PNG üretmeden kartı alabilir.
**Sonuç:**
- `tune-core` → `resvg = { version = "0.48", optional = true }`,
  `[features] render-png = ["dep:resvg"]`.
- `tune-cli` tune-core'yu `render-png` ile açar.
- Feature-gated kod `wrapped/png.rs` içinde; `render_svg` koşulsuz.

**Uygulandı (2026-08-29).** `wrapped::write_card` uzantıya bakıp biçime karar
veriyor: `.svg` koşulsuz çalışır, `.png` feature kapalıysa "bu derlemede yok,
SVG kullanın" diye **açıkça** hata verir — sessizce yanlış biçim yazmaz.
Rasterizasyon `usvg` + `tiny-skia` üzerinden; `usvg::Options::default()` boş
bir fontdb ile geldiği için sistem fontları elle yükleniyor ve sans-serif
somut bir aileye bağlanıyor (yoksa metin hiç çizilmiyordu).

**Ölçülen etki:** ağaç feature kapalıyken **56 crate**, açıkken **114**.
Kararın gerekçesi doğrulandı: 58 crate'lik fark mobil bağlamaların dışında kalıyor.

## D-012 — Wrapped kartı tasarımı
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
bar rayı), `metrics` on ölçü. İkisi de `wrapped/svg.rs` içinde `mod`, dışa
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
alıyor. Snapshot testleri `tune_version`'ı zaten değişken sayıp normalize
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
`tune` üretmeye başlar — geçmiş artık hiçbir sağlayıcıda oluşmaz, ki projenin
asıl iddiası bu.
**Kabul edilen risk:** D-003 gereği geliştiricinin yerel arşivi yok, bu faz
**dogfood edilemez**. Karşılığında telifsiz fixture'larla ve testle doğrulanır;
aralık Wrapped penceresine GUI yetişmeyebilir.
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
**Soru:** Yerel indeks bellekteydi ve her `tune play` çağrısı diski baştan
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
- `tune play` **tarama yapmıyor**; `tune provider scan` bir kez çalışır.
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
**Soru:** `tune-core`'un bağımlılık ağacında bugün HTTP/TLS yok. İlk ağ
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
  `cargo tree -p tune-core --no-default-features [--features F] --prefix none |
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
ve `tune` ikisine de bağlandı:

| Sunucu | Sürüm | Sonuç |
|---|---|---|
| Navidrome (OpenSubsonic) | 0.63.2 | kayıt → doğrulama → arama → **akış** → scrobble |
| Jellyfin | 10.11.11 | kayıt (parola→anahtar) → doğrulama → arama → **akış** → scrobble |

Her iki sunucudan da fixture FLAC'ı gerçekten çalındı ve `tune stats`
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

TUNE_PASSWORD=parola123 tune provider add subsonic \
  --url http://127.0.0.1:14533 --user admin --name nav
tune provider test nav && tune play "Test" --all && tune stats
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
  - `tune provider test` başlığı "ERİŞİLEMİYOR" değil "**KULLANILAMIYOR**":
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
**Soru:** Değişiklikler yalnızca elle `tune provider scan` ile alınıyor.
Dizin izleme (watch) için `notify` crate'i eklensin mi?
**Karar:** **Hayır — bağımlılık eklenmedi.** Yerine `tune provider scan
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
  `tune provider scan` gerekiyor ve komut yardımı bunu söylüyor.
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
  paket düzeyinde `allow` ile **geçersiz kılınamaz**; `tune-core` ve `tune-cli`
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
**Karar:** Aynı depo, aynı workspace, **üç paket**: `tune-core`, `tune-cli`,
`tune` (GUI). Ayrı paket **gibi davranılır** ama ayrı depo olmak zorunda değil.
**Gerekçe:** Ayrılabilirlik bir yapı özelliği, dosya düzeni değil. Önemli olan
şu: `tune-cli` ve `tune`, `tune-core` olmadan **çalışamaz** — bütün baz işlemler
orada (K1). Ayırmak istendiğinde ayrılabilmesi için bugünden bağımlılık yönünün
tek yönlü olması yeterli.
**Sonuç:**
- Bağımlılık yönü: `tune-core` ← `tune-cli`, `tune-core` ← `tune`.
  **`tune-cli` ile `tune` birbirini hiç görmez.** Biri diğerinden bir şey
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
- Komut listesi CLI'nin alt komutlarıyla birebir: `search`, `stats`, `wrapped`,
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
