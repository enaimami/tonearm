# CLAUDE.md

> **Türkçe kopya.** Kanonik metin İngilizcedir: [`CLAUDE.md`](CLAUDE.md).
> Bu kopya 2026-09-25 tarihli hâlidir; İngilizce metinle birlikte güncel
> tutulacağı garanti değildir (D-073).

Her oturumda okunan **operasyonel özet**. Normatif metin burada değil:
değişmez kurallar, faz planı ve çalışma protokolü [`PLAN.tr.md`](PLAN.tr.md)'de yaşar.
Çelişki olursa **PLAN.md geçerlidir**.

Bu dosya şunların tek sahibidir: workspace ağacı, komutlar, CLI test yüzeyi,
kod konvansiyonları, tanılama pratiği, test düzeni. Bunları PLAN.md tekrar
etmez, buraya işaret eder.

> Proje adı `headshell` (D-058). Pikap kolu: plağı seçmez, ne koyarsan onu okur.

---

## Proje nedir

Sağlayıcıdan bağımsız bir müzik dinleme katmanı. Ürün ses değil — **dinleme kimliği**:
geçmiş, istatistikler, çalma listeleri ve sosyal bağlar kullanıcıya ait olur, sağlayıcıya değil.
Ses nereden gelirse gelsin (yerel dosya, Subsonic/Jellyfin, SoundCloud, YouTube Music,
torrent, FTP) üstteki katman aynı kalır.

Çekirdek bir Rust kütüphanesidir; onu bir CLI, bir Tauri masaüstü kabuğu ve
(ileride) `uniffi` üzerinden mobil bağlamalar tüketir.

---

## ALTIN KURAL (K1)

**CLI ince bir kabuktur. Bütün mantık `headshell-core` içindedir.**

Test: bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.
CLI yalnızca şunları yapar — argüman ayrıştırma, çekirdek çağrısı, çıktı biçimleme, çıkış kodu.

CLI içinde **asla**: iş mantığı, veri dönüşümü, ağ çağrısı, SQL, eşleştirme algoritması.
Bir şeyi CLI'de yazmak istiyorsan önce "bunu GUI de isteyecek mi?" diye sor. Cevap evetse çekirdeğe koy.

Aynısı Tauri kabuğu (`crates/headshell`) için de geçerlidir: o da bir kabuktur.

> Tam metin ve gerekçe: PLAN.md §2, K1. Burada tekrarlanmasının tek sebebi,
> kod yazarken en sık ihlal edilen kural olması.

---

## Değişmez kurallar — indeks

Tam metin ve gerekçeleri **[`PLAN.tr.md` §2](PLAN.tr.md)**'de. Aşağısı yalnızca hatırlatma
indeksidir; bir kuralı uygulamadan önce oradaki metni oku.

| # | Kural |
|---|---|
| **K1** | Altın Kural: CLI ince kabuktur |
| **K2** | İçe aktarma export dosyalarından yapılır, API'den değil |
| **K3** | Ses asla röle edilmez, yalnızca pozisyon senkronlanır |
| **K4** | Spotify çekirdeğe girmez |
| **K5** | Eklentiler gömülü JS motorunda (QuickJS) koşar; dışarıya yalnızca `host`'tan çıkar |
| **K6** | Kanonik kimlik zinciri sırası: ISRC → MBID → bulanık → AcoustID |
| **K7** | Çekirdek API'si `uniffi` ile ifade edilebilir olmalı |
| **K8** | `headshell-core` içinde `unwrap()` / `expect()` / `panic!()` yok |
| **K9** | Her başarısızlık hangi aşamada olduğunu söyler |
| **K10** | Faz sınırı aşılmaz |

Bir kuralı ihlal etmen gerekiyorsa **dur ve sor** — PLAN.md §0.1.

> K7 hakkında sık yapılan hata: kural *lifetime, generic parametre ve closure
> parametresini* yasaklar. `Arc<dyn Trait>` ve `async fn` **serbesttir**
> (D-006 düzeltmesi). Kuralın "trait object yok" diyen ilk yazımı geçersizdir.

---

## Workspace

```
headshell/
├── Cargo.toml                  # workspace
├── CLAUDE.md                   # bu dosya — operasyonel özet
├── PLAN.md                     # kurallar, faz planı, protokol (normatif)
├── DECISIONS.md                # karar defteri (D-001…)
├── CONTRIBUTING.md             # katkıcı süreci
├── crates/
│   ├── headshell-core/              # BÜTÜN mantık burada
│   │   ├── src/
│   │   │   ├── import/         # export zip ayrıştırıcıları
│   │   │   ├── identity/       # kanonik çözümleme (+ musicbrainz, acoustid, fuzzy)
│   │   │   ├── stats/          # dinleme istatistikleri
│   │   │   ├── sleeve/         # paylaşılabilir kart (svg + png)
│   │   │   ├── library/        # SQLite + FTS
│   │   │   ├── provider/       # sağlayıcı trait'leri + local + remote/{subsonic,jellyfin}
│   │   │   ├── plugin/         # QuickJS motoru (script) + host kapıları + eserler (artifact) + katalog
│   │   │   ├── playback/       # symphonia + cpal
│   │   │   ├── net/            # HTTP trait'i + ureq istemcisi + fake
│   │   │   ├── diag/           # tanılama, aşağıya bak
│   │   │   └── session.rs      # dışa açılan komut yüzeyi (Session)
│   │   ├── examples/           # elle koşulan probe'lar (fingerprint, mb, playback)
│   │   └── tests/              # fixtures/ üzerinden entegrasyon testleri
│   ├── headshell-cli/               # ince kabuk (ikili adı: `headshell`)
│   │   └── tests/snapshots/    # --json çıktısının snapshot'ları
│   └── headshell/                   # Tauri masaüstü kabuğu (ikili adı: `headshell-desktop`)
│       ├── src/                # main + env + state + core_thread + commands
│       ├── ui/                 # düz statik webview — bundler yok, npm yok
│       ├── icons/              # icon.svg kaynak, ötekiler üretilir (icons/README.md)
│       └── themes/             # iki referans tema (contrast, daylight)
├── parked/                     # DERLENMEYEN, silinmemiş kod — torrent (D-069)
├── packaging/                  # dağıtım: copyright, .desktop girdisi, aur/ (D-066)
├── docs/                       # eklenti yazma rehberi, tanıtım sayfası
├── spike/                      # ATILABILIR prototipler — workspace DIŞI, CI DIŞI
└── fixtures/                   # test verisi: kırpılmış export'lar, doğruluk kümesi
```

`spike/` derlenmez, test edilmez, CI'ya girmez. Eşleştirme sezgilerini önce burada
dene; doğruluk tatmin edici olunca `identity/`'ye porta.

**Eklentiler bu depoda değil** (D-071): [`headshell/plugins`](https://github.com/headshell/plugins)
deposunda dizin olarak yaşarlar ve uygulama onları o deponun `index.json`'undan
kurar. İndeksi `headshell plugin index <katalog-deposu>` üretir; elle yazılmaz.
SoundCloud/YouTube Music'in canlı testleri bu depoda kalır ve eklentiyi canlı
katalogdan kurar — katalogdaki bozuk bir sürüm burada kırmızı yanar.

`parked/` workspace'in `exclude`'unda: derlenmez, test edilmez, CI'a girmez.
`spike/`'tan farkı, oradaki kodun atılabilir değil **geri dönmesi beklenen**
kod olması. Bugün tek sakini torrent sağlayıcısı: api 1'in alt süreç
protokolüne yazılmıştı ve eklenti sistemi QuickJS'e geçerken (D-069)
kullanıcının kararıyla taşınmadı. Geri dönüşün açık soruları
`parked/README.md`'de; karar verilmeden workspace'e geri alınmaz.

---

## Komutlar

```bash
cargo run -p headshell-cli -- <alt-komut>
cargo run -p headshell            # masaüstü arayüzü
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Aynılarının kısayolu `Makefile`'da (yalnızca Unix; Windows'ta komutlar doğrudan
koşulur); `make` ya da `make help` hedefleri listeler.
`make gates` üç kapıyı CI sırasıyla koşar (biçim en ucuzu, en önce düşsün),
`make core-features` çekirdeği feature'lar birleşmeden denetler (D-054),
`make cli ARGS="stats --year 2024"` CLI'yi çağırır, `make aur-test PKG=…`
bir AUR paketini Arch konteynerinde derler. Makefile bir kural koymuyor,
yalnızca buradaki komutları tek yerden koşulur hâle getiriyor.

Bir değişikliği bitmiş saymadan önce üçü de temiz geçmeli: `test`, `clippy`, `fmt`.
Tam "bitti" ölçütü: PLAN.md §0.4.

> `cargo test -p headshell-core` tek başına koşulduğunda feature'lar birleşmediği için
> workspace koşumunda görünmeyen `dead_code` uyarıları çıkar. Üç kapı **workspace**
> üzerinden geçer; tek crate koşumu bir tanı aracıdır, kapı değil.

---

## CLI test yüzeyi

CLI'nin amacı çekirdeği elle sınamak. Her çekirdek yeteneğinin bir alt komutu olmalı.

```
headshell import <zip|dizin>                     # export içe aktar
headshell stats [--year N] [--top N] [--min-ms MS]
headshell resolve "<sanatçı> - <başlık>" | --file <ses>
headshell library search <sorgu> [--limit N] [--min-ms MS]
headshell sleeve [--year N] [--out <dosya>] [--format square|story]
headshell provider list | test <ad> | scan [--if-stale]
headshell provider add <tür> --url U --user K [--name AD] [--api-key A] [--verify]
headshell provider remove <ad> | servers
headshell plugin list | approve <ad> | install <ad> | disable <ad> | enable <ad> | forget <ad>
headshell plugin catalog | update [<ad>] | remove <ad>
headshell plugin index <katalog-deposu> [--url-template T] [--check]
headshell secret list | set <ad-alanı> <anahtar> | remove <ad-alanı> <anahtar>
headshell play <parça> [--all] [--shuffle] [--dry-run] [--tui]
headshell diag                                   # son çalıştırmanın tanı raporu
```

Küresel bayraklar: `--json`, `--data-dir <DİZİN>`, `--online`, `-v/-vv`.

Her komut `--json` desteklemeli — hem betiklenebilirlik hem de GUI'nin aynı veriyi
alacağının kanıtı olarak. Masaüstü kabuğunun IPC komutları bu listeyi birebir
yansıtır (D-033); arayüze bir yetenek eklemek, önce burada bir alt komut
olmasını gerektirir. İnsan okunur çıktı ayrı bir biçimlendirme katmanıdır
(`headshell-cli/src/output.rs`).

`--online` varsayılan **kapalı**: bir export'u içe aktarmak kimseyi sessizce
ağa bağlamaz. Bayrak yokken kimlik zinciri yalnızca yerel halkaları koşar.
İndirmenin **komutun kendisi** olduğu yerler (`plugin catalog`, `install`,
`update`) bayrağı beklemez; katalog adresi `HEADSHELL_PLUGIN_INDEX` ile
değişir (D-071).

---

## Tanılama kültürü

Bu proje bir bash prototipinden doğdu ve orada işe yarayan tek şey **her başarısızlığın
nerede olduğunu söylemesiydi.** Bunu koru (K9):

- Her başarısızlık **hangi aşamada** olduğunu söylemeli (`STEP: IDENTITY_RESOLVE`).
- `headshell diag` son çalıştırmanın ortam bilgisi, aşama, hata zinciri ve ilgili sayıları
  tek blokta, kopyalanıp yapıştırılabilir şekilde basmalı.
- Loglama `tracing` ile; `println!` yalnızca CLI'nin kullanıcıya dönük çıktısında.
- Sessiz `unwrap_or_default()` yasak — veri kaybını yutar. Ya hata döndür ya say ve raporla.
- **"Bakmadım" ile "bulamadım" ayrı tanılardır.** Yapılandırılmamış bir sağlayıcı
  boş küme değil, ne yazılacağını söyleyen bir hata döndürür.

Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürsün:
kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.

---

## Kod konvansiyonları

- **Dil (D-073): her şey İngilizce.** Tanımlayıcılar (fonksiyon, tip,
  değişken, CSS sınıfı, HTML id, JSON anahtarı, fixture dosya adı, tema
  token'ı) da yazı da (yorum, doküman, CLI yardım metni, arayüz yazısı,
  `STEP:` çıktısı). D-036 ikisini ayırmıştı — tanımlayıcı İngilizce, yazı
  Türkçe; D-073 yazı dili yarısının yerini aldı. Belgelerin Türkçe kopyaları
  yanlarında `*.tr.md` olarak durur (2026-09-25 tarihli) ve güncel tutulmaz;
  kanonik metin İngilizcedir. Gerçek veri olduğu gibi kalır: fixture'lardaki
  sanatçı ve parça adları, müzik dizini aramasının baktığı `Müzik` klasörü.
- `headshell-core` hataları `thiserror` ile tiplenmiş; `headshell-cli` `anyhow` kullanabilir.
- Genel API'de `async` — çalışma zamanını çağıran seçsin, çekirdek `#[tokio::main]` kurmasın.
- Yeni bağımlılık eklemeden önce sor. Ağaç küçük kalmalı (mobil binary boyutu).
  Zorunlu değilse opsiyonel bir cargo feature arkasına koy (`render-png`, `audio`,
  `fingerprint`, `http-client`, `plugin-engine`).
- Ağ ve dosya sistemine dokunan her şey trait arkasında olsun ki testler sahte (fake) kullanabilsin.
- Kimlikler tip güvenli: `CanonicalId`, `ProviderTrackId`, `ListenId` ayrı newtype'lar, `String` değil.

---

## Test

- `headshell-core`: birim testleri + `fixtures/` üzerinden entegrasyon testleri.
- Gerçek export zip'lerini kırpıp fixture yap.
- **Ağa bağlı test yazılabilir (D-043)** ama "ulaşamamak" başarısızlık değildir:
  ağ yoksa test kendini atlar ve sebebini `stderr`'e yazar; ulaşıp beklenmeyeni
  alırsa düşer. Sınır "ağa çıkma" değil, iki başarısızlığı ayırmaktır (K9).
  Atlanan test **geçmiş sayılmaz** — raporlarken "atlandı" de.
- Kimlik çözümlemesi için **etiketli bir doğruluk kümesi** tut (`fixtures/identity/cases.json`).
  Her değişiklikte doğruluk oranını ölç — bu sayı projenin en önemli metriğidir:
  `cargo test -p headshell-core --test identity_accuracy`
- CLI için: alt komutların `--json` çıktısını snapshot testiyle doğrula.
- **Testler makinede iz bırakmaz (D-070).** Geçici dizin kendini silen bir
  yardımcıyla açılır: çekirdekte `crate::test_support::TempDir`/`TestConfig`,
  entegrasyon testlerinde `tests/support/mod.rs` (kök `target/tmp`). Doğrudan
  `std::env::temp_dir()` altına dizin açma — testler bir zamanlar orada 1,2 GB
  bırakmıştı.
- **Testler dışarıda bir program istemez.** `node`, `python3`, `sh` gibi bir
  çalışma zamanına yaslanan test başka bir makinede ya başarısız olur ya
  sessizce atlanır. JS gerekiyorsa gömülü QuickJS kullanılır (D-070); bir
  işletim sistemine özgü program gerekiyorsa test o sisteme kapılanır
  (`cfg(unix)` / `cfg(windows)`) ve öteki sistemin karşılığı yazılır.
- Yol karşılaştırmasında dize değil `PathBuf` kullan: `/` ve `\` ayırıcısı
  Windows'ta ikisi de geçerli, dize karşılaştırması orada yanlış düşer.

---

## Nerede ne var

| Arıyorsan | Bak |
|---|---|
| Bir kuralın tam metni ve gerekçesi | PLAN.md §2 |
| Ne zaman durup soracağım | PLAN.md §0.1 |
| Hangi fazdayız, sırada ne var | PLAN.md — faz başlıkları ve alt bölüm durumları |
| Bir kararın gerekçesi (D-001…) | DECISIONS.md |
| "Asla yapma" listesi | PLAN.md — ASLA YAPMA |
| Terimler (canonical id, anchor, listen…) | PLAN.md — SÖZLÜK |
| Hangi platform hukuken hangi tarafta | PLAN.md — EK: Yayın platformları |
| Eklenti nasıl yazılır (JS, `host` API'si) | docs/writing-plugins.tr.md |
| Eklenti kataloğu, yeni sürüm yayımlamak | `headshell/plugins` deposunun README'si |
| Park edilmiş kod neden orada | parked/README.md |
| Tema nasıl yazılır | crates/headshell/themes/README.md |
| Masaüstü paketleri nasıl üretilir | .github/workflows/release.yml, crates/headshell/icons/README.md |
| AUR paketi nasıl yayımlanır | packaging/aur/README.md |

**Faz durumunu bu dosyaya yazma.** Tek yerde dursun ki bayatlamasın: PLAN.md'nin
faz başlıkları ve `TAMAM` / `YAPILACAK` işaretleri.
