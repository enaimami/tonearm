# Katkı

Proje erken aşamada ve mimari kararlar hâlâ veriliyor. Kod yazmadan önce
[`PLAN.md`](PLAN.md) ve [`DECISIONS.md`](DECISIONS.md) okunmalı — çoğu "neden
böyle yapılmamış?" sorusunun cevabı orada, gerekçesiyle duruyor.

## Önce oku

- [`PLAN.md`](PLAN.md) — yol haritası, değişmez kurallar (§2), faz sınırları,
  ASLA YAPMA, sözlük. **Kurallarda bu dosya geçerlidir.**
- [`CLAUDE.md`](CLAUDE.md) — workspace ağacı, komutlar, kod konvansiyonları,
  CLI test yüzeyi, tanılama pratiği. **Operasyonel bilgide bu dosya geçerlidir.**
- [`DECISIONS.md`](DECISIONS.md) — verilmiş kararlar ve gerekçeleri.
  Aynı soru iki kez tartışılmaz.

## Üç kapı

Bir değişiklik bunlar temiz geçmeden bitmiş sayılmaz:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Üçünü birden: `make gates` (yalnızca Unix; `make` ve `sh` istiyor —
Windows'ta yukarıdaki üç `cargo` komutu doğrudan koşulur).

Tam "bitti" ölçütü PLAN.md §0.4'te. Üçü CI'da da koşuyor
([`.github/workflows/ci.yml`](.github/workflows/ci.yml), D-053): Linux'ta
üçü, Windows ve macOS'ta clippy ile testler (D-070). Ama önce kendi
makinende geçmeli — CI bir hatırlatıcıdır, ilk savunma hattı değil.

### Çalışma zamanı gereksinimi

Yok. Eklenti motoru (QuickJS) çekirdeğe gömülü (D-069); eklentileri koşturan
testler ve komutlar makinede hiçbir yorumlayıcı aramaz. Eklentilerin ihtiyaç
duyduğu araçları (yt-dlp) motor, platformun kendi kendine yeten ikilisi
olarak indirir. Derleme için bir C derleyicisi gerekir — ama SQLite
(`rusqlite` `bundled`) onu zaten istiyordu.

Testler de dışarıda bir şey istemiyor: webview'in çapa formülünü sınayan
test eskiden `node` çalıştırıyordu, artık aynı dosyayı gömülü QuickJS'te
değerlendiriyor (D-070).

### Testler makinede iz bırakmaz

Her test açtığı geçici dizini **siler** — düşen bir test de (`Drop` panikte
koşar). Birim testleri işletim sisteminin geçici dizinini, entegrasyon
testleri Cargo'nun `target/tmp`'sini kullanır; öldürülmüş bir koşumun artığı
bile ortak `/tmp`'ye değil projeye düşer ve `cargo clean` ile gider. Yeni bir
test yazarken `std::env::temp_dir()`'e doğrudan dizin açma: çekirdekte
`crate::test_support::TempDir`, entegrasyon testlerinde
`tests/support/mod.rs` var. Kural bir ölçümden doğdu: testler bir geliştirme
makinesinin tmpfs'i olan `/tmp`'sinde 1.100 dizin, 1,2 GB bırakmıştı.

YouTube Music testlerinin indirdiği yt-dlp (~40 MB) `target/tmp`'de
önbellekte tutulur, ama önbellek körü körüne kullanılmaz: her koşumda karması
ve indirme adresinin hâlâ yaşadığı denetlenir — önbellek, ölmüş bir adresi bu
makinede yeşil göstermesin.

### Windows ve macOS'ta geliştirme

Derleme ve testler aynı `cargo` komutlarıyla koşar. Farklar:

- Veri dizini Windows'ta `%LOCALAPPDATA%\headshell`, macOS'ta
  `~/Library/Application Support/headshell`. Denemeler için
  `HEADSHELL_DATA_DIR` ya da CLI'de `--data-dir` ile ayrı bir dizin verin.
- `HEADSHELL_MUSIC_DIRS` listesi `PATH` gibi yazılır: Windows'ta `;`,
  ötekilerde `:` ile.
- Depo her platformda LF satır sonlarıyla çıkar (`.gitattributes`). Windows'ta
  `core.autocrlf` açık olsa da snapshot'lar bayt bayt tutar.
- Araç çalıştırma testleri Unix'te bir `sh` betiği, Windows'ta sistemin
  `cmd.exe`'sinin bir kopyasını "eser" olarak kuruyor.

### Kendini atlayan testler

Ağa bağlı testler (D-043) ulaşamadıklarında **düşmez, kendilerini atlar ve
sebebini `stderr`'e yazar.** Atlanan test geçmiş sayılmaz — rapor ederken
"atlandı" de. Bugün Linux'ta 447 test koşuyor ve 3'ü kendini atlıyor
(AcoustID anahtarı yok); torrent'in 56 testi eklentiyle birlikte park edildi
(D-069). Testleri bir terminalden koşarsan iki CLI testi daha atlanır: "terminal
yokken ne olur" sorusu, çocuk süreç terminale ulaşabildiği sürece sınanamaz
(D-070 eki). `playback_local`'in ses testi ses aygıtı olan ama yük altındaki bir
makinede aralıklı düşebiliyor (D-059, D-070).

Atlananları gerçekten koşturmak için gereken ortam değişkenleri:

| Değişken | Neyi açar |
|---|---|
| `HEADSHELL_ACOUSTID_KEY` | AcoustID canlı sınamaları (3 test). Anahtarsız derlemede `EMBEDDED_API_KEY` boş olduğu için atlanırlar. |
| `HEADSHELL_TEST_YTMUSIC_COOKIES` | YouTube'un bot duvarını aşmak için çerez (D-061). Yalnızca veri merkezi adreslerinde gerekiyor; ev bağlantısında testler çerezsiz de koşuyor. |
| `HEADSHELL_PLUGIN_INDEX` | SoundCloud ve YouTube Music canlı testlerinin eklentiyi kurduğu katalog (D-071). Varsayılan `headshell/plugins`'in yayımlanmış indeksi; yayımlanmamış bir eklenti değişikliğini sınamak için `headshell plugin index` ile üretilmiş yerel bir aynayı gösterebilir (düz `http` yalnızca `127.0.0.1`'e). |

`HEADSHELL_PYTHON` ve `HEADSHELL_YTDLP` **kaldırıldı** (D-055, D-069): motor
Python aramıyor, yt-dlp'yi eklenti değil motor kuruyor. `ytmusic` testleri
onu manifestteki sabitlenmiş sürümden, bu platformun ikilisi olarak
indiriyor. Eklentilerin kendisi bu depoda değil (D-071): canlı testler onları
kataloğa ulaşamazsa kendini atlıyor, ulaşıp kuramazsa düşüyor. Torznab değişkenleri torrent eklentisiyle birlikte park edildi
(`parked/`, D-069).

Depo kökündeki `.env` **hiçbir kod tarafından okunmaz** — `dotenv` benzeri bir
bağımlılık yok. Oraya yazdığınız değer kendiliğinden ortama girmez; kabuğunuza
siz aktarmalısınız (`set -a; . ./.env; set +a`). Dosya `.gitignore`'da.

Yeni bir yetenek eklediysen ayrıca:

- CLI'de bir alt komutu var ve `--json` destekliyor.
- Kimlik veya içe aktarmaya dokunduysan doğruluk kümesi çalıştırıldı ve
  oran PR açıklamasında yazıyor.

## Altın Kural (K1)

**CLI ince bir kabuktur. Bütün mantık `headshell-core` içindedir.**

Testi şu: bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.
CLI yalnızca argüman ayrıştırır, çekirdeği çağırır, çıktıyı biçimler, çıkış kodu
verir. CLI'de iş mantığı, veri dönüşümü, ağ çağrısı, SQL veya eşleştirme
algoritması **olmaz**. Aynısı Tauri kabuğu için de geçerlidir.

Bir şeyi CLI'de yazmak istiyorsan önce sor: "bunu GUI de isteyecek mi?"
Cevap evetse çekirdeğe koy. Bu kural GUI'nin ve `uniffi` üzerinden mobil
bağlamaların sıfır kod tekrarıyla çalışması için var.

## Değişmez kurallar

Tartışmaya kapalı. İhlal etmen gerektiğini düşünüyorsan kod yazma — bir issue aç
ve neden gerektiğini anlat.

Tam metin ve gerekçeleri **[`PLAN.md` §2](PLAN.md)**'de, K1–K10 olarak numaralı.
Aşağısı yalnızca indekstir; burada tekrarlanmamalarının sebebi, bir zamanlar üç
dosyada birden yazılı olmaları ve kopyaların birbiriyle çelişecek kadar
kaymasıydı.

| # | Kural |
|---|---|
| **K1** | Altın Kural: CLI ince kabuktur |
| **K2** | İçe aktarma export dosyalarından yapılır, API'den değil |
| **K3** | Ses asla röle edilmez, yalnızca pozisyon senkronlanır |
| **K4** | Spotify çekirdeğe girmez |
| **K5** | Eklentiler gömülü JS motorunda koşar; dışarıya yalnızca motorun kapılarından çıkar |
| **K6** | Kanonik kimlik zinciri sırası: ISRC → MBID → bulanık → AcoustID |
| **K7** | Çekirdek API'si `uniffi` ile ifade edilebilir olmalı |
| **K8** | `headshell-core` içinde `unwrap()` / `expect()` / `panic!()` yok |
| **K9** | Her başarısızlık hangi aşamada olduğunu söyler |
| **K10** | Faz sınırı aşılmaz |

Ayrıca **ASLA YAPMA** listesi (DRM, ses rölesi, ham `listen` kaydını silme…)
PLAN.md'nin sonundadır.

## Kod konvansiyonları

Tek sahibi [`CLAUDE.md`](CLAUDE.md), "Kod konvansiyonları" başlığı — hata tipleri,
`async` sözleşmesi, newtype kimlikler, bağımlılık politikası ve isimlendirme dili
(D-036: tanımlayıcılar İngilizce, yazı Türkçe).

Workspace ağacı ve komutlar da orada.

## Tanılama kültürü ve bağımlılıklar

Tek sahibi [`CLAUDE.md`](CLAUDE.md). Özü (K9): her başarısızlık **hangi aşamada**
olduğunu söyler, kısmi başarı üreten her işlem özet döndürür, atlanan kayıt
sayılır ve raporlanır — sessizce düşürülmez.

Yeni bağımlılık eklemeden **önce sor**; ağaç küçük kalmalı (mobil binary boyutu).

## Kimlik doğruluğu

`fixtures/identity/cases.json` elle etiketlenmiş bir doğruluk kümesidir ve
oradaki oran **projenin en önemli metriğidir**. Eşleştirme koduna dokunuyorsan:

```bash
cargo test -p headshell-core --test identity_accuracy
```

Test sınıf bazında kırılım basar — toplam oran tek bir sınıftaki çöküşü
gizleyebilir. Yeni bir hata sınıfı bulduysan **önce vakayı ekle, testin
düştüğünü gör**, sonra düzelt.

## Lisans

Katkın MIT veya Apache-2.0 (çift lisans) altında yayımlanmayı kabul eder.
