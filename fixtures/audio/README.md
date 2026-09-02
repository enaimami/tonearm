# `fixtures/audio/`

Buradaki ses dosyalarının **hiçbiri gerçek müzik değildir.** Hepsi sentetik
olarak üretildi ya da elle yazıldı; depo hiçbir telifli kaydı taşımıyor ve
taşımamalı. Yeni bir ses fixture'ı eklerken kuralı koru: üretim komutunu
buraya yaz, üretilemeyen bir ikiliyi commit etme.

| Dosya | Ne için | Nasıl üretildi |
|---|---|---|
| `fingerprint_sample.flac` | Chromaprint parmak izi (D-045) | aşağıdaki `ffmpeg` komutu |
| `Test Artist - Mp3 Track.mp3` | dosya adından etiket çıkarma; parmak izi için **kasten çok kısa** | 2 sn sentetik ton |
| `Other Artist - Ogg Track.ogg` | kap çeşitliliği | sentetik ton |
| `tagged.flac` | gömülü etiketlerin okunması | sentetik ton + Vorbis yorumları |
| `corrupt.flac` | çözücünün aşamayı bildirmesi | geçerli olmayan 24 bayt |
| `cover.jpg` | kapak görseli yolu | 5 baytlık yer tutucu |
| `Dir Artist/` | dizin yapısından çıkarım | boş dizin ağacı |

## `fingerprint_sample.flac`

15 saniye, 44.1 kHz, mono, 16 bit. Her 2.5 saniyede bir yarım ses yukarı
kayan dört sesli bir akor — sabit bir ton chroma özelliklerini düz bırakır,
gürültü ise parmak izini anlamsızlaştırır; ikisinin arasında duran tonal ama
değişen bir sinyal gerekiyordu.

Uzunluk kaza değil: Chromaprint ilk parmak izi öğesini üretmek için birkaç
saniyelik pencere ister, bu yüzden 2 saniyelik `Test Artist - Mp3 Track.mp3`
"çok kısa" hatası verir — o dosya artık bu sınırın testi.

```sh
ffmpeg -y \
  -f lavfi -i "aevalsrc='\
0.32*sin(2*PI*(220*pow(2,(2*floor(t/2.5))/12))*t) + \
0.26*sin(2*PI*(220*pow(2,(4+2*floor(t/2.5))/12))*t) + \
0.20*sin(2*PI*(220*pow(2,(7+2*floor(t/2.5))/12))*t) + \
0.10*sin(2*PI*(220*pow(2,(12+2*floor(t/2.5))/12))*t)':d=15:s=44100:c=mono" \
  -sample_fmt s16 "fixtures/audio/fingerprint_sample.flac"
```

Sentetik olduğu için AcoustID veritabanında **karşılığı yoktur** ve olmaması
gerekir: canlı AcoustID testi "eşleşme bulunamadı" ile "servise ulaşılamadı"
ayrımını sınar (K9), bilinen bir parçayı tanımayı değil.
