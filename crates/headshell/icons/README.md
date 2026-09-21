# İkonlar

Kaynak tek dosya: `icon.svg`. Ötekiler ondan üretilir — elle düzenlenmez,
yeniden üretilir. Renkler varsayılan temanın değerleriyle aynı
(`ui/style.css`: `--headshell-bg`, `--headshell-surface`, `--headshell-accent`); ikon temanın
parçası **değildir**, kullanıcı tema değiştirince değişmez.

```sh
cd crates/headshell/icons
rsvg-convert -w 32  -h 32  icon.svg -o 32x32.png
rsvg-convert -w 128 -h 128 icon.svg -o 128x128.png
rsvg-convert -w 256 -h 256 icon.svg -o '128x128@2x.png'
rsvg-convert -w 512 -h 512 icon.svg -o icon.png
python3 -c "
from PIL import Image
src = Image.open('icon.png')
src.save('icon.ico', sizes=[(16,16),(32,32),(48,48),(64,64),(128,128),(256,256)])
src.resize((1024, 1024), Image.LANCZOS).save('icon.icns')
"
```

`tauri.conf.json`'daki `bundle.icon` listesi bu adları bekler. `icon.png`
listede yok ama AppImage ve pencere ikonu için duruyor.

Bu dosyalar bir zamanlar 103 baytlık tek renkli bir yer tutucuydu ve
`bundle.active` kapalı olduğu için kimse fark etmemişti — paketleme açılınca
ortaya çıktı.
