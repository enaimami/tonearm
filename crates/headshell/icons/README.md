# Icons

The source is a single file: `icon.svg`. The others are generated from it —
they aren't edited by hand, they are regenerated. The colours are the same as
the default theme's values (`ui/style.css`: `--headshell-bg`,
`--headshell-surface`, `--headshell-accent`); the icon is **not** part of the
theme and doesn't change when the user changes the theme.

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

The `bundle.icon` list in `tauri.conf.json` expects these names. `icon.png`
isn't on the list, but it's there for the AppImage and the window icon.

These files were once a 103-byte single-colour placeholder, and nobody had
noticed because `bundle.active` was off — it came out when packaging was
turned on.
