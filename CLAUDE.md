# CLAUDE.md

Her oturumda okunan **operasyonel özet**. Normatif metin burada değil:
değişmez kurallar, faz planı ve çalışma protokolü [`PLAN.md`](PLAN.md)'de yaşar.
Çelişki olursa **PLAN.md geçerlidir**.

Bu dosya şunların tek sahibidir: workspace ağacı, komutlar, CLI test yüzeyi,
kod konvansiyonları, tanılama pratiği, test düzeni. Bunları PLAN.md tekrar
etmez, buraya işaret eder.

> Proje adı `tonearm` (D-058). Pikap kolu: plağı seçmez, ne koyarsan onu okur.

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

**CLI ince bir kabuktur. Bütün mantık `tonearm-core` içindedir.**

Test: bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.
CLI yalnızca şunları yapar — argüman ayrıştırma, çekirdek çağrısı, çıktı biçimleme, çıkış kodu.

CLI içinde **asla**: iş mantığı, veri dönüşümü, ağ çağrısı, SQL, eşleştirme algoritması.
Bir şeyi CLI'de yazmak istiyorsan önce "bunu GUI de isteyecek mi?" diye sor. Cevap evetse çekirdeğe koy.

Aynısı Tauri kabuğu (`crates/tonearm`) için de geçerlidir: o da bir kabuktur.

> Tam metin ve gerekçe: PLAN.md §2, K1. Burada tekrarlanmasının tek sebebi,
> kod yazarken en sık ihlal edilen kural olması.

---

## Değişmez kurallar — indeks

Tam metin ve gerekçeleri **[`PLAN.md` §2](PLAN.md)**'de. Aşağısı yalnızca hatırlatma
indeksidir; bir kuralı uygulamadan önce oradaki metni oku.

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

Bir kuralı ihlal etmen gerekiyorsa **dur ve sor** — PLAN.md §0.1.

> K7 hakkında sık yapılan hata: kural *lifetime, generic parametre ve closure
> parametresini* yasaklar. `Arc<dyn Trait>` ve `async fn` **serbesttir**
> (D-006 düzeltmesi). Kuralın "trait object yok" diyen ilk yazımı geçersizdir.

---

## Workspace

```
tonearm/
├── Cargo.toml                  # workspace
├── CLAUDE.md                   # bu dosya — operasyonel özet
├── PLAN.md                     # kurallar, faz planı, protokol (normatif)
├── DECISIONS.md                # karar defteri (D-001…)
├── CONTRIBUTING.md             # katkıcı süreci
├── crates/
│   ├── tonearm-core/              # BÜTÜN mantık burada
│   │   ├── src/
│   │   │   ├── import/         # export zip ayrıştırıcıları
│   │   │   ├── identity/       # kanonik çözümleme (+ musicbrainz, acoustid, fuzzy)
│   │   │   ├── stats/          # dinleme istatistikleri
│   │   │   ├── sleeve/         # paylaşılabilir kart (svg + png)
│   │   │   ├── library/        # SQLite + FTS
│   │   │   ├── provider/       # sağlayıcı trait'leri + local + remote/{subsonic,jellyfin}
│   │   │   ├── plugin/         # alt süreç + JSON-RPC eklentiler + motor (runtime)
│   │   │   ├── playback/       # symphonia + cpal
│   │   │   ├── net/            # HTTP trait'i + ureq istemcisi + fake
│   │   │   ├── diag/           # tanılama, aşağıya bak
│   │   │   └── session.rs      # dışa açılan komut yüzeyi (Session)
│   │   ├── examples/           # elle koşulan probe'lar (fingerprint, mb, playback)
│   │   └── tests/              # fixtures/ üzerinden entegrasyon testleri
│   ├── tonearm-cli/               # ince kabuk (ikili adı: `tonearm`)
│   │   └── tests/snapshots/    # --json çıktısının snapshot'ları
│   ├── tonearm/                   # Tauri masaüstü kabuğu (ikili adı: `tonearm-desktop`)
│   │   ├── src/                # main + env + state + core_thread + commands
│   │   ├── ui/                 # düz statik webview — bundler yok, npm yok
│   │   ├── icons/              # icon.svg kaynak, ötekiler üretilir (icons/README.md)
│   │   └── themes/             # iki referans tema (contrast, daylight)
│   └── tonearm-plugin-torrent/    # torrent sağlayıcısı — ayrı ikili, JSON-RPC (D-047)
├── plugins/                    # kurulabilir eklentiler: soundcloud, ytmusic, torrent
├── docs/                       # eklenti yazma rehberi, tanıtım sayfası
├── spike/                      # ATILABILIR prototipler — workspace DIŞI, CI DIŞI
└── fixtures/                   # test verisi: kırpılmış export'lar, doğruluk kümesi
```

`spike/` derlenmez, test edilmez, CI'ya girmez. Eşleştirme sezgilerini önce burada
dene; doğruluk tatmin edici olunca `identity/`'ye porta.

`tonearm-plugin-torrent` neden `crates/` altında ama çekirdeğin dışında: eklentidir
(K5), ama Rust yazılmıştır ve aynı workspace'te derlenir. `tonearm-core`'un
bağımlılık ağacına girmez — D-047.

> Bu düzen **kalıcı.** D-050 S3 bir zamanlar torrent'ı çekirdeğe feature'lı bir
> sağlayıcı olarak taşımayı kararlaştırmıştı; **D-056 o kararı geri aldı.**
> Torrent eklenti olarak kalıyor, yukarıdaki ağaç doğru. Geriye kalan borç
> taşıma değil **dağıtım**: eklenti kullanıcıya `cargo build --release`
> yaptırıyor ve bu D-049'u ihlal ediyor — ilk sürümden sonraya ertelendi.

---

## Komutlar

```bash
cargo run -p tonearm-cli -- <alt-komut>
cargo run -p tonearm            # masaüstü arayüzü
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Bir değişikliği bitmiş saymadan önce üçü de temiz geçmeli: `test`, `clippy`, `fmt`.
Tam "bitti" ölçütü: PLAN.md §0.4.

> `cargo test -p tonearm-core` tek başına koşulduğunda feature'lar birleşmediği için
> workspace koşumunda görünmeyen `dead_code` uyarıları çıkar. Üç kapı **workspace**
> üzerinden geçer; tek crate koşumu bir tanı aracıdır, kapı değil.

---

## CLI test yüzeyi

CLI'nin amacı çekirdeği elle sınamak. Her çekirdek yeteneğinin bir alt komutu olmalı.

```
tonearm import <zip|dizin>                     # export içe aktar
tonearm stats [--year N] [--top N] [--min-ms MS]
tonearm resolve "<sanatçı> - <başlık>" | --file <ses>
tonearm library search <sorgu> [--limit N] [--min-ms MS]
tonearm sleeve [--year N] [--out <dosya>] [--format square|story]
tonearm provider list | test <ad> | scan [--if-stale]
tonearm provider add <tür> --url U --user K [--name AD] [--api-key A] [--verify]
tonearm provider remove <ad> | servers
tonearm plugin list | approve <ad> | install <ad> | disable <ad> | enable <ad> | forget <ad>
tonearm secret list | set <ad-alanı> <anahtar> | remove <ad-alanı> <anahtar>
tonearm play <parça> [--all] [--shuffle] [--dry-run] [--tui]
tonearm diag                                   # son çalıştırmanın tanı raporu
```

Küresel bayraklar: `--json`, `--data-dir <DİZİN>`, `--online`, `-v/-vv`.

Her komut `--json` desteklemeli — hem betiklenebilirlik hem de GUI'nin aynı veriyi
alacağının kanıtı olarak. Masaüstü kabuğunun IPC komutları bu listeyi birebir
yansıtır (D-033); arayüze bir yetenek eklemek, önce burada bir alt komut
olmasını gerektirir. İnsan okunur çıktı ayrı bir biçimlendirme katmanıdır
(`tonearm-cli/src/output.rs`).

`--online` varsayılan **kapalı**: bir export'u içe aktarmak kimseyi sessizce
ağa bağlamaz. Bayrak yokken kimlik zinciri yalnızca yerel halkaları koşar.

---

## Tanılama kültürü

Bu proje bir bash prototipinden doğdu ve orada işe yarayan tek şey **her başarısızlığın
nerede olduğunu söylemesiydi.** Bunu koru (K9):

- Her başarısızlık **hangi aşamada** olduğunu söylemeli (`ADIM: IDENTITY_RESOLVE`).
- `tonearm diag` son çalıştırmanın ortam bilgisi, aşama, hata zinciri ve ilgili sayıları
  tek blokta, kopyalanıp yapıştırılabilir şekilde basmalı.
- Loglama `tracing` ile; `println!` yalnızca CLI'nin kullanıcıya dönük çıktısında.
- Sessiz `unwrap_or_default()` yasak — veri kaybını yutar. Ya hata döndür ya say ve raporla.
- **"Bakmadım" ile "bulamadım" ayrı tanılardır.** Yapılandırılmamış bir sağlayıcı
  boş küme değil, ne yazılacağını söyleyen bir hata döndürür.

Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürsün:
kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.

---

## Kod konvansiyonları

- **İsimlendirme dili (D-036): tanımlayıcılar İngilizce, yazı Türkçe.**
  Fonksiyon, tip, değişken, CSS sınıfı, HTML id, JSON anahtarı, fixture dosya
  adı, tema token'ı — hepsi İngilizce. Yorum, doküman, CLI yardım metni,
  arayüz yazısı ve `ADIM:` çıktısı Türkçe. Ayrım kod dili değil, kimin
  okuduğu: tanımlayıcıyı yabancı bir katkıcı okur, metni kullanıcı.
- `tonearm-core` hataları `thiserror` ile tiplenmiş; `tonearm-cli` `anyhow` kullanabilir.
- Genel API'de `async` — çalışma zamanını çağıran seçsin, çekirdek `#[tokio::main]` kurmasın.
- Yeni bağımlılık eklemeden önce sor. Ağaç küçük kalmalı (mobil binary boyutu).
  Zorunlu değilse opsiyonel bir cargo feature arkasına koy (`render-png`, `audio`,
  `fingerprint`, `http-client`).
- Ağ ve dosya sistemine dokunan her şey trait arkasında olsun ki testler sahte (fake) kullanabilsin.
- Kimlikler tip güvenli: `CanonicalId`, `ProviderTrackId`, `ListenId` ayrı newtype'lar, `String` değil.

---

## Test

- `tonearm-core`: birim testleri + `fixtures/` üzerinden entegrasyon testleri.
- Gerçek export zip'lerini kırpıp fixture yap.
- **Ağa bağlı test yazılabilir (D-043)** ama "ulaşamamak" başarısızlık değildir:
  ağ yoksa test kendini atlar ve sebebini `stderr`'e yazar; ulaşıp beklenmeyeni
  alırsa düşer. Sınır "ağa çıkma" değil, iki başarısızlığı ayırmaktır (K9).
  Atlanan test **geçmiş sayılmaz** — raporlarken "atlandı" de.
- Kimlik çözümlemesi için **etiketli bir doğruluk kümesi** tut (`fixtures/identity/cases.json`).
  Her değişiklikte doğruluk oranını ölç — bu sayı projenin en önemli metriğidir:
  `cargo test -p tonearm-core --test identity_accuracy`
- CLI için: alt komutların `--json` çıktısını snapshot testiyle doğrula.

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
| Eklenti nasıl yazılır | docs/eklenti-yazma.md |
| Tema nasıl yazılır | crates/tonearm/themes/README.md |
| Masaüstü paketleri nasıl üretilir | .github/workflows/release.yml, crates/tonearm/icons/README.md |

**Faz durumunu bu dosyaya yazma.** Tek yerde dursun ki bayatlamasın: PLAN.md'nin
faz başlıkları ve `TAMAM` / `YAPILACAK` işaretleri.
