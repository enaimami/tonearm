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

Tam "bitti" ölçütü PLAN.md §0.4'te. Üçü CI'da da koşuyor
([`.github/workflows/ci.yml`](.github/workflows/ci.yml), D-053) — ama önce
kendi makinende geçmeli, CI bir hatırlatıcıdır, ilk savunma hattı değil.

### Çalışma zamanı gereksinimi

Eklentiler Python 3.9+ ister ve bu **projenin** gereksinimidir, eklentinin
değil (D-050). Derlemek için gerekli değil; yalnızca eklenti sağlayıcılarını
koşturan testler ve komutlar onu arar. Eklentilerin ihtiyaç duyduğu paketleri
motor indirir — `pip`, `venv` ya da sistem paketi gerekmez.

### Kendini atlayan testler

Ağa bağlı testler (D-043) ulaşamadıklarında **düşmez, kendilerini atlar ve
sebebini `stderr`'e yazar.** Atlanan test geçmiş sayılmaz — rapor ederken
"atlandı" de. Bugün 450 test geçiyor, 7'si kendini atlıyor.

Atlananları gerçekten koşturmak için gereken ortam değişkenleri:

| Değişken | Neyi açar |
|---|---|
| `TONEARM_ACOUSTID_KEY` | AcoustID canlı sınamaları (3 test). Anahtarsız derlemede `EMBEDDED_API_KEY` boş olduğu için atlanırlar. |
| `TONEARM_TORZNAB_URL` + `TONEARM_TORZNAB_KEY` | Torznab canlı sınamaları (3 test). Kendi Prowlarr/Jackett'ınızı ister; `TONEARM_TORZNAB_QUERY` sorguyu değiştirir. |
| `TONEARM_PYTHON` | Motorun kullanacağı Python yorumlayıcısı. Verilirse **geri düşülmez**: o yorumlayıcı çalışmıyorsa motor `python3`'e kaymaz, durur ve söyler. |

`TONEARM_YTDLP` **kaldırıldı** (D-055): yt-dlp'yi artık eklenti aramıyor, motor
kuruyor. `ytmusic` testleri onu manifestteki sabitlenmiş sürümden indiriyor
ve `--features http-client` olmayan bir derlemede kendilerini atlıyor.

Depo kökündeki `.env` **hiçbir kod tarafından okunmaz** — `dotenv` benzeri bir
bağımlılık yok. Oraya yazdığınız değer kendiliğinden ortama girmez; kabuğunuza
siz aktarmalısınız (`set -a; . ./.env; set +a`). Dosya `.gitignore`'da.

Yeni bir yetenek eklediysen ayrıca:

- CLI'de bir alt komutu var ve `--json` destekliyor.
- Kimlik veya içe aktarmaya dokunduysan doğruluk kümesi çalıştırıldı ve
  oran PR açıklamasında yazıyor.

## Altın Kural (K1)

**CLI ince bir kabuktur. Bütün mantık `tonearm-core` içindedir.**

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
| **K5** | Eklentiler alt süreç + JSON-RPC ile konuşur |
| **K6** | Kanonik kimlik zinciri sırası: ISRC → MBID → bulanık → AcoustID |
| **K7** | Çekirdek API'si `uniffi` ile ifade edilebilir olmalı |
| **K8** | `tonearm-core` içinde `unwrap()` / `expect()` / `panic!()` yok |
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
cargo test -p tonearm-core --test identity_accuracy
```

Test sınıf bazında kırılım basar — toplam oran tek bir sınıftaki çöküşü
gizleyebilir. Yeni bir hata sınıfı bulduysan **önce vakayı ekle, testin
düştüğünü gör**, sonra düzelt.

## Lisans

Katkın MIT veya Apache-2.0 (çift lisans) altında yayımlanmayı kabul eder.
