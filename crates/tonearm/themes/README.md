# Tema yazma

Bir tema `tonearm`'un renklerini, köşe yarıçaplarını ve canlandırma süresini
değiştirir. JavaScript çalıştırmaz, veriye erişmez, uygulamanın davranışını
değiştirmez — yalnızca görünüm.

Bu dizindeki iki tema (`daylight`, `contrast`) uygulamayla birlikte geliyor ve
aynı zamanda örnek: kopyalayıp kendinizinkini yazabilirsiniz.

## Kurulum

Bir tema, içinde iki dosya olan bir dizindir:

```
<veri-dizini>/themes/<tema-adi>/
  theme.json
  theme.css
```

Veri dizininin yolu uygulamada **görünüm** sekmesinde yazıyor (genellikle
`~/.local/share/tonearm`). Dizini oraya koyup **listeyi yenile** deyin —
uygulamayı kapatmanız gerekmez. Dizin adı temanın kimliğidir.

`theme.json`:

```json
{
  "name": "Tema Adım",
  "author": "Siz",
  "api": 1
}
```

`api`, aşağıdaki sözleşmenin sürümü. Uyuşmazsa tema **sessizce yok sayılmaz**;
listede hangi sürümü istediğiyle birlikte görünür.

## Sözleşme: token'lar

Garanti edilen yüzey **yalnızca** `theme.css`'in `:root` bloğundaki bu
değişkenlerdir. Yazmadığınız token varsayılan değerinde kalır.

| Token | Varsayılan | Anlamı |
|---|---|---|
| `--tonearm-color-scheme` | `dark` | `dark` \| `light` — onay kutusu, imleç, kaydırma çubuğu gibi motorun kendi çizdiği parçalar için |
| `--tonearm-bg` | `#12100e` | sayfa arka planı |
| `--tonearm-surface` | `#1b1815` | panel arka planı (üst çubuk, kenar çubuğu, oynatıcı) |
| `--tonearm-surface-raised` | `#221e1a` | etkileşimli/üzerine gelinen arka plan (girdi, aktif sekme, uyarı) |
| `--tonearm-border` | `#2e2925` | kenarlık, ayraç, ilerleme çubuğu izi |
| `--tonearm-text` | `#eae2d8` | birincil metin |
| `--tonearm-text-dim` | `#9a8f83` | ikincil/soluk metin |
| `--tonearm-accent` | `#ffb454` | marka + etkileşim vurgusu |
| `--tonearm-success` | `#7bd88f` | çalıyor / başarı |
| `--tonearm-error` | `#ff6b6b` | hata |
| `--tonearm-info` | `#79c0ff` | arabelleğe alınıyor / bilgi |
| `--tonearm-radius-sm` | `6px` | kontrol köşe yarıçapı |
| `--tonearm-radius-lg` | `8px` | panel köşe yarıçapı |
| `--tonearm-duration` | `120ms` | canlandırma süresi (`0ms` = hareket yok) |

En kısa çalışan tema:

```css
:root {
  --tonearm-accent: #7aa2f7;
}
```

## `:root` dışına çıkmak

Sınıf adları da (`.topbar`, `.player`, `.queue`, `.toast`, …) sözleşmenin
parçası ve habersiz yeniden adlandırılmıyorlar — tam liste
`crates/tonearm/tests/ui_contract.rs` içindeki `CONTRACT_CLASSES` dizisidir ve
bir test onu tutuyor. Ama `:root` dışına yazan bir
tema listede **"genişletilmiş · garantisi yok"** diye işaretlenir.

Reddedilmez — engellemiyoruz, saklamıyoruz da. Bir `api` sürüm atlamasında
geriye dönük uyumluluk yalnızca yukarıdaki token'lar için taahhüt ediliyor.

## Canlandırma: yalnızca `transform` ve `opacity`

Bu bir üslup tercihi değil, ölçüm. WebKitGTK'da (Linux'ta varsayılan motor)
`height`, `box-shadow`, `filter` ve `background-position` canlandırmaları
kare hızını 58.8'den 47.6'ya düşürüyor; `transform` ve `opacity` hiç
düşürmüyor.

Bu yüzden ilerleme çubuğu `scaleX` ile çiziliyor, genişlikle değil. Aynısını
yapın — yoksa fark **kullanıcının** makinesinde ortaya çıkar ve suçlanan tema
değil uygulama olur.

## Sürümleme

- Yeni token **eklenmesi** `api`'yi artırmaz: temanız onu yazmıyordu,
  varsayılanını alır ve çalışmaya devam eder.
- Bir token'ın **kaldırılması** ya da anlamının değişmesi artırır.

Şu anki sürüm: **`api: 1`**.
