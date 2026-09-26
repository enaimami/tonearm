# `fixtures/artwork/`

Audio files for the cover chain (D-076). Like everything under `fixtures/`,
**nothing here is real music or real art**: the audio is the synthetic tone
of `fixtures/audio/` or digital silence, the covers are gradients drawn by
`ffmpeg`. One file carries a real recording's tags and no picture: the live
test (`tests/artwork_online.rs`) asks MusicBrainz and the Cover Art Archive
for its album's cover.

| File | Tags | Cover | What for |
|---|---|---|---|
| `Cover Artist - Flac Front.flac` | Cover Artist · Flac Front · *Covered* | a FLAC `PICTURE` block, front cover, 400×400 PNG | reading a FLAC picture; resizing a PNG to 320 and 96 px |
| `Cover Artist - Mp3 Front.mp3` | Cover Artist · Mp3 Front · *Covered Too* | an ID3v2.3 `APIC` frame, front cover, 200×200 JPEG | reading an ID3 picture; decoding a JPEG, keeping it for the label and resizing it to 96 px |
| `Portishead - Sour Times.flac` | Portishead · Sour Times · *Dummy* — a real recording's tags | none | the live test: the album's cover from the Cover Art Archive, not a compilation's; 251 s of silence, the recording's real length |

They were produced with ffmpeg 7.1 (Debian 13) from `fixtures/audio/`:

```sh
cd fixtures/artwork
ffmpeg -y -f lavfi -i "gradients=s=400x400:c0=0xffb454:c1=0x2e2925:x0=0:y0=0:x1=399:y1=399:n=2" \
  -frames:v 1 /tmp/cover-400.png
ffmpeg -y -f lavfi -i "gradients=s=200x200:c0=0x2ec4b6:c1=0x6a4c93:x0=0:y0=199:x1=199:y1=0:n=2" \
  -frames:v 1 -q:v 3 /tmp/cover-200.jpg
ffmpeg -y -i ../audio/tagged.flac -i /tmp/cover-400.png -map 0:a -map 1:v -c copy \
  -disposition:v attached_pic -metadata:s:v comment="Cover (front)" \
  -metadata artist="Cover Artist" -metadata album="Covered" -metadata title="Flac Front" \
  "Cover Artist - Flac Front.flac"
ffmpeg -y -i "../audio/Test Artist - Mp3 Track.mp3" -i /tmp/cover-200.jpg -map 0:a -map 1:v -c copy \
  -id3v2_version 3 -disposition:v attached_pic -metadata:s:v comment="Cover (front)" \
  -metadata artist="Cover Artist" -metadata album="Covered Too" -metadata title="Mp3 Front" \
  "Cover Artist - Mp3 Front.mp3"
```

`comment="Cover (front)"` is what makes ffmpeg write picture type 3 (front
cover) in both formats. The silent one:

```sh
ffmpeg -y -f lavfi -i "anullsrc=r=44100:cl=mono:d=251" -c:a flac -compression_level 8 \
  -metadata artist="Portishead" -metadata title="Sour Times" -metadata album="Dummy" \
  "Portishead - Sour Times.flac"
```
