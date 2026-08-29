# tune

> **Not:** `tune` bir yer tutucu isim. Proje adı henüz kesinleşmedi.

Sağlayıcıdan bağımsız bir müzik dinleme katmanı.

Ürün ses değil — **dinleme kimliği**. Geçmişin, istatistiklerin, çalma listelerin
ve sosyal bağların sana ait olur; sağlayıcıya değil. Ses nereden gelirse gelsin
(yerel dosya, Subsonic/Jellyfin, SoundCloud, torrent) üstteki katman aynı kalır.

<p align="center">
  <img src="docs/ornek-kart.svg" alt="Örnek Wrapped kartı" width="420">
</p>

## Neden

Dinleme geçmişin bir sağlayıcının veritabanında yaşıyor. Aboneliğini bıraktığın
gün on yıllık bir kayıt seninle gelmiyor. Yılda bir gösterilen "Wrapped" ise
yalnızca son 12 ayı biliyor — çünkü daha fazlasını göstermek onun işine yaramıyor.

`tune` bu kaydı geri alır: veri export'unu içe aktarır, parçaları sağlayıcıdan
bağımsız kanonik kimliklere bağlar ve istatistikleri **senin** makinende üretir.

## Durum

**Faz 1.** Bugün çalışan şey: export içe aktarma, kanonik kimlik çözümlemesi,
istatistikler, paylaşılabilir Wrapped kartı ve **yerel dosya oynatma**.

Faz 1'den itibaren scrobble'ı `tune` üretiyor: çaldığın parça import verinle
aynı tabloya yazılıyor, geçmiş ve bugün tek bir zaman çizelgesi oluyor.
Subsonic/Jellyfin ve TUI henüz yok.

Ayrıntılı yol haritası: [`PLAN.md`](PLAN.md). Verilmiş kararlar ve gerekçeleri:
[`DECISIONS.md`](DECISIONS.md).

## Kurulum

Rust 1.85+ gerekiyor (edition 2024).

```bash
git clone <depo-adresi> tune && cd tune
cargo build --release
# ikili: target/release/tune
```

## Kullanım

Verini sağlayıcından iste — Spotify'da *Hesap → Gizlilik → Genişletilmiş
akış geçmişi*. Bu bir **GDPR taşınabilirlik hakkı**; sağlayıcı geliştirici
şartlarıyla kısıtlayamaz. Hazırlanması birkaç gün sürebilir.

```bash
# Export zip'ini (ya da açılmış dizini) içe aktar
tune import my_spotify_data.zip

# İstatistikler
tune stats
tune stats --year 2024 --top 20

# Paylaşılabilir kart
tune wrapped --year 2024 --out kart.png
tune wrapped --format story --out story.png    # 1080×1920

# Tek bir parçayı kimlik zincirinden geçir
tune resolve "Radiohead - Creep"

# Kütüphanede ara
tune library search radiohead

# Bir şey ters gittiyse
tune diag
```

### Oynatma

Müzik dizinini `TUNE_MUSIC_DIRS` ile belirt (`:` ile ayırarak birden çok
verilebilir); verilmezse `XDG_MUSIC_DIR`, sonra `~/Müzik` ve `~/Music` denenir.

```bash
export TUNE_MUSIC_DIRS=~/Müzik

tune provider list          # sağlayıcılar ve yetenekleri
tune provider scan          # dizinleri tara (bir kez; indeks kalıcı)
tune provider test local    # ayakta mı, kaç parça görüyor

tune play "radiohead"              # ilk eşleşmeyi çal
tune play "radiohead" --all        # eşleşenlerin hepsini kuyruğa al
tune play "radiohead" --all --shuffle
tune play "radiohead" --dry-run    # çalmadan kuyruğu göster
```

Tarama indeksi kalıcıdır: `scan` bir kez çalışır, `play` diski taramaz.
Sonraki taramalar artımlıdır — dosyanın damgası değişmediyse etiketleri
yeniden okunmaz. Diskten sildiğin dosya katalogdan düşer ama **dinleme
geçmişi kalır**; geçmiş ayrı bir tabloda ve hiç silinmiyor.

Çalınan her parça bir `listen` kaydı üretir ve `stats` çıktısına girer —
import edilmiş geçmişle aynı tabloda.

Desteklenen biçimler: FLAC, MP3, OGG, M4A/AAC, WAV.

Her komut `--json` destekler:

```bash
tune stats --year 2024 --json | jq '.report.top_artists[0]'
```

## Tasarım kararları

Bunlar tercih değil, projenin şekli:

**İçe aktarma export dosyalarından yapılır, API'den değil.** Sağlayıcı API'si
geçmişi vermez, verse de şartları değiştirebilir. Export hakkı yasal ve kalıcıdır.

**Ses asla röle edilmez.** Birlikte dinleme (Faz 4) her istemcinin *kendi*
kaynağından çaldığı, ağdan yalnızca bir zaman çapasının geçtiği bir tasarım.

**Bütün mantık `tune-core` içinde.** CLI ince bir kabuk: argüman ayrıştırma,
çekirdek çağrısı, çıktı biçimleme. Gelecek GUI ve mobil bağlamalar aynı
çekirdeği çağıracak — kod üç kez yazılmasın diye.

**Sağlayıcı eklentileri alt süreç + JSON-RPC.** Eklenti çökerse çekirdek düşmez
ve eklentiler herhangi bir dilde yazılabilir.

**Her başarısızlık hangi aşamada olduğunu söyler.** `ADIM: IDENTITY_RESOLVE`
gibi. Kısmi başarı üreten her işlem özet döndürür: kaç kayıt geldi, kaçı hangi
yolla çözüldü, kaçı çözülemedi. Sessizce yutulan veri yok.

**Oynatma durumu bir çapadır, bildirim akışı değil.** Çekirdek
`{parça, duvar_saati, pozisyon, hız, durum}` verir; arayüz aradaki zamanı
kendi hesaplar. Aynı primitif ileride odalarda (birlikte dinleme) ağdan
dağıtılacak — iki ayrı durum modeli tutulmuyor.

## Kanonik kimlik

Bir parçayı sağlayıcıdan bağımsız kimliğe bağlama zinciri, bu sırayla:

```
ISRC → MusicBrainz ID → bulanık eşleşme (sanatçı+başlık+süre) → AcoustID parmak izi
```

Her adım bir güven skoru döndürür. Doğruluk `fixtures/identity/cases.json`
içindeki elle etiketlenmiş vaka kümesiyle ölçülür — **69 vaka, 22'si negatif**
(eşleşmemesi gereken). Bu sayı projenin en önemli metriği ve her değişiklikte
ölçülüyor. Küme canlı kayıt, cover, remaster, klasik müzik, transliterasyon ve
aynı adlı farklı sanatçı vakalarını içerir.

## Geliştirme

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Üçü de temiz geçmeden bir değişiklik bitmiş sayılmaz. Ayrıntı:
[`CONTRIBUTING.md`](CONTRIBUTING.md).

## Lisans

MIT veya Apache-2.0 — hangisini istersen.
Bkz. [LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE).
