# §3.1 GO / NO-GO — sonuç

**Tarih:** 2026-08-31
**Makine:** Intel HD 6000 (Broadwell GT3, 2015), 4 çekirdek, 8 GB, Wayland
**Motor:** WebKitGTK 2.52.6 (webkit2gtk-4.1), Tauri 2, 431 crate, 11 MB ikili
**Eşikler:** `ESIKLER.md` — **ölçümden önce** yazıldı.

## Karar: GO — ama koşullu

Tauri kabul edilebilir. **Koşul:** Linux'ta iki ortam değişkeni ayarlanmadan
kabul edilemez.

## Bulgu 1 — Sanallaştırılmış liste sorun değil

50.000 satır, sürekli kaydırma, düğüm havuzlu sanallaştırma:
**58.8 fps, sıfır takılan kare.** Bu ölçümün en kolay geçen kısmı.

## Bulgu 2 — Varsayılan ortamda CSS animasyonu kare hızını 2.4× düşürüyor

| Faz | Varsayılan (Wayland + DMABUF) | `GDK_BACKEND=x11` + `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
|---|---|---|
| boşta | 58.8 fps | 58.8 fps |
| kaydırma | 52.6 fps | 58.8 fps |
| kaydırma + disiplinli CSS | **23.8 fps** | **58.8 fps** |
| kaydırma + saf CSS | **18.5 fps** | **47.6 fps** |
| kaydırma + CSS + IPC 30 Hz | **23.8 fps** | **55.6 fps** |

## Bulgu 3 — Suçlu donanım değil, motorun yolu

Kontrol deneyi (`kontrol.py`): **aynı makinede, aynı sayfada, aynı GPU'da**
Firefox 154 dört fazın dördünde de **58.8 fps** — saf CSS dahil.

Bu ayrım kararı belirledi. Donanım tavanı olsaydı yerel Rust GUI'ye kaçmak da
kurtarmazdı, çünkü aynı GPU'ya çarpardı. Kontrol olmadan yanlış karar verilirdi.

## Bulgu 4 — Tek değişkenli düzeltme yanıltıcı

`WEBKIT_DISABLE_DMABUF_RENDERER=1` **tek başına kaydırmayı kötüleştiriyor**
(52.6 → 30.3 fps). `GDK_BACKEND=x11` tek başına CSS fazını kurtarmıyor
(23.8 → 26.3 fps). Yalnızca **ikisi birden** işe yarıyor.

Değişkenleri tek tek denemekle yetinen bir araştırma "bu geçici çözüm işe
yaramıyor" sonucuna varır ve NO-GO verirdi.

## Bulgu 5 — §3.2'nin korkusu bu ölçekte gerçek değil

- IPC gidiş-dönüş: **p50 1 ms, p95 1 ms, p99 2 ms** (1000 örnek).
- 200 satırlık sayfa çekme: **p50 1 ms**.
- 30 Hz çapa yoklaması kare süresine **1 ms** ekliyor.
- Rust → JS olay akışı: **~10.000–11.000 olay/sn**.

"Saniyede yüzlerce mesaj = takılma" bu motorda **doğrulanmadı**. Toplu gönderim
performans için gerekli değil.

Çapadan tahmin (§3.2) yine de doğru tasarım — ama gerekçesi performans değil:
(a) IPC duraksarsa arayüz donmaz, (b) Faz 4'ün oda primitifi zaten aynı tip
(D-015), iki ayrı pozisyon kavramı tutmaya gerek kalmaz.

## Bulgu 6 — Saf CSS hâlâ pahalı, ama artık kabul edilebilir

Düzeltilmiş ortamda bile `height` / `box-shadow` / `filter` /
`background-position` animasyonları 58.8 → 47.6 fps götürüyor; disiplinli
(`transform` + `opacity`) küme hiç düşürmüyor.

Bu bir Tauri kusuru değil, **§3.3'ün tema sözleşmesine girecek bir kısıt**:
tema yazarına hangi özellikleri canlandırabileceği söylenmezse fark
kullanıcının makinesinde ortaya çıkar.

## Tekrar üretme

```bash
cargo build --release
GDK_BACKEND=x11 WEBKIT_DISABLE_DMABUF_RENDERER=1 ./olc.sh   # Tauri
./varyant.sh                                                # 4 ortam varyantı
python3 kontrol.py firefox                                  # kontrol deneyi
```

## Sınırlar — ölçülmeyen ne var

- **Tek makine, tek sürücü.** Mesa / Broadwell. Nvidia, AMD ve daha yeni Intel
  denenmedi; düzeltmenin oralarda gerekli ya da zararsız olduğu bilinmiyor.
- `GDK_BACKEND=x11` XWayland gerektirir. XWayland'sız saf Wayland kurulumunda
  ne olacağı denenmedi.
- Pencere yeniden boyutlandırma, çoklu pencere, yüksek DPI ölçülmedi.
- Ölçüm 1100×800'de yapıldı. Tam ekran 4K'da piksel sayısı ~9× artar.
- WebKit `performance.now()`'u 1 ms'e yuvarlıyor; ms altı ayrım yok.
