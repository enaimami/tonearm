# Katkı

Proje erken aşamada ve mimari kararlar hâlâ veriliyor. Kod yazmadan önce
[`PLAN.md`](PLAN.md) ve [`DECISIONS.md`](DECISIONS.md) okunmalı — çoğu "neden
böyle yapılmamış?" sorusunun cevabı orada, gerekçesiyle duruyor.

## Önce oku

- [`PLAN.md`](PLAN.md) — yol haritası, değişmez kurallar, faz sınırları.
  CLAUDE.md ile çelişirse **PLAN.md geçerlidir**.
- [`DECISIONS.md`](DECISIONS.md) — verilmiş kararlar ve gerekçeleri.
  Aynı soru iki kez tartışılmaz.

## Üç kapı

Bir değişiklik bunlar temiz geçmeden bitmiş sayılmaz:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Yeni bir yetenek eklediysen ayrıca:

- CLI'de bir alt komutu var ve `--json` destekliyor.
- Kimlik veya içe aktarmaya dokunduysan doğruluk kümesi çalıştırıldı ve
  oran PR açıklamasında yazıyor.

## Altın Kural

**CLI ince bir kabuktur. Bütün mantık `tune-core` içindedir.**

Testi şu: bir özellik CLI'den silindiğinde çekirdek onu hâlâ sunabiliyor olmalı.
CLI yalnızca argüman ayrıştırır, çekirdeği çağırır, çıktıyı biçimler, çıkış kodu
verir. CLI'de iş mantığı, veri dönüşümü, ağ çağrısı, SQL veya eşleştirme
algoritması **olmaz**.

Bir şeyi CLI'de yazmak istiyorsan önce sor: "bunu GUI de isteyecek mi?"
Cevap evetse çekirdeğe koy. Bu kural gelecek Tauri GUI'sinin ve `uniffi`
üzerinden mobil bağlamaların sıfır kod tekrarıyla çalışması için var.

## Değişmez kurallar

Bunlar tartışmaya kapalı. İhlal etmen gerektiğini düşünüyorsan kod yazma —
bir issue aç ve neden gerektiğini anlat.

1. **İçe aktarma export dosyalarından yapılır, API'den değil.** Sağlayıcı
   API'sinden geçmiş veya kütüphane çekme.
2. **Ses asla röle edilmez, yalnızca pozisyon senkronlanır.** Sunucudan ses
   akıtan bir tasarım önerme.
3. **Spotify çekirdeğe girmez.** `tune-core`'un bağımlılık ağacında Spotify'a
   ait hiçbir şey olmaz.
4. **Sağlayıcı eklentileri alt süreç + JSON-RPC ile konuşur.** Dinamik
   kütüphane değil.
5. **Kanonik kimlik zinciri sırası bozulmaz:** ISRC → MusicBrainz ID →
   bulanık eşleşme → AcoustID.
6. **Çekirdek API'si `uniffi` ile ifade edilebilir olmalı.** Dışa açık
   imzalarda generic parametre, lifetime veya closure parametresi yok.
   (`Arc<dyn Trait>` ve `async fn` serbest.)
7. **`tune-core` içinde `unwrap()` / `expect()` / `panic!()` yok.** Testler
   hariç. Sessiz `unwrap_or_default()` de yasak — veri kaybını yutar.
8. **Faz sınırı aşılmaz.** Sonraki fazın kodunu "hazır olsun diye" yazma.

## Kod konvansiyonları

- Hata tipleri: `tune-core` → `thiserror`; `tune-cli` → `anyhow` serbest.
- Loglama `tracing` ile. `println!` yalnızca CLI'nin kullanıcıya dönük çıktısında.
- Genel API `async`; çalışma zamanını çağıran seçer, çekirdek `#[tokio::main]` kurmaz.
- Ağ ve dosya sistemine dokunan her şey trait arkasında olsun ki testler
  sahte (fake) kullanabilsin.
- Kimlikler newtype: `CanonicalId`, `ProviderTrackId`. Çıplak `String` değil.
- **Kod ve tanımlayıcılar İngilizce; yorumlar ve dokümanlar Türkçe.**

## Tanılama kültürü

Bu proje bir bash prototipinden doğdu ve orada işe yarayan tek şey her
başarısızlığın *nerede* olduğunu söylemesiydi. Bunu koru:

- Her başarısızlık hangi aşamada olduğunu söyler: `ADIM: IDENTITY_RESOLVE`.
- Kısmi başarı üreten her işlem **özet döndürür**: kaç kayıt geldi, kaçı hangi
  yolla çözüldü, kaçı çözülemedi. Atlanan kayıt sayılır ve raporlanır,
  sessizce düşürülmez.

## Bağımlılıklar

Yeni bağımlılık eklemeden önce sor. Ağaç küçük kalmalı — mobil binary boyutu
buna bağlı. Zorunlu değilse opsiyonel bir cargo feature arkasına koy
(örnek: `render-png`, bkz. D-011).

## Kimlik doğruluğu

`fixtures/identity/cases.json` elle etiketlenmiş bir doğruluk kümesidir ve
oradaki oran **projenin en önemli metriğidir**. Eşleştirme koduna dokunuyorsan:

```bash
cargo test -p tune-core --test identity_accuracy
```

Test sınıf bazında kırılım basar — toplam oran tek bir sınıftaki çöküşü
gizleyebilir. Yeni bir hata sınıfı bulduysan **önce vakayı ekle, testin
düştüğünü gör**, sonra düzelt.

## Lisans

Katkın MIT veya Apache-2.0 (çift lisans) altında yayımlanmayı kabul eder.
