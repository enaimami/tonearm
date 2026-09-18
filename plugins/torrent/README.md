# torrent eklentisi

Faz 2 §2.4'ün sağlayıcısı. Karar kaydı: **D-047**.

Bu dizin eklentinin *kurulu* hâlidir; kaynağı `crates/tune-plugin-torrent/`.

> **`TODO: AFTER FIRST RELEASE` — kurulum D-049'u ihlal ediyor.**
> Aşağıdaki kurulum size bir **Rust araç zinciri** kurduruyor. D-049 hiçbir
> eklentinin sistem çapında kurulum istememesini şart koşuyor ve D-055'ten
> sonra bunu ihlal eden tek eklenti bu: ötekiler betik, motorun Python'undan
> geçiyorlar; bu bir ikili, geçemiyor.
>
> Bir ara çözüm "çekirdeğe feature kapılı sağlayıcı olarak taşı" idi
> (D-050 S3); **D-056 onu iptal etti** — sökülecek şey 2.335 satır kaynak +
> 647 satır test, çalışan bir eklenti. Açık kalan soru **dağıtım**: platform
> başına önceden derlenmiş yayın çıktısı mı, yoksa "kaynaktan derle" mi
> kalacak. İlk sürümden sonra karara bağlanacak (PLAN §2.8 madde 5).

Eklenti çekirdeğin içinde değil, **alt süreç** olarak çalışır (K5) — sebebi
ölçüldü: `librqbit` `tune-core`'un bağımlılık ağacına 179 crate ekliyordu
(77 → 256) ve o ağaç `uniffi` ile mobile de gidecekti. Bu düzen D-056 ile
**kalıcı** oldu.

## Kurulum

```bash
cargo build --release -p tune-plugin-torrent
mkdir -p ~/.local/share/tune/plugins/torrent
cp plugins/torrent/plugin.json ~/.local/share/tune/plugins/torrent/
cp target/release/tune-plugin-torrent ~/.local/share/tune/plugins/torrent/
tune plugin approve torrent
```

Dizin adı kimliktir (D-037): dizin `torrent` olmalı, `plugin.json`'daki
`name` ile birebir aynı.

## Arama için Torznab gerekiyor

Arama doğrudan indekslere gitmiyor. Prowlarr ya da Jackett'ın konuştuğu
**Torznab** API'sini kullanıyoruz: tek standart, tek ayrıştırıcı, ve depoda
hiçbir siteye özel kazıyıcı yok. Hangi indekslerin sorgulanacağını siz kendi
Prowlarr/Jackett'ınızda seçersiniz; bir site bozulduğunda güncellenmesi
gereken bizim kodumuz değil, onların indeks tanımıdır.

```bash
tune secret set plugin:torrent torznab_url      # ör. http://127.0.0.1:9696/1/api
tune secret set plugin:torrent torznab_api_key
```

Yapılandırılmamışsa `search` **boş sonuç değil, açık bir hata** döndürür:
"bakmadım" ile "bulamadım" ayrı tanılardır (K9). Çalma bu durumda da çalışır —
elinizde bir infohash ya da magnet varsa.

## İki adımlı arama

Torznab bir **yayım** (release) döndürür, bir parça değil — genelde bir albüm.
Protokolün `WireTrack`'i ise bir parça. api 1'i büyütmeden çözüm iki adım:

**Sağlayıcıya doğrudan arama yaptıran bir CLI komutu yok** — `tune provider`
altında `search` diye bir alt komut yoktur. Eklentinin araması `tune play`'in
üzerinden çalışıyor: katalogda sonuç yoksa çekirdek akıtabilen bütün
sağlayıcılara soruyor (`Session::queue_from_search`), torrent de onlardan
biri.

Kimlikleri görmek için `--dry-run --json` gerekiyor; insan okunur çıktı
yalnızca `sanatçı - başlık` basıyor ve bir yayımın infohash'i orada
**görünmüyor**:

```bash
# 1. adım — yayımlar. Kimlik = <infohash>, ve yalnızca JSON'da görünür.
tune play "radiohead ok computer" --dry-run --json | jq -r '.queued[].id.id'

# 2. adım — o yayımın içindeki ses dosyaları. Kimlik = <infohash>/<sıra>
tune play <infohash> --dry-run --json | jq -r '.queued[].id.id'

# 3. adım — çal.
tune play <infohash>/3
```

> Bu iki adımın `--json`'a mahkûm olması eklentinin değil **CLI'nin** eksiği:
> `play --dry-run` sağlayıcı parça kimliğini insan çıktısına yazmıyor. Açık
> borç olarak PLAN.md §2.6'da duruyor.

Bir magnet bağlantısını doğrudan aratabilirsiniz; eklenti onu kataloğuna yazıp
içindeki dosyaları listeler.

Tek ses dosyası olan bir yayımda çıplak `<infohash>` doğrudan çalar. Birden
çok dosya varsa eklenti **tahmin etmez**: dosyaları listeleyen ve ne
yazacağınızı söyleyen bir hata döner — 2. adımı `--json` olmadan da bu hata
üzerinden görebilirsiniz.

## Ses nasıl geliyor

İndirmenin bitmesi beklenmiyor. `librqbit` parça önceliğini okuma konumuna
göre ayarlıyor; eklenti o akışı yalnızca `127.0.0.1`'e bağlı küçük bir HTTP
sunucusundan sunuyor ve `resolve_source` o adresi döndürüyor.

Bu K3'ün yasakladığı şey değil: röle edilen bir veri yok, akış kullanıcının
kendi makinesinde kendi çektiği veriden okunuyor. Adresin yolunda süreç ömrü
kadar yaşayan rastgele bir jeton var — aynı makinedeki başka bir süreç
adresleri deneyerek indirilenleri okuyamasın diye.

## İzin beyanı eksik, ve bilerek eksik

`plugin.json` yalnızca iki DHT giriş noktası beyan ediyor. Bir torrent
istemcisi tanımı gereği **önceden bilinemeyen** tracker'lara ve rastgele peer
adreslerine bağlanır; ayrıca arama için sizin verdiğiniz Torznab adresine
gider. D-040'ın izin sözlüğü ("ana bilgisayar adı listesi, `*` yok") bunu
ifade edemiyor. Beyanı eksik bırakıp `description`'da söylemek, olmayan bir
kısıtlama varmış gibi göstermekten dürüst. Sözlüğün genişletilmesi açık bir
konu (D-047).

## Ayarlar

| Değişken | Ne işe yarar |
|---|---|
| `TUNE_TORRENT_LOG` | `tracing` filtresi (varsayılan `info`). Günlük stderr'e gider, çekirdek onu `tune diag`'a taşır. |

İndirilenler `<eklenti veri dizini>/downloads/<infohash>/` altına, her torrent
kendi dizinine iner (PLAN §2.4: iki yayımın aynı dosya adını taşıması sık, ve
üst üste yazmak sessiz veri kaybıdır).
