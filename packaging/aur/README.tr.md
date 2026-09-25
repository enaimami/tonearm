# AUR paketleri

> **Türkçe kopya.** Kanonik metin İngilizcedir: [`README.md`](README.md).
> Bu kopya 2026-09-25 tarihli hâlidir; İngilizce metinle birlikte güncel
> tutulacağı garanti değildir (D-073).

Arch Linux için üç PKGBUILD, dört paket:

| Dizin | AUR adı | Ne yapar |
|---|---|---|
| `headshell/` | `headshell` + `headshell-cli` | Sürüm etiketini indirir, kaynaktan derler |
| `headshell-bin/` | `headshell-bin` | Sürümün `.deb`inden masaüstü ikilisini kurar |
| `headshell-cli-bin/` | `headshell-cli-bin` | Sürümün arşivinden CLI ikilisini kurar |

**Yalnızca kaynak paket split**, ve sebebi var: iki ikili tek `cargo build`'den
çıkıyor, ayrı PKGBUILD'ler olsaydı ikisini birlikte kuran kullanıcı ~500
crate'i iki kez derlerdi. `-bin` tarafında paylaşılan bir derleme yok, o yüzden
ikisi ayrı duruyor — böylece yalnızca CLI kuran kullanıcı 10 MB'lık `.deb`i
indirmiyor, ve namcap'in `splitpkgmakedeps` hatası hiç doğmuyor (split bir
PKGBUILD'in global `makedepends`'i alt paketlerin `depends`'ini kapsamalı;
`-bin` tarafında o bağımlılıklar yalnızca çalışma zamanına ait).

`-bin` paketleri komşularını `provides`/`conflicts` ile karşılıyor, yani
`headshell` ile `headshell-bin` aynı anda kurulamaz — zaten aynı dosyaları
kurarlar.

Bu dizin paketlerin **kaynağı**; AUR'un kendi git depoları ayrıdır
(aşağıya bak). Burada tutulmasının sebebi, paketin depoyla birlikte
sürümlenmesi: `.desktop` girdisi, ikon adları ve bağımlılıklar değişince
PKGBUILD aynı commit'te değişir.

## Ortak dosyalar nereden geliyor

Masaüstü girdisi **ayrı ayrı yazılmıyor**: `headshell` ile `headshell-bin`
aynı `packaging/headshell.desktop`'u kuruyor. `-bin` paketleri bunun için
kaynak arşivini de indiriyor (1,2 MB) — ikonlar ve lisans dosyaları da oradan
geliyor; CLI arşivinin içinde yalnızca ikili var. İkinci bir elle yazılmış kopya olsaydı zamanla ayrışırdı; bu
projenin defterinde aynı arızanın üç kaydı var (D-062, D-063).

`StartupWMClass=headshell-desktop` uydurulmuş bir değer değil: Tauri'nin
`.deb` için ürettiği girdi açılıp okundu, pencere sınıfı ikili adından
türüyor. `headshell` yazılsaydı çalışan pencere başlatıcı ikonuyla
eşleşmezdi.

## Bir sürümü yayımlarken

Sıralama önemli, çünkü her adım bir sonrakinin girdisini değiştiriyor.

1. **Önce etiket, sonra PKGBUILD.** `source=` etiketin arşivine bakıyor ve
   `-bin` sürüm varlıklarına; ikisi de etiket atılmadan var olmaz.
2. **Sürüm varlıkları yayımlanmış olmalı.** `release.yml` **taslak** bir
   sürüm açıyor ve taslağın varlıkları anonim indirilemez — yalnızca depoya
   erişimi olan görür. `-bin` paketi bu yüzden sürüm taslaktan çıkmadan
   çalışmaz. Kaynak paketin böyle bir borcu yok: etiket arşivi taslaktan
   bağımsız, depo açık olduğu sürece herkese açıktır.
3. **`pkgver`'i güncelle.** Tek satır: `pkgver=X.Y.Z`. Arch sürümünde `-`
   pkgrel ayıracı olduğu için ön sürümlerde `_` yazılır (`0.0.1_beta`),
   yukarı akıştaki yazım `_pkgver` ile türetilir. `pkgrel=1`'e döndür.
4. **Sağlama toplamlarını tazele:**
   ```bash
   updpkgsums          # pacman-contrib
   ```
5. **`.SRCINFO` üret** (AUR bunu zorunlu tutar, ve elle yazılmaz):
   ```bash
   makepkg --printsrcinfo > .SRCINFO
   ```
6. **Sına:**
   ```bash
   makepkg -si --clean
   namcap PKGBUILD *.pkg.tar.zst
   ```
7. **AUR'a it:**
   ```bash
   git clone ssh://aur@aur.archlinux.org/headshell.git aur-headshell
   cp PKGBUILD aur-headshell/
   cd aur-headshell && makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO && git commit && git push
   ```
   Üç AUR deposu var — `headshell` (split, iki paket), `headshell-bin`,
   `headshell-cli-bin`. Sürüm üçünde de yükselir.

`.SRCINFO` **bu depoda tutulmuyor**: AUR deposunda yaşıyor ve orada
`makepkg`'den üretiliyor. Burada bir kopyası olsaydı PKGBUILD değişince
sessizce bayatlardı.

## Konteynerde sınamak

PKGBUILD'ler **depo kökünde değil**, yukarıdaki dizinlerin altında — kökte
`makepkg` koşarsan "PKGBUILD mevcut değil" dersin. Depo kökündeki `Makefile`
doğru dizine kendisi giriyor:

```bash
make aur-test PKG=headshell
make aur-test PKG=headshell-bin
make aur-test PKG=headshell-cli-bin
make aur-clean                     # artıkları sil
```

Arch makinesinde `makepkg` **doğrudan** koşar; `makepkg` olmayan bir makinede
aynı iş bir Arch konteynerinde yapılır (bu Makefile Debian'da yazıldı).
Seçim otomatik, zorlamak için `ENGINE=native` ya da `ENGINE=container`.
Yerel koşum `pacman-contrib` (`updpkgsums`) ve `namcap` istiyor; yoksa hedef
ne eksik olduğunu ve ne yazacağını söyleyip duruyor.

Hedef şunları sırayla yapıyor: çalışma ağacından **etiket-eşi** bir kaynak
arşivi üretir, `-bin` paketleri için sürüm varlıklarını `gh` ile çeker,
hepsini `target/aur/<paket>/` altına kopyalar, konteynerde `updpkgsums` ile
toplamları o yerel dosyalara göre tazeler, `makepkg` koşar ve `namcap`'ler.
**Depodaki PKGBUILD'e dokunmaz** — o gerçek etiketin toplamlarını taşımaya
devam eder.

Etiket-eşi arşiv şunun için gerekli: `source=` etikete bakıyor, oysa sınamak
istediğin şey henüz itilmemiş çalışma ağacın. Arşiv PKGBUILD'in yanında
durunca `makepkg` indirmeyi atlar.

Elle koşacaksan iki tuzak var: `makepkg` root koşmaz ve `-s` `sudo` ister —
konteynerde makedepends'i önce root olarak kur. Ve konteynerde `fakeroot`un
bıraktığı dosyalar ana makinede root'a ait olur; temizlik
`podman unshare rm -rf` ile (`make aur-clean` ikisini de deniyor).

## Neden `cargo tauri build` değil

`cargo tauri build` bir bundler: `.deb`, `.rpm`, `.AppImage` üretir. Arch
paketini zaten `makepkg` kuruyor, yani bundler'ın işi tekrar olurdu — ve
`tauri-cli` bir yapım bağımlılığı olarak gelirdi. Düz `cargo build` yeter;
karşılığında `.desktop` girdisini ve ikonları PKGBUILD elle kuruyor.

## Eklentilerin çalışma zamanı bağımlılığı yok

`python` ve `yt-dlp` `optdepends`'ten çıktı (D-069): eklenti motoru (QuickJS)
ikilinin içinde, ve YouTube Music eklentisinin istediği yt-dlp'yi motor,
platformun kendi kendine yeten ikilisi olarak kullanıcının veri dizinine
indiriyor. Sistemin `yt-dlp` paketi kullanılmıyor — sürümü manifest
sabitliyor ve karması doğrulanıyor.

Torrent eklentisi park edildi (`parked/`, D-069) ve hiçbir pakete girmiyor.
