# CLAUDE.md

> Proje adı `tune` bir yer tutucudur. Değiştirirsen crate adlarını da güncelle.

## Proje nedir

Sağlayıcıdan bağımsız bir müzik dinleme katmanı. Ürün ses değil — **dinleme kimliği**:
geçmiş, istatistikler, çalma listeleri ve sosyal bağlar kullanıcıya ait olur, sağlayıcıya değil.
Ses nereden gelirse gelsin (yerel dosya, Subsonic/Jellyfin, SoundCloud, Tidal, torrent, FTP)
üstteki katman aynı kalır.

Şu anki hedef: **Rust çekirdek kütüphane + onu tüketen bir CLI.**
CLI, GUI gelene kadar tek test yüzeyidir.

---

## ALTIN KURAL

**CLI ince bir kabuktur. Bütün mantık `tune-core` içindedir.**

Test: bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.
CLI yalnızca şunları yapar — argüman ayrıştırma, çekirdek çağrısı, çıktı biçimleme, çıkış kodu.

CLI içinde **asla**: iş mantığı, veri dönüşümü, ağ çağrısı, SQL, eşleştirme algoritması.
Bir şeyi CLI'de yazmak istiyorsan önce "bunu GUI de isteyecek mi?" diye sor. Cevap evetse çekirdeğe koy.

Bu kural, sonradan gelecek Tauri GUI ve `uniffi` üzerinden mobil bağlamaların
sıfır kod tekrarıyla çalışması için var. İhlal edilirse üç kez yazarız.

---

## Değişmez kurallar

Bunlar tartışmaya kapalı; ihlal ediyorsan dur ve sor.

1. **İçe aktarma API'den değil, veri export dosyalarından yapılır.**
   Spotify/Apple/Google export zip'leri (GDPR taşınabilirlik hakkı). Sağlayıcı geliştirici
   şartları bunu kısıtlayamaz. Bu, projenin kırılamayan tek parçası — API'ye kaydırma.

2. **Ses asla röle edilmez, yalnızca pozisyon senkronlanır.**
   Odalar (Faz 3) her istemcinin *kendi* kaynağından çaldığı, sadece zaman çapasının
   dağıtıldığı bir tasarımdır. Sunucudan ses akıtan kod yazma.

3. **Spotify çekirdeğe girmez.** Ayrı, opsiyonel bir eklenti olarak kalır.
   `tune-core`'un bağımlılık ağacında Spotify'a ait hiçbir şey olmamalı.

4. **Sağlayıcı eklentileri alt süreç + JSON-RPC ile konuşur.**
   Dinamik kütüphane değil. Eklenti çökerse çekirdek düşmez, ve eklentiler
   herhangi bir dilde yazılabilir (yt-dlp saran bir Python eklentisi gibi).

5. **Kanonik kimlik zinciri:** ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre)
   → AcoustID parmak izi. Bu sıra bozulmaz; her adım bir güven skoru döndürür.

6. **Çekirdek arayüzü `uniffi` ile ifade edilebilir olmalı.**
   Dışa açılan tiplerde ömür (lifetime) sızıntısı, trait object, kapanış (closure) parametresi olmasın.

---

## Workspace

```
tune/
├── Cargo.toml            # workspace
├── crates/
│   ├── tune-core/        # BÜTÜN mantık burada
│   │   ├── import/       # export zip ayrıştırıcıları
│   │   ├── identity/     # kanonik çözümleme
│   │   ├── stats/        # dinleme istatistikleri
│   │   ├── library/      # SQLite + FTS
│   │   ├── provider/     # sağlayıcı trait'leri + JSON-RPC istemcisi
│   │   ├── playback/     # symphonia + cpal (Faz 1)
│   │   ├── sync/         # çapa protokolü (Faz 3)
│   │   └── diag/         # tanılama, aşağıya bak
│   └── tune-cli/         # ince kabuk
├── spike/                # ATILABILIR prototipler (Python vb.) — workspace DIŞI
└── fixtures/             # test verisi: kırpılmış export zip'leri, örnek JSON
```

`spike/` derlenmez, test edilmez, CI'ya girmez. Eşleştirme sezgilerini önce burada
Python'da dene; doğruluk tatmin edici olunca `identity/`'ye porta.

---

## Komutlar

```bash
cargo run -p tune-cli -- <alt-komut>
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Bir değişikliği bitmiş saymadan önce üçü de temiz geçmeli: `test`, `clippy`, `fmt`.

---

## CLI test yüzeyi

CLI'nin amacı çekirdeği elle sınamak. Her çekirdek yeteneğinin bir alt komutu olmalı.

```
tune import <zip>                    # export içe aktar
tune stats [--year N] [--top N]      # istatistikler
tune resolve "<sanatçı> - <başlık>"  # kimlik çözümlemesini tek parçada dene
tune library search <sorgu>
tune provider list | test <ad>
tune play <parça>                    # Faz 1
tune diag                            # son çalıştırmanın tanı raporu
```

Her komut `--json` desteklemeli — hem betiklenebilirlik hem de GUI'nin aynı veriyi
alacağının kanıtı olarak. İnsan okunur çıktı ayrı bir biçimlendirme katmanıdır.

---

## Tanılama kültürü

Bu proje bir bash prototipinden doğdu ve orada işe yarayan tek şey **her başarısızlığın
nerede olduğunu söylemesiydi.** Bunu koru:

- Her başarısızlık **hangi aşamada** olduğunu söylemeli (`ADIM: IDENTITY_RESOLVE`).
- `tune diag` son çalıştırmanın ortam bilgisi, aşama, hata zinciri ve ilgili sayıları
  tek blokta, kopyalanıp yapıştırılabilir şekilde basmalı.
- Loglama `tracing` ile; `println!` yalnızca CLI'nin kullanıcıya dönük çıktısında.
- Sessiz `unwrap_or_default()` yasak — veri kaybını yutar. Ya hata döndür ya say ve raporla.

Eşleştirme gibi kısmi başarı üreten işlemler **her zaman** özet döndürsün:
kaç kayıt geldi, kaçı ISRC ile, kaçı bulanık, kaçı eşleşmedi.

---

## Kod konvansiyonları

- `tune-core` hataları `thiserror` ile tiplenmiş; `tune-cli` `anyhow` kullanabilir.
- **`tune-core` içinde `unwrap()` / `expect()` / `panic!()` yok.** Testler hariç.
- Genel API'de `async` — çalışma zamanını çağıran seçsin, çekirdek `#[tokio::main]` kurmasın.
- Yeni bağımlılık eklemeden önce sor. Ağaç küçük kalmalı (mobil binary boyutu).
- Ağ ve dosya sistemine dokunan her şey trait arkasında olsun ki testler sahte (fake) kullanabilsin.
- Kimlikler tip güvenli: `CanonicalId`, `ProviderTrackId` ayrı newtype'lar, `String` değil.

---

## Faz planı

Şu an **Faz 0**. Kapsam dışı işe başlamadan önce sor.

| Faz | Kapsam | Backend? |
|-----|--------|----------|
| **0** | export içe aktarma + kanonik kimlik + istatistikler | Hayır |
| 1 | oynatma: yerel dosya, Subsonic/Jellyfin | Hayır |
| 2 | sağlayıcı eklentileri, kimlik çözümlemesi olgunlaşır | Hayır |
| 3 | odalar — çapa tabanlı pozisyon senkronu | Evet, başlar |
| 4 | arkadaşlık grafı (odalardan türer, ayrıca kurulmaz) | Evet |

Tauri GUI + tema sistemi Faz 1'den sonra paralel bir hat olarak açılır.
Mobil (`uniffi`) daha sonra. İkisi de çekirdeği değiştirmeden gelmeli — gelmiyorsa
çekirdek yanlış tasarlanmış demektir.

---

## Test

- `tune-core`: birim testleri + `fixtures/` üzerinden entegrasyon testleri.
- Gerçek export zip'lerini kırpıp fixture yap; ağa bağlı test yazma.
- Kimlik çözümlemesi için **etiketli bir doğruluk kümesi** tut (`fixtures/identity/cases.json`).
  Her değişiklikte doğruluk oranını ölç — bu sayı projenin en önemli metriği.
- CLI için: alt komutların `--json` çıktısını snapshot testiyle doğrula.

---

## Asla yapma

- CLI'ye iş mantığı koyma (Altın Kural).
- Sağlayıcı API'sinden geçmiş/kütüphane çekmeye çalışma — export dosyası kullan.
- Sunucudan ses akıtan bir tasarım önerme.
- **DRM'li bir akışı çözen kod yazma** (Widevine, FairPlay, Deezer'ın Blowfish'i).
  Koruma önlemi aşmak telif ihlalinden ayrı bir kanun maddesidir. Hangi platformun
  hangi tarafta olduğu PLAN.md'nin "EK — Yayın platformları" bölümünde.
- Çekirdeğe Spotify bağımlılığı ekleme.
- Faz 3 gelmeden ağ servisi, hesap sistemi veya sunucu kodu yazma.
- Çekirdek API'sine `uniffi`'nin ifade edemeyeceği tip sızdırma.
- İzinsiz büyük bağımlılık ekleme.

---

## Sözlük

| Terim | Anlamı |
|---|---|
| **canonical id** | Sağlayıcıdan bağımsız parça kimliği (tercihen MBID) |
| **provider** | Ses kaynağı: yerel, Subsonic, SoundCloud, torrent… |
| **plugin** | Alt süreç olarak çalışan, JSON-RPC konuşan sağlayıcı |
| **listen** | Tek bir dinleme olayı (parça + zaman damgası + süre + kaynak) |
| **anchor** | `{track, wall_time, position, rate, state}` — oda senkronunun tek primitifi |
| **resolve** | Bir parçayı kanonik kimliğe, oradan da bir sağlayıcı kimliğine eşleme |
