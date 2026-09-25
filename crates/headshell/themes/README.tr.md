# Tema yazma

> **Türkçe kopya.** Kanonik metin İngilizcedir: [`README.md`](README.md).
> Bu kopya 2026-09-25 tarihli hâlidir; İngilizce metinle birlikte güncel
> tutulacağı garanti değildir (D-073).

Bir tema `headshell`'un renklerini, köşe yarıçaplarını ve canlandırma süresini
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

Veri dizininin yolu uygulamada **görünüm** bölümünde yazıyor (genellikle
`~/.local/share/headshell`). Dizini oraya koyup **listeyi yenile** deyin —
uygulamayı kapatmanız gerekmez. Dizin adı temanın kimliğidir.

Listede her tema kendi renkleriyle çizilmiş küçük bir pencereyle görünür:
`:root`'ta yazdığınız `--headshell-bg`, `--headshell-surface`,
`--headshell-text` ve `--headshell-accent`, yazmadıklarınız varsayılan
temadan — uygulandığında göreceğinizin aynısı. `@media` içindeki koşullu
değerler önizlemeye girmez.

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
| `--headshell-color-scheme` | `dark` | `dark` \| `light` — onay kutusu, imleç, kaydırma çubuğu gibi motorun kendi çizdiği parçalar için |
| `--headshell-bg` | `#12100e` | sayfa arka planı |
| `--headshell-surface` | `#1b1815` | panel arka planı (üst çubuk, kenar çubuğu, oynatıcı) |
| `--headshell-surface-raised` | `#221e1a` | etkileşimli/üzerine gelinen arka plan (girdi, aktif sekme, uyarı) |
| `--headshell-border` | `#2e2925` | kenarlık, ayraç, ilerleme çubuğu izi |
| `--headshell-text` | `#eae2d8` | birincil metin |
| `--headshell-text-dim` | `#9a8f83` | ikincil/soluk metin |
| `--headshell-accent` | `#ffb454` | marka + etkileşim vurgusu |
| `--headshell-success` | `#7bd88f` | çalıyor / başarı |
| `--headshell-error` | `#ff6b6b` | hata |
| `--headshell-info` | `#79c0ff` | arabelleğe alınıyor / bilgi |
| `--headshell-radius-sm` | `6px` | kontrol köşe yarıçapı |
| `--headshell-radius-lg` | `8px` | panel köşe yarıçapı |
| `--headshell-duration` | `120ms` | canlandırma süresi (`0ms` = hareket yok) |

En kısa çalışan tema:

```css
:root {
  --headshell-accent: #7aa2f7;
}
```

## `:root` dışına çıkmak

Sınıf adları da (`.topbar`, `.player`, `.queue`, `.toast`, …) sözleşmenin
parçası ve habersiz yeniden adlandırılmıyorlar — tam liste
`crates/headshell/tests/ui_contract.rs` içindeki `CONTRACT_CLASSES` dizisidir ve
bir test onu tutuyor. Ama `:root` dışına yazan bir
tema listede **"genişletilmiş · garantisi yok"** diye işaretlenir.

Reddedilmez — engellemiyoruz, saklamıyoruz da. Bir `api` sürüm atlamasında
geriye dönük uyumluluk yalnızca yukarıdaki token'lar için taahhüt ediliyor.

Sınıf adlarının yerinde kalması **yerleşimin** aynı kalacağı anlamına
gelmiyor. D-072'de iskelet değişti: kenar çubuğu tam boy oldu, `.topbar`
içerik sütununun üstüne, `.player` onun altına geçti, `.state` bir glif
değil bir nokta oldu (rengi hâlâ `.state.playing { color: … }` ile
değişiyor). Hiçbir sınıf kaldırılmadı ve anlamı değişmedi — `api` 1'de
kaldı — ama konum varsayan genişletilmiş bir tema bunu hissedebilir.

## Canlandırma: yalnızca `transform` ve `opacity`

Bu bir üslup tercihi değil, ölçüm. WebKitGTK'da (Linux'ta varsayılan motor)
`height`, `box-shadow`, `filter` ve `background-position` canlandırmaları
kare hızını 58.8'den 47.6'ya düşürüyor; `transform` ve `opacity` hiç
düşürmüyor.

Bu yüzden ilerleme çubuğu `scaleX` ile çiziliyor, genişlikle değil. Aynısını
yapın — yoksa fark **kullanıcının** makinesinde ortaya çıkar ve suçlanan tema
değil uygulama olur.

### `--headshell-duration` bütün hareketi ölçekler

Arayüzün hareketi iki türlü: kısa geçişler (üstüne gelme, sürükle-bırak
alanı) doğrudan bu süreyi kullanır; konum hareketleri (seçim göstergesi,
bölüm geçişi, uyarılar, kısayol penceresi) **yaydır** ve yayın tepkisi bu
sürenin üç katıdır — varsayılan 120ms → 0.36 sn.

- `0ms` hiçbir şeyin hareket etmediği demek: yaylar da, dönen meşguliyet
  halkası da durur. Yüksek Karşıtlık teması bunu kullanıyor.
- Daha uzun bir süre bütün hareketi orantılı olarak yavaşlatır.
- Sistemin "hareketi azalt" tercihi temadan bağımsız uygulanır: konum ve
  ölçek hareketi kalkar, opaklık geçişi kalır.

## Sürümleme

- Yeni token **eklenmesi** `api`'yi artırmaz: temanız onu yazmıyordu,
  varsayılanını alır ve çalışmaya devam eder.
- Bir token'ın **kaldırılması** ya da anlamının değişmesi artırır.

Şu anki sürüm: **`api: 1`**.
