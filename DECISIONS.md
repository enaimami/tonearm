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
