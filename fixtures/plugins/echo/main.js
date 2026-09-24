// Sınama eklentisi: eklenti sözleşmesini (api 2) **eksiksiz** uygulayan en
// küçük örnek. Ağa çıkmaz, sabit bir katalogla cevap verir.
//
// Kötü eklenti taklidi burada yok — zaman aşımı, fırlatma, izinsiz ağ ve
// eksik dışa aktarım testleri kendi küçük betiklerini yazıyor. Bu dosya
// `docs/eklenti-yazma.md`'deki iskeletin çalışan hâli olarak temiz kalıyor.

const CATALOG = [
  {
    id: "track-1",
    artist: "Ezhel",
    title: "Geceler",
    album: "Müptezhel",
    duration_ms: 213000,
    isrc: "TR1234567890",
  },
  {
    id: "track-2",
    artist: "Sezen Aksu",
    title: "Gülümse",
    album: "Gülümse",
    duration_ms: 254000,
    // Bilerek biçimsiz: çekirdek bunu düşürüp saymalı, kabul etmemeli.
    isrc: "uydurma",
  },
];

// Arama "ezhel" ile "EZHEL"i aynı saymalı. QuickJS'te `Intl` yok; yerel
// ayara duyarlı katlama gerekseydi elle yazılması gerekirdi.
const fold = (text) => text.toLowerCase();

export function health() {
  return {
    reachable: true,
    track_count: CATALOG.length,
    // Sırrın **değeri** değil, varlığı raporlanıyor.
    detail: host.secrets.get("token") === null ? "sır yok" : "sır var",
  };
}

export function search(query, limit) {
  const needle = fold(String(query));
  return CATALOG.filter(
    (track) => fold(track.artist).includes(needle) || fold(track.title).includes(needle),
  ).slice(0, limit);
}

export function resolve_source(id) {
  if (!CATALOG.some((track) => track.id === id)) {
    // "Yok" bir cevaptır, hata değil.
    return null;
  }
  return {
    kind: "http_stream",
    url: `https://ornek.gecersiz/${id}.mp3`,
    headers: [{ name: "authorization", value: host.secrets.get("token") ?? "" }],
  };
}
