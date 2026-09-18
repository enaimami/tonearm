# Eklenti yazma rehberi (protokol api 1)

`tune` sağlayıcıları **alt süreç** olarak çalıştırır ve onlarla satır bazlı
JSON-RPC 2.0 konuşur. Yani bir eklenti herhangi bir dilde yazılabilir:
stdin'den satır okuyup stdout'a satır yazabilen her şey yeterli.

Üç çalışan örnek:

- [`plugins/soundcloud/main.py`](../plugins/soundcloud/main.py) — **gerçek
  eklenti.** Python, yalnızca standart kütüphane, canlı bir servise bağlanır.
  Yazacağınız şeye en yakın örnek budur.
- [`plugins/ytmusic/main.py`](../plugins/ytmusic/main.py) — üstverisini bir
  servisten, sesini **harici bir araçtan** (yt-dlp alt süreci) alan eklenti.
  İkisini birleştiren bir eklenti yazacaksanız buraya bakın.
- [`fixtures/plugins/echo/main.py`](../fixtures/plugins/echo/main.py) — sabit
  kataloglu sınama eklentisi (~150 satır). Protokolü çıplak görmek için.

---

## 1. Eklenti nedir

Bir dizin:

```
<veri-dizini>/plugins/soundcloud/
├── plugin.json      # manifest
└── main.py          # (ya da bir ikili, bir kabuk betiği, ne olursa)
```

Veri dizini: `$XDG_DATA_HOME/tune` (varsayılan `~/.local/share/tune`).
`TUNE_DATA_DIR` ile değiştirilebilir.

**Dizin adı kimliktir.** `plugin.json` içindeki `name` dizin adıyla aynı
olmak zorunda; uyuşmazsa eklenti reddedilir. Sessizce dizin adına düşmüyoruz,
çünkü o zaman hangi adın kazandığı tahmin edilirdi.

### `plugin.json`

```json
{
  "name": "soundcloud",
  "display_name": "SoundCloud",
  "version": "0.1.0",
  "api": 1,
  "exec": ["python3", "./main.py"],
  "capabilities": ["search", "stream"],
  "permissions": {
    "net": ["api.soundcloud.com"],
    "fs": []
  },
  "description": "Tek cümlelik açıklama."
}
```

| Alan | Zorunlu | Anlamı |
|---|---|---|
| `name` | evet | Dizin adıyla aynı. Sağlayıcı kimliği bu. |
| `display_name` | evet | Kullanıcıya gösterilen ad. |
| `version` | hayır | Eklentinin kendi sürümü (protokol sürümü değil). |
| `api` | evet | Konuştuğu protokol sürümü — bugün `1`. |
| `exec` | evet | İlk öğe program, gerisi argüman. |
| `capabilities` | hayır | `search`, `browse`, `stream`, `control`. |
| `permissions` | hayır | Aşağıya bakın. |
| `requires` | hayır | Motorun sizin için kuracağı eserler — §2.5. |

`exec`'in ilk öğesinde `/` varsa eklenti dizinine göre çözülür (`./main.py`),
yoksa `PATH`'ten aranır. Süreç **eklenti dizininde** çalıştırılır, yani göreli
yollar kendi dosyalarınıza işaret eder.

**Özel durum:** `exec`'in ilk öğesi çıplak `python3` ya da `python` ise onu
`PATH`'ten değil **motor** çözer (D-055) — sürümü denetlenmiş tek bir
yorumlayıcı, bütün eklentiler için aynısı. "Hangi Python" sorusuyla işiniz
olmaz.

---

## 2. İzinler: sözleşme, güvenlik duvarı değil

`permissions.net` erişeceğiniz ana bilgisayarları, `permissions.fs`
dokunacağınız yol öneklerini bildirir. Kullanıcı bunları
`tune plugin approve <ad>` ile onaylar; onay `plugins.json`'a yazılır.
İzinleri **büyütürseniz** kullanıcıya yeniden sorulur, küçültürseniz
sorulmaz.

**Bu bir hapis değildir.** Eklenti kullanıcının bütün yetkisiyle çalışır;
`tune` beyanınızı zorlamaz ve kullanıcıya da böyle söyler. Beyan dürüst
olmak içindir — yalan söyleyen bir manifest, kullanıcıyı kaybetmenin en
hızlı yoludur. (Karar ve gerekçesi: `DECISIONS.md`, D-040.)

Çekirdeğin kendi eliyle verdiği şey daraltılmıştır:

- Yazabileceğiniz dizin: `<veri-dizini>/plugins/<ad>/state` (el sıkışmada
  `data_dir` olarak gelir).
- Görebileceğiniz sırlar: yalnızca **kendi ad alanınız**
  (`plugin:<ad>`). Kullanıcı `tune secret set plugin:soundcloud client_id`
  ile yazar; siz el sıkışmada `secrets` olarak alırsınız. Başka bir
  eklentinin sırrını göremezsiniz.

---

## 2.5 Bağımlılıklar: root isteyemezsiniz (D-049)

Eklentiniz sistemde kurulu bir araca yaslanıyorsa, o aracı **kullanıcıya
kurdurmak sizin çözümünüz değildir.** Kural:

> Bir eklenti ya bağımlılıklarını kendisi getirir, ya da onları **root yetkisi
> istemeden** kuran bir yordam sunar.

Sebep destek yüzeyi: "paket yöneticinle kur" cümlesi her dağıtım ve her
işletim sistemi için ayrı bir yol demek, ve o yolları eklenti yazarı değil
proje taşır. Hata mesajınızda `apt`/`pacman`/`brew` gibi tek bir sisteme ait
komut **yazmayın** — kullanıcıların çoğuna yanlış tavsiye olur.

Uygulanış şekli **D-050'de kapandı, D-055'te yazıldı: `tune`'un kendi eklenti
motoru var.** Çalışma zamanı (Python 3.9+) eklentinin değil host'un işi ve
`tune`'un gereksinimi olarak bir kez ilan ediliyor. İhtiyacınız olan paketleri
**siz kurmazsınız**: `plugin.json`'da `requires` ile beyan edersiniz, motor
onları indirir. Eklenti hiçbir şey indirmez, `pip` çağırmaz, sisteme dokunmaz.

```json
"requires": [
  {
    "name": "yt-dlp",
    "version": "2026.08.19",
    "url": "https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp",
    "sha256": "1fa6733c37ea6fb51c99ad8fe785e7b7e5f3246c9b980230329d4fb72ed8d4d6"
  }
]
```

Dört alanın dördü de **zorunlu.** Sürümsüz bir eser güncellendiğinde sessizce
başka bir şey olur; karmasız bir eser ağdan ne geldiyse odur. `url` `https://`
ile başlamak zorunda. Doğrulama tutmazsa dosya **yerine konmaz.**

Eser tek dosya olmalı — motor `venv`/`pip` kullanmıyor (bkz. D-055: `ensurepip`
her dağıtımda yok ve orada motor root'suz bir çıkış yolu sunamazdı). yt-dlp
gibi zipapp olarak dağıtılan araçlar bu yola uyar; sıradan bir PyPI paketi
bugün kurulamaz.

**Kurulum yolunu el sıkışmada alırsınız.** `handshake` parametrelerindeki
`requirements` haritası `ad → mutlak yol` verir ve **yalnızca hazır eserleri**
içerir: haritada bir ad varsa o eser kurulu ve karması doğrulanmıştır, yoksa
hiç yoktur. Boş dize dönmez.

```python
def handshake(params):
    state["requirements"] = params.get("requirements") or {}
    ...

def ytdlp():
    path = state["requirements"].get("yt-dlp")
    if not path:
        raise PluginError("yt-dlp kurulu değil: `tune plugin install <ad>`")
    return [path]
```

Kullanıcı `tune plugin install <ad>` yazınca motor indirir, doğrular, yerine
koyar. `tune plugin list` süreç açmadan eksiği söyler.

`api` kırılmadı — `requires` da `requirements` da birer **ekleme** ve eklemek
sürümü artırmaz (§5). Bu alanları okumayan eski bir eklenti bugüne kadar
olduğu gibi çalışır.

**Bağlantılar eskirse ne olur.** Beyan ettiğiniz adres bir gün 404 döndürür;
motor bunu "ağ yok" değil **YETİM** diye raporlar ve düzeltmenin *sizin* işiniz
olduğunu söyler. Zaten kurulmuş bir eser bundan etkilenmez: dosya diskte,
karması tutuyor, çalışmaya devam eder. Yetimlik yalnızca henüz kurmamış bir
kullanıcı için bir sorundur — yani yeni sürüm yayımlamak sizin sorumluluğunuz.

---

## 3. Protokol

Her mesaj **tek satır JSON + `\n`**. Uzunluk başlığı yok.

- stdout **yalnızca protokol içindir**. Hata ayıklama çıktınızı **stderr**'e
  yazın; `tune` onu log'a aktarır. (stdout'a düşen JSON olmayan satır sizi
  öldürmez, uyarı olarak atlanır — ama ona güvenmeyin.)
- İsteklerin `id`'si vardır, cevabınız **aynı `id`'yi** taşımalı.
- İstemediğiniz bir metoda `-32601` (metot yok) dönün; çekirdek bunu
  "bu yeteneği desteklemiyor" diye okur, çökme saymaz.

### `handshake` — zorunlu

İlk çağrı budur. **Ağa çıkmayın**: bu çağrının zaman aşımı 5 saniye,
ötekilerin 20.

İstek:

```json
{"jsonrpc":"2.0","id":1,"method":"handshake","params":{
  "api": 1,
  "host": {"name": "tune", "version": "0.0.1-beta"},
  "data_dir": "/home/kisi/.local/share/tune/plugins/soundcloud/state",
  "secrets": {"client_id": "..."},
  "permissions": {"net": ["api.soundcloud.com"], "fs": []}
}}
```

Cevap:

```json
{"jsonrpc":"2.0","id":1,"result":{
  "api": 1,
  "name": "soundcloud",
  "display_name": "SoundCloud",
  "plugin_version": "0.1.0",
  "capabilities": ["search", "stream"]
}}
```

`api` çekirdeğinkiyle **eşit değilse eklenti yüklenmez** ve kullanıcı iki
sayıyı da görür. `name` manifestteki adla aynı olmak zorunda.

### `health` — zorunlu

```json
{"jsonrpc":"2.0","id":2,"result":{
  "reachable": true, "track_count": 1234, "detail": "SoundCloud API v2"
}}
```

`track_count` bilinmiyorsa `null` — sıfır değil. "Bilmiyorum" ile "hiç yok"
farklı cevaplardır. Ulaşamıyorsanız `reachable: false` + `detail` ile
**sebebini** yazın; hata dönmeyin, ulaşamamak bir sağlık cevabıdır.

### `search` — `search` yeteneği varsa

İstek `params`: `{"query": "...", "limit": 20}`.

```json
{"jsonrpc":"2.0","id":3,"result":{"tracks":[
  {"id":"12345","artist":"Sanatçı","title":"Parça",
   "album":"Albüm","duration_ms":213000,"isrc":"TR1234567890"}
]}}
```

`id` **sizin** kimliğiniz — çıplak bir dize. Sağlayıcı adını çekirdek ekler;
başka bir sağlayıcının kimlik alanına yazamazsınız. `isrc` biçimi tutmuyorsa
alan düşürülür ve sayılır (parça düşmez).

### `resolve_source` — `stream` yeteneği varsa

İstek `params`: `{"id": "12345"}`.

```json
{"jsonrpc":"2.0","id":4,"result":{"source":{
  "kind": "http_stream",
  "url": "https://.../stream.mp3",
  "headers": [{"name":"authorization","value":"OAuth ..."}]
}}}
```

`kind` iki değer alır: `http_stream` (yukarıdaki) ve `local_file`
(`{"kind":"local_file","path":"/yol/dosya.flac"}`).

Çalınamıyorsa `{"source": null}` — bu bir hata değil, "yok" cevabıdır.

**Sesi siz röle etmezsiniz.** Verdiğiniz adresi istemci kendisi çeker
(Değişmez Kural K3).

### `shutdown` — bildirim, cevap beklenmez

```json
{"jsonrpc":"2.0","method":"shutdown"}
```

Aldığınızda temizlenip çıkın. Çıkmazsanız stdin kapatılır, sonra
öldürülürsünüz.

### `log` — isteğe bağlı bildirim

Çekirdeğe log yollamak isterseniz:

```json
{"jsonrpc":"2.0","method":"log","params":{"level":"info","message":"..."}}
```

### Hata dönmek

```json
{"jsonrpc":"2.0","id":3,"error":{"code":-32000,"message":"kota doldu"}}
```

Hata dönmek **çökmek değildir**: süreciniz ayakta kalır, çekirdek yalnızca o
çağrıyı başarısız sayar.

---

## 4. Yaşam döngüsü ve dayanıklılık

- Süreciniz **ilk çağrıda** başlatılır, kurulumda değil.
- Çökerseniz çağrı hata döner ve bir sonraki çağrıda **yeniden
  başlatılırsınız**. Üç denemeden sonra vazgeçilir (sonsuz yeniden başlatma
  bir çökme döngüsünü gizler).
- Zaman aşımına uğrarsanız süreciniz düşürülür — cevap vermeyen bir
  eklentiyle sonraki çağrıların `id`'leri karışırdı.
- Sürüm uyuşmazlığında yeniden denenmezsiniz; tekrarla düzelmez.

---

## 5. Sürümleme kuralı

`api` tek bir tam sayı ve kuralı tema sözleşmesiyle aynı:

> **Eklemek sürümü artırmaz, kaldırmak ya da anlamını değiştirmek artırır.**

Yeni bir metot eklendiğinde eski eklentiler onu bilmez, `-32601` döner ve
çekirdek "desteklemiyor" diye okur. Bu yüzden `api 1` bilerek dar tutuldu:
`scan_catalog` ve `catalog_changed_since` gibi opsiyonel metotlar, gerçek bir
kullanıcıları çıktığında ve tel biçimleri ölçüldüğünde eklenecek.

---

## 6. Kurulum ve sınama

```bash
# Eklentiyi yerine koyun
mkdir -p ~/.local/share/tune/plugins/soundcloud
cp plugin.json main.py ~/.local/share/tune/plugins/soundcloud/

# Görünüyor mu, ne istiyor?
tune plugin list

# İzinleri onaylayın
tune plugin approve soundcloud

# Sır gerekiyorsa (değer komut satırına yazılmaz)
tune secret set plugin:soundcloud client_id

# Ayakta mı?
tune provider test soundcloud

# Bir şey ters giderse: hangi aşamada bozulduğunu söyler
tune diag
```

`tune plugin disable <ad>` kapatır (onay korunur), `enable` geri açar,
`forget` onayı tamamen unutur.

Hata mesajları aşamayı taşır: `PLUGIN_LOAD` (manifest/onay),
`PLUGIN_HANDSHAKE` (başlatma/sürüm), `PROVIDER_CALL` (çağrı).

---

## 7. SoundCloud eklentisini kurmak

Depodaki `plugins/soundcloud/` doğrudan kullanılabilir:

```bash
mkdir -p ~/.local/share/tune/plugins/soundcloud
cp plugins/soundcloud/{main.py,plugin.json} ~/.local/share/tune/plugins/soundcloud/

tune plugin approve soundcloud
tune provider test soundcloud     # "kullanılabilir" demeli
tune play "nujabes aruarian dance"
```

`client_id` **istenmez**: eklenti SoundCloud'un web istemcisinden kendisi
keşfeder ve `state/client_id.txt` içine önbellekler. Kendi anahtarınız varsa
o kullanılır ve keşfe hiç gidilmez:

```bash
tune secret set plugin:soundcloud client_id
```

`tune provider test soundcloud` hangi kaynağın kullanıldığını yazar
(`sır` / `önbellek` / `keşif`) — yanlış anahtarla çalışan bir kurulum sessizce
doğru görünmesin diye (D-043).

**Bilinen sınırlar**, ikisi de kasıtlı:

- **Yalnızca `progressive` (düz HTTP MP3).** Ölçüldü: parçaların %99'unda var.
  Kalan %1 yalnızca HLS sunuyor ve açık bir hata alır — sessizce boş sonuç
  değil.
- **`[önizleme]` etiketli parçalar 30 saniyedir.** SoundCloud'un `SNIP`
  politikası; tam parça abonelik istiyor. api 1'de bunu taşıyacak bir alan
  olmadığı için başlığa yazılıyor.

Keşif dokümante edilmemiş bir yola dayanıyor ve **haber vermeden bozulabilir**.
Bozulursa eklenti size kendi `client_id`'nizi vermenizi söyler; sessizce boş
sonuç döndürmez.

---

## 8. YouTube Music eklentisini kurmak

Depodaki `plugins/ytmusic/` doğrudan kullanılabilir. **yt-dlp'yi siz
kurmazsınız** — eklenti onu manifestinde beyan eder, motor indirir (D-055).

```bash
mkdir -p ~/.local/share/tune/plugins/ytmusic
cp plugins/ytmusic/{main.py,plugin.json} ~/.local/share/tune/plugins/ytmusic/

tune plugin approve ytmusic    # izinleri ve motorun indireceğini gösterir
tune plugin install ytmusic    # yt-dlp'yi indirir, sha256'sını doğrular
tune provider test ytmusic     # "kullanılabilir" + yt-dlp sürümünü yazmalı
tune play "nujabes aruarian dance"
```

Sır **istemiyor**. `install` çalıştırılmadan önce `tune plugin list` eksiği
süreç açmadan söyler; eklenti sessizce boş sonuç dönmez.

Eser `~/.local/share/tune/runtime/yt-dlp-<sürüm>` altına iner ve sisteme
hiçbir şey yazılmaz. `install`'ı ikinci kez koşturmak ağa çıkmaz. Dosya
bozulursa (`karma tutmuyor`) yeniden koşturmak düzeltir.

**Neden alt süreç, neden kütüphane değil** (D-048): depoda hiçbir Python
bağımlılığı yok, ve YouTube bir şeyi bozduğunda kullanıcının gördüğü mesaj
yt-dlp'nin kendi mesajı oluyor ("Sign in to confirm you're not a bot" gibi).
Tamiri de yt-dlp yapıyor, biz değil.

**Ama D-055'ten sonra sürümü biz sabitliyoruz** ve bunun bir bedeli var:
YouTube yt-dlp'yi bozduğunda kullanıcı artık kendi paket yöneticisiyle
güncelleyip kurtulamaz, manifestte yeni bir sürüm yayımlamamızı bekler.
Bakım yükü bir miktar bize geçti — D-049'un "güncel kalabilir" şartının
karşılığı bu depoda `requires`'ı güncel tutmaktır.

**Bilinen sınırlar**, üçü de ölçülmüş:

- **Ses m4a (AAC-LC, ~130 kbps).** Daha iyisi var (opus, 136 kbps) ama
  çekirdeğin symphonia'sında ne opus çözücüsü ne webm kabı var; çalınamayan
  yüksek kalite yerine çalınabilen düşük kalite seçildi.
- **Akış `Range: bytes=0-` başlığıyla çekiliyor.** Bu başlık olmadan aynı adres
  32 KB/s veriyor, onunla 8 MB/s — 250 kat. Başlık `source.headers` içinde
  geliyor; kendi eklentinizi yazarken benzer bir kısıtlamayla karşılaşırsanız
  taşıyacağınız yer orası.
- **İzin beyanı eksik.** Ses adresi her çözümde değişen bir
  `googlevideo.com` ana bilgisayarında duruyor ve izin sözlüğü joker kabul
  etmiyor (`*` yok). `music.youtube.com` + `www.youtube.com` beyan edildi,
  gerisi `description`'da yazıyor. Aynı sıkıntı torrent eklentisinde de var.
