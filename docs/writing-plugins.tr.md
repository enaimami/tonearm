# Eklenti yazma rehberi (sözleşme api 2)

> **Türkçe kopya.** Kanonik metin İngilizcedir: [`writing-plugins.md`](writing-plugins.md).
> Bu kopya 2026-09-25 tarihli hâlidir; İngilizce metinle birlikte güncel
> tutulacağı garanti değildir (D-073).

`headshell` eklentileri **JavaScript** ile yazılır ve `headshell`'un içine
gömülü **QuickJS** motorunda koşar (D-069). Kullanıcının makinesinde Python,
Node ya da başka bir çalışma zamanı gerekmez: eklentiyi bir dizine koymak
yeter.

Bir eklenti dış dünyaya yalnızca motorun verdiği `host` nesnesinden çıkabilir
— HTTP, sırlar, küçük bir kalıcı depo, motorun kurduğu araçlar ve günlük.
Dosya sistemine, sürece, sokete doğrudan erişimi yoktur. Beyan ettiği izinler
bu yüzden **zorlanır**.

Eklentiler bu depoda durmaz: [`headshell/plugins`](https://github.com/headshell/plugins)
kataloğunda yaşarlar ve kullanıcılar onları oradan kurar (D-071, §9).

Üç çalışan örnek:

- [`soundcloud/main.js`](https://github.com/headshell/plugins/blob/main/soundcloud/main.js)
  — **gerçek eklenti.** Canlı bir servise bağlanır, anahtarını keşfedip
  `host.storage`'ta önbellekler. Yazacağınız şeye en yakın örnek budur.
- [`ytmusic/main.js`](https://github.com/headshell/plugins/blob/main/ytmusic/main.js)
  — üstverisini bir servisten, sesini **motorun kurduğu bir araçtan** (yt-dlp)
  alan eklenti.
- [`fixtures/plugins/echo/main.js`](../fixtures/plugins/echo/main.js) — sabit
  kataloglu sınama eklentisi (~60 satır). Sözleşmeyi çıplak görmek için.

> **api 1'den geliyorsanız:** api 1 bir alt süreç + JSON-RPC protokolüydü ve
> eklentiler Python'la yazılıyordu. api 1 eklentileri artık **yüklenmez**;
> `headshell plugin list` onları "protokol sürümü uyuşmuyor: eklenti api 1"
> diye gösterir. Taşıma kısa: `exec` yerine `main`, metotlar yerine dışa
> aktarılan fonksiyonlar, `urllib` yerine `host.http`, dosya yerine
> `host.storage`. SoundCloud ve YouTube Music eklentileri bu yolu izledi.

---

## 1. Eklenti nedir

Bir dizin:

```
<veri-dizini>/plugins/soundcloud/
├── plugin.json      # manifest
└── main.js          # betik
```

Veri dizini sistemden sisteme değişir (D-070); `headshell diag` kullanılanı
yazar:

| Sistem | Veri dizini |
|---|---|
| Linux, BSD | `~/.local/share/headshell` (`$XDG_DATA_HOME` tanımlıysa onun altı) |
| macOS | `~/Library/Application Support/headshell` |
| Windows | `%LOCALAPPDATA%\headshell` |

`HEADSHELL_DATA_DIR` ortam değişkeni ya da CLI'nin `--data-dir` bayrağı her
sistemde önce gelir. Aşağıdaki komutlar Linux yolunu kullanıyor; öteki
sistemlerde yalnızca dizin değişir.

**Dizin adı kimliktir.** `plugin.json` içindeki `name` dizin adıyla aynı
olmak zorunda; uyuşmazsa eklenti reddedilir.

### `plugin.json`

```json
{
  "name": "soundcloud",
  "display_name": "SoundCloud",
  "version": "0.2.0",
  "api": 2,
  "main": "main.js",
  "capabilities": ["search", "stream"],
  "permissions": {
    "net": ["soundcloud.com", "api-v2.soundcloud.com", "*.sndcdn.com"]
  },
  "requires": [],
  "description": "Bir cümle."
}
```

| Alan | Zorunlu | Anlamı |
|---|---|---|
| `name` | evet | Dizin adıyla aynı. Sağlayıcı kimliği. |
| `display_name` | evet | Kullanıcıya gösterilen ad. |
| `version` | katalogda evet | Eklentinin kendi sürümü. Katalogdaki her eklenti sürümlüdür: güncellemeyi o yakalar ve dosya adresleri ona sabitlenir (§9). |
| `api` | evet | Konuştuğu sözleşme sürümü: bugün `2`. |
| `main` | evet | Eklenti dizinine göre betiğin yolu. `.js` olmalı, dizinin dışına çıkamaz (`..` ve mutlak yol reddedilir). |
| `capabilities` | hayır | `search`, `stream`. Motor, beyan edilen her yeteneğin fonksiyonunun dışa aktarıldığını denetler. |
| `permissions.net` | hayır | Bağlanılacak ana bilgisayarlar. Bkz. §2. |
| `requires` | hayır | Motorun kuracağı araçlar. Bkz. §5. |
| `description` | hayır | Bir cümle. |

api 1'in iki alanı **reddedilir**, yok sayılmaz: `exec` (eklenti bir komut
değil, betiktir) ve `permissions.fs` (eklenti dosya sistemine erişemez).

---

## 2. İzinler — zorlanıyor

Her `host.http` isteğinde, izlenen **her yönlendirmede** ve `resolve_source`'un
döndürdüğü akış adresinde motor ana bilgisayarı `permissions.net`'e göre
denetler. Beyan edilmemiş bir adrese istek ağa hiç çıkmaz; eklentiye
`izin yok: <ana bilgisayar> …` hatası fırlatılır.

- **Tam ad:** `api.soundcloud.com` yalnızca o adı kapsar, alt alan adlarını
  kapsamaz.
- **Joker:** `*.sndcdn.com` her alt alan adını (`cf-media.sndcdn.com`)
  kapsar, `sndcdn.com`'un kendisini **kapsamaz**. Yalnızca en solda olabilir.
  Çıplak `*` ve `*.com` gibi tek etiketli joker reddedilir — "her yere
  çıkarım" diyen bir eklenti bunu tek tek yazmalı ya da kullanıcı onu
  reddetmeli.
- **Şema, port ve yol yazılmaz.** Yalnızca `http` ve `https`'e gidilebilir;
  herhangi bir port serbest.
- IP adresi yazılabilir (`127.0.0.1`) ama IPv6 köşeli ayraçlı adres
  desteklenmiyor.

**Onay:** İlk kurulumda `headshell plugin approve <ad>` ile kullanıcı izinleri
onaylar. Onaylanan küme `<veri-dizini>/plugins.json`'da saklanır. Eklentiniz
güncellenip **daha fazla** izin isterse yeniden onaya kadar yüklenmez; daha
**az** isterse sorun yok. Joker hesaba katılır: onaylanmış `*.x.com`, sonradan
istenen `a.x.com`'u kapsar.

**Sınırın dışında kalan tek şey motorun kurduğu araçlardır.** `host.tools.run`
ile çalışan bir program (yt-dlp) ayrı bir süreçtir ve hapsedilmez; kendi ağ
trafiği bu listeyle sınırlanmaz. `headshell plugin list` bunu her seferinde
yazar.

---

## 3. Sözleşme: dışa aktarılan fonksiyonlar

Betik bir **ES modülüdür**. Çekirdek onu ilk çağrıda değerlendirir ve dışa
aktardığı fonksiyonları çağırır:

```js
export function health() {
  return { reachable: true, detail: "her şey yolunda" };
}

export function search(query, limit) {
  return [{ id: "42", artist: "Ezhel", title: "Geceler", duration_ms: 213000 }];
}

export function resolve_source(id) {
  return { kind: "http_stream", url: `https://cdn.ornek.com/${id}.mp3`, headers: [] };
}
```

| Fonksiyon | Zorunlu | Dönüş |
|---|---|---|
| `health()` | evet | `{ reachable, detail?, track_count? }` |
| `search(query, limit)` | `search` yeteneği varsa | parça dizisi |
| `resolve_source(id)` | `stream` yeteneği varsa | kaynak ya da `null` |

Adlar bilerek api 1'in metot adlarıyla ve çekirdeğin `Provider` trait'iyle
aynı (`resolve_source`, `resolveSource` değil). `async function` de
yazabilirsiniz; motor sözü çözer.

### `health()`

Ulaşamamak bir **cevaptır**, hata değil: `{ reachable: false, detail: "…" }`
dönün ve sebebi yazın. `track_count` bilinmiyorsa `null` bırakın — yanlış bir
sayı vermek "bilmiyorum"dan kötüdür. Fırlatırsanız da çekirdek bunu
"ulaşılamıyor" diye gösterir, fırlattığınız mesajla.

### `search(query, limit)`

```js
[
  {
    id: "42",               // zorunlu — çıplak kimlik
    artist: "Ezhel",         // zorunlu
    title: "Geceler",        // zorunlu
    album: "Müptezhel",      // isteğe bağlı
    duration_ms: 213000,     // isteğe bağlı ama bulanık eşleşme için önemli
    isrc: "TRA111700001"     // isteğe bağlı; biçimi tutmazsa düşürülür ve sayılır
  }
]
```

`id` **çıplak** bir dizedir. Sağlayıcı adını çekirdek ekler — başka bir
sağlayıcının ad alanında kimlik uyduramazsınız. Sonuç yoksa `[]` dönün.

### `resolve_source(id)`

```js
{ kind: "http_stream", url: "https://…", headers: [{ name: "Range", value: "bytes=0-" }] }
```

`null` "bu parça çalınamaz" demektir ve hata değildir. Adres izinlerinizin
içinde olmalı (§2); dışındaysa çekirdek kaynağı reddeder. `local_file`
döndürülemez — eklentinin dosya sistemine erişimi yok.

**Ses asla röle edilmez (K3):** döndürdüğünüz adresi çekirdek kendisi çeker.

### Hata vermek

Sıradan bir JS hatası fırlatın:

```js
throw new Error("SoundCloud kotayı doldurdu (429); bir süre bekleyin");
```

Çekirdek bunu **reddetme** olarak okur ve eklentiyi yeniden başlatmaz.
Kullanıcı mesajınızı ve hatanın çıktığı satırı görür:
`soundcloud eklentisi search çağrısında hata verdi: … (main.js:42:7)`.
"Bulamadım" ile "bakamadım" farklı şeylerdir (K9): sonuç yoksa `[]`/`null`
dönün, bir şey bozulduysa fırlatın.

Sözleşmeye uymayan bir dönüş (dizi yerine nesne, `kind`'ı tanınmayan kaynak)
**sözleşme ihlali** olarak ayrıca raporlanır: kullanıcı onu bekleyerek
düzeltemez, yalnızca siz düzeltebilirsiniz.

---

## 4. `host` — dış dünyaya açılan kapı

Hepsi **eşzamanlı**: fonksiyon döndüğünde iş bitmiştir. Motorda olay döngüsü
ve zamanlayıcı yok (`setTimeout` yok).

| API | Ne yapar |
|---|---|
| `host.http.get(url, headers?)` | GET. Başlıklar düz bir nesne: `{ "User-Agent": "…" }`. |
| `host.http.post(url, body, headers?)` | POST, gövde bir dize. |
| `host.http.request({ url, method?, headers?, body? })` | Genel biçim; `method` `GET` ya da `POST`. |
| `host.secrets.get(key)` | Kendi ad alanınızdaki sır; yoksa `null`. |
| `host.secrets.file(key)` | Sırrı `0600` bir geçici dosyaya yazar ve yolunu döndürür (yoksa `null`). Motor kapanınca silinir. Bir araca dosya yolu olarak vermek için. |
| `host.storage.get(key)` / `.set(key, value)` / `.remove(key)` | Eklentiye özel kalıcı anahtar-değer; değerler dize. Toplam 1 MB. |
| `host.tools.run(name, args, { timeoutMs? })` | `requires`'ta beyan edilmiş, kurulu ve karması doğrulanmış aracı çalıştırır. |
| `host.log.debug/info/warn/error(message)` | `tracing`'e yazar; `headshell -v` ile görünür. |
| `console.log/info/warn/error/debug` | `host.log`'a bağlı. |
| `host.api`, `host.version`, `host.platform`, `host.plugin` | Sözleşme sürümü, çekirdek sürümü, platform anahtarı (`linux-x86_64`), eklentinin adı. |

**HTTP cevabı:**

```js
{ status: 200, ok: true, url: "son adres", headers: { "content-type": "…" }, body: "…" }
```

2xx dışı durum kodları **fırlatılmaz**, `status` ile döner — 404 ile 500'ü
ayırmak sizin işiniz. Ağa ulaşılamazsa, izin yoksa ya da çağrının süresi
dolduysa fırlatılır. Yönlendirmeleri motor izler (en çok 5) ve her adımda
izni yeniden sorar. Gövde metin olarak gelir (UTF-8, bozuk baytlar
değiştirilir); ikili içerik için tasarlanmadı.

**Araç çıktısı:**

```js
{ code: 0, stdout: "…", stderr: "…", truncated: false }
```

Aracın süresi çağrının kalan süresini geçemez; `timeoutMs` daha kısa bir
sınır koyar. Süre dolarsa araç durdurulur ve fırlatılır.

### Modül yüklenirken

Betiğin üst düzeyi (fonksiyonların dışı) yüklemede bir kez çalışır ve **ağa
çıkamaz, araç çalıştıramaz** — bu işler ilk çağrıya aittir. Yükleme 5 saniyeyle
sınırlı.

### Sınırlar

| | |
|---|---|
| Çağrı süresi | 20 sn. Döngüde takılan kod kesilir (`try/catch` kesmeyi yakalayamaz). |
| Yükleme süresi | 5 sn |
| Bellek | 128 MB; aşılırsa "out of memory" istisnası — çekirdek değil, o çağrı düşer |
| Tek HTTP isteği | 15 sn |
| Modül | Tek dosya; `import` ile başka dosya yüklenemez |
| Web API'leri | Yok: `fetch`, `URL`, `TextEncoder`, `Intl`, `setTimeout`. Dil ve standart kütüphanesi (JSON, RegExp, Map, Date…) var. |

---

## 5. Araçlar: `requires` (D-049, D-055, D-069)

**Hiçbir eklenti kullanıcıdan root yetkisi ya da sistem çapında bir kurulum
isteyemez** (D-049). Aracınızı kullanıcıya kurdurmazsınız; manifestte beyan
edersiniz, **motor** kurar:

```json
"requires": [
  {
    "name": "yt-dlp",
    "version": "2026.08.19",
    "assets": {
      "linux-x86_64":   { "url": "https://…/yt-dlp_linux",   "sha256": "58162f9b…" },
      "macos-aarch64":  { "url": "https://…/yt-dlp_macos",   "sha256": "0f192b7e…" },
      "windows-x86_64": { "url": "https://…/yt-dlp.exe",     "sha256": "66674953…" }
    }
  }
]
```

- Eser **platform başına** beyan edilir. Anahtar `<işletim sistemi>-<mimari>`,
  musl Linux için sonuna `-musl`. Tanınan anahtarlar: `linux-x86_64`,
  `linux-aarch64`, `linux-x86`, `linux-arm`, `linux-x86_64-musl`,
  `linux-aarch64-musl`, `macos-x86_64`, `macos-aarch64`, `windows-x86_64`,
  `windows-aarch64`, `windows-x86`. Bilinmeyen anahtar manifesti geçersiz
  kılar (yazım hatası "bu platformda yok"a dönüşmesin diye).
- Her yayın **kendi kendine yeten** bir ikili olmalı — başka bir yorumlayıcı
  istemeyen (yt-dlp'nin zipapp'i Python istiyor, PyInstaller ikilileri
  istemiyor).
- `url` `https://` olmalı, `sha256` 64 haneli. Karma tutmazsa dosya yerine
  konmaz.
- Kullanıcının platformu haritada yoksa eklenti yüklenmez ve durum satırı
  bunu söyler; kurulum komutu önerilmez, çünkü kurulum bunu düzeltmez.
- Eser `<veri-dizini>/runtime/<ad>-<sürüm>-<platform>` altına iner
  (Windows'ta `.exe` ile). Sisteme hiçbir şey yazılmaz.

Eklenti aracı adıyla çağırır; yolunu bilmesine gerek yok:

```js
const run = host.tools.run("yt-dlp", ["--version"], { timeoutMs: 5000 });
```

Motor aracı ilk kullanımda karmasıyla yeniden doğrular: kurulumdan sonra
değiştirilmiş bir dosya çalıştırılmaz.

---

## 6. Yaşam döngüsü ve dayanıklılık

- Motor eklentiniz için **ilk çağrıda** açılır, kurulumda değil. Her eklenti
  kendi iş parçacığında, kendi QuickJS çalışma zamanında koşar; eklentiler
  birbirinin durumunu göremez.
- Fırlattığınız bir hata motoru düşürmez: modül durumu (önbelleğe aldığınız
  değişkenler) bir sonraki çağrıda yerinde durur.
- Süre dolarsa motor bırakılır ve bir sonraki çağrı betiği **baştan**
  yükler — yarım kalmış bir çağrının bıraktığı durumla devam edilmez.
- Üç başlatmadan sonra vazgeçilir; sonsuz yeniden başlatma bir çökme
  döngüsünü gizler. Sözleşme ihlalinde (eksik dışa aktarım) hiç yeniden
  denenmez.
- **Bilinen sınır:** QuickJS'in kendi C kodunda bir çökme çekirdeği de
  düşürür — api 1'de eklenti ayrı süreçti, api 2'de değil (D-069'un takası).
  JS'in yapabileceği hiçbir şey (sonsuz döngü, bellek taşması, derin
  özyineleme) bu sınıfta değil.

---

## 7. Sürümleme kuralı

`api` tek bir tam sayı ve kuralı tema sözleşmesiyle aynı:

> **Eklemek sürümü artırmaz, kaldırmak ya da anlamını değiştirmek artırır.**

`host`'a yeni bir fonksiyon, manifeste yeni bir alan, `PLATFORMS`'a yeni bir
platform eklemek `api`'yi artırmaz. api 1 → 2 geçişi ikinci türdendi:
eklentinin nerede koştuğu değişti.

---

## 8. Geliştirirken kurmak ve sınamak

Geliştirdiğiniz eklentiyi dizin olarak elle koyun. Katalog **elle konmuş bir
eklentiye dokunmaz** — güncelleme onun üstüne yazmaz — o yüzden çalışma
kopyanızı bir bağlantıyla da koyabilirsiniz; `headshell plugin remove`
yalnızca bağlantıyı kaldırır, kopyanıza dokunmaz.

```bash
# Eklentiyi yerine koyun (Linux; macOS ve Windows yolu için §1'deki tablo)
mkdir -p ~/.local/share/headshell/plugins/benim
cp plugin.json main.js ~/.local/share/headshell/plugins/benim/

# Görünüyor mu, ne istiyor?
headshell plugin list

# İzinleri onaylayın
headshell plugin approve benim

# Araç istiyorsa
headshell plugin install benim

# Sır gerekiyorsa (değer komut satırına yazılmaz)
headshell secret set plugin:benim client_id

# Ayakta mı?
headshell provider test benim

# Bir şey ters giderse: hangi aşamada bozulduğunu söyler
headshell diag
```

Windows'ta (PowerShell) yerleştirme adımı:

```powershell
$hedef = "$env:LOCALAPPDATA\headshell\plugins\benim"
New-Item -ItemType Directory -Force -Path $hedef | Out-Null
Copy-Item plugin.json, main.js $hedef
```

`headshell plugin disable <ad>` kapatır (onay korunur), `enable` geri açar,
`forget` onayı tamamen unutur, `remove` eklentiyi kaldırır ve onayını unutur.

Hata mesajları aşamayı taşır: `PLUGIN_LOAD` (manifest/onay),
`PLUGIN_CATALOG` (katalog: indeks, indirme, karma, güncelleme, kaldırma),
`PLUGIN_RUNTIME` (araç kurulumu), `PLUGIN_START` (betiği yükleme, dışa
aktarım denetimi), `PROVIDER_CALL` (çağrı).

---

## 9. Katalog: `headshell/plugins` (D-071)

Kullanıcılar eklentileri [`headshell/plugins`](https://github.com/headshell/plugins)
kataloğundan kurar:

```bash
headshell plugin catalog              # ne var, ne kurulu, ne güncellenebilir
headshell plugin install soundcloud   # indirir, her dosyanın sha256'sını doğrular
headshell plugin approve soundcloud   # kurulan eklenti onay bekler
headshell plugin update               # katalogdan kurulanları güncelle
```

Katalog deposunun kökündeki `index.json` her eklentinin manifestini ve
dosyalarının adresini + sha256'sını taşır. İstemci her dosyayı karmasıyla
doğrular ve inen `plugin.json`'u indeksin gösterdiğiyle karşılaştırır; biri
tutmazsa diske hiçbir şey yazmaz. İndeks elle yazılmaz, çekirdeğin kendi
doğrulamasıyla üretilir:

```bash
headshell plugin index <katalog-deposu>           # index.json'u yaz
headshell plugin index <katalog-deposu> --check   # güncel mi (CI)
```

**Yayımlamak:** katalog deposuna bir PR — `<ad>/plugin.json`, betik ve
yeniden üretilmiş `index.json`. Ad küçük harf ASCII (harf, rakam, `-`, `_`,
`.`), `version` zorunlu. Dosya adresleri `<ad>-<sürüm>` etiketine sabitlidir;
bir sürümün dosyaları yayımlandıktan sonra değişmez, düzeltme yeni bir
sürümdür. Ayrıntılar ve yayım adımları katalog deposunun README'sinde.

Katalog yalnızca bu komutlarla okunur; uygulama açılırken ya da arka planda
ağa çıkmaz. Başka bir katalog için `HEADSHELL_PLUGIN_INDEX=<adres>`: adres
`https://` olmalı (düz `http` yalnızca `127.0.0.1`/`localhost` için).

Eklenti başına kullanım notları — SoundCloud'un `client_id` keşfi, YouTube
Music'in çerezi, yt-dlp'nin platform listesi ve sürüm sabitleme — kendi
dizinlerinin README'sinde:
[`soundcloud/`](https://github.com/headshell/plugins/tree/main/soundcloud),
[`ytmusic/`](https://github.com/headshell/plugins/tree/main/ytmusic).
