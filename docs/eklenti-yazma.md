# Eklenti yazma rehberi (protokol api 1)

`tune` sağlayıcıları **alt süreç** olarak çalıştırır ve onlarla satır bazlı
JSON-RPC 2.0 konuşur. Yani bir eklenti herhangi bir dilde yazılabilir:
stdin'den satır okuyup stdout'a satır yazabilen her şey yeterli.

Çalışan bir örnek: [`fixtures/plugins/echo/main.py`](../fixtures/plugins/echo/main.py)
(Python, ~150 satır, protokolün tamamını uygular).

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

`exec`'in ilk öğesinde `/` varsa eklenti dizinine göre çözülür (`./main.py`),
yoksa `PATH`'ten aranır (`python3`). Süreç **eklenti dizininde** çalıştırılır,
yani göreli yollar kendi dosyalarınıza işaret eder.

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
