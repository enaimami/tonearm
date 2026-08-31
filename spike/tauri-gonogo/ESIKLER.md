# GO / NO-GO eşikleri

**Bu dosya ölçümden ÖNCE yazıldı.** Sebep: eşiği sonuçları gördükten sonra
koymak ölçüm değil, kararı ölçüme uydurmaktır.

Ölçüm makinesi bilerek zayıf: **Intel HD 6000 (Broadwell GT3, 2015), 4 çekirdek,
8 GB RAM, Wayland, WebKitGTK 2.52.6.** Hedef kitle Linux ağırlıklı ve WebKitGTK
üç platformun en zayıfı (PLAN §3.1). Burada geçen genelde geçer; burada kalan
"yeni makinede iyiydi" savunmasını hak etmez.

| Ölçü | GO | SINIRDA | NO-GO |
|---|---|---|---|
| Kaydırma + CSS, medyan kare | ≤ 18 ms (≈55 fps) | ≤ 25 ms (≈40 fps) | > 25 ms |
| Kaydırma + CSS, p95 kare | ≤ 25 ms | ≤ 40 ms | > 40 ms |
| Kaydırma + CSS, en kötü kare | ≤ 120 ms | ≤ 300 ms | > 300 ms |
| IPC gidiş-dönüş p95 | ≤ 5 ms | ≤ 15 ms | > 15 ms |
| 30 Hz IPC'nin kareye maliyeti | medyan artışı ≤ 2 ms | ≤ 5 ms | > 5 ms |
| Olay akışı (Rust → JS) | ≥ 2000 olay/sn | ≥ 500 olay/sn | < 500 olay/sn |
| Pencere görünene kadar | ≤ 1500 ms | ≤ 3000 ms | > 3000 ms |
| RSS tepe (50k satır yüklü) | ≤ 250 MB | ≤ 400 MB | > 400 MB |

## Neden bu sayılar

- **Kare süresi medyanı değil p95'i belirleyicidir.** Kullanıcı ortalamayı
  hissetmez, takılmayı hisseter. 25 ms'lik p95, kaydırma sırasında saniyede
  birkaç kez fark edilen bir sarsıntı demektir — sınırda kabul.
- **IPC gidiş-dönüşü zaten sıcak yolda olmamalı.** §3.2 pozisyonun çapadan
  **tahmin edileceğini** söylüyor; IPC yalnızca kullanıcı eylemlerinde (çal,
  duraklat, ara) devrede. 5 ms bu yüzden cömert bir eşik, dar değil.
- **30 Hz IPC ölçülüyor çünkü §3.2'nin korkusu tam olarak bu.** Ölçüm bu korkuyu
  ya doğrular ya da "toplu gönderim gereksiz" der. İkisi de kullanılır bilgi.
- **Olay akışı eşiği düşük tutuldu** (2000/sn), çünkü tasarım zaten saniyede
  yüzlerce mesaj göndermemeyi hedefliyor. Buradaki sayı hedef değil, tavan
  bilgisi: tavanı bilmeden "toplu gönderim" tasarlanamaz.

## Karar kuralı

- Hepsi **GO** → Tauri seçilir, §3.2'ye geçilir.
- Bir veya iki ölçü **SINIRDA**, kalanı GO → Tauri seçilir ama sınırdaki ölçü
  §3.2'nin tasarım kısıtı olarak yazılır.
- Herhangi biri **NO-GO** → durulur ve sorulur. PLAN'ın alternatifi
  "Dioxus / yerel Rust GUI" diyor; **ama Dioxus desktop da WebKitGTK kullanır**
  (wry üzerinden). Bu ölçüm başarısız olursa gerçek alternatif webview değil,
  yerel çizen bir yığındır (egui / Dioxus native) — ve bedeli CSS tema
  ekosistemini kaybetmektir. Bu seçenek benim değil, kullanıcının.
