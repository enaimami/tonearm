// JS tarafının doğruluk kümesi koşumu (D-033).
//
// Aynı `fixtures/anchor/position_cases.json` dosyasını Rust tarafı
// `tune-core/tests/anchor_parity.rs` okuyor. İki koşum aynı sayıları
// vermezse kayma başlamış demektir.
//
// Bu betik `tune/tests/anchor_parity_js.rs` içinden çağrılıyor; tek başına
// da çalışır: `node crates/tune/tests/anchor_parity.mjs`

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { positionAt } from "../ui/anchor.js";

const burasi = dirname(fileURLToPath(import.meta.url));
const kokDizin = join(burasi, "..", "..", "..");
const kumeYolu = join(kokDizin, "fixtures", "anchor", "position_cases.json");

const kume = JSON.parse(readFileSync(kumeYolu, "utf8"));
const vakalar = kume.vakalar;

if (!Array.isArray(vakalar) || vakalar.length === 0) {
  console.error("ADIM: ANCHOR_PARITY — doğruluk kümesi boş ya da okunamadı");
  process.exit(1);
}

const hatalar = [];
for (const vaka of vakalar) {
  const bulunan = positionAt(vaka.capa, Date.parse(vaka.now));
  if (bulunan !== vaka.beklenen_ms) {
    hatalar.push(`  ${vaka.ad}\n    beklenen ${vaka.beklenen_ms}, bulunan ${bulunan}`);
  }
}

if (hatalar.length > 0) {
  console.error(`ADIM: ANCHOR_PARITY — ${hatalar.length}/${vakalar.length} vaka kaydı:`);
  console.error(hatalar.join("\n"));
  process.exit(1);
}

console.log(`${vakalar.length}/${vakalar.length} vaka geçti`);
