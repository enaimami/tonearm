// The test plugin: the smallest example that implements the plugin contract
// (api 2) **in full**. It never goes online and answers from a fixed catalog.
//
// There is no misbehaving-plugin act here — the timeout, throwing, unpermitted
// network and missing export tests write their own small scripts. This file
// stays clean as the working form of the skeleton in `docs/writing-plugins.md`.

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
    // Malformed on purpose: the core must drop and count it, not accept it.
    isrc: "bogus",
  },
];

// Search must treat "ezhel" and "EZHEL" as the same. QuickJS has no `Intl`;
// locale-aware folding, if it were needed, would have to be written by hand.
const fold = (text) => text.toLowerCase();

export function health() {
  return {
    reachable: true,
    track_count: CATALOG.length,
    // The secret's presence is reported, not its **value**.
    detail: host.secrets.get("token") === null ? "no secret" : "has secret",
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
    // "None" is an answer, not an error.
    return null;
  }
  return {
    kind: "http_stream",
    url: `https://example.invalid/${id}.mp3`,
    headers: [{ name: "authorization", value: host.secrets.get("token") ?? "" }],
  };
}
