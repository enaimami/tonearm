// JS tarafının doğruluk kümesi koşumu (D-033).
//
// Aynı `fixtures/anchor/position_cases.json` dosyasını Rust tarafı
// `tonearm-core/tests/anchor_parity.rs` okuyor. İki koşum aynı sayıları
// vermezse kayma başlamış demektir.
//
// Bu betik `tonearm/tests/anchor_parity_js.rs` içinden çağrılıyor; tek başına
// da çalışır: `node crates/tonearm/tests/anchor_parity.mjs`

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { positionAt } from "../ui/anchor.js";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, "..", "..", "..");
const setPath = join(repoRoot, "fixtures", "anchor", "position_cases.json");

const truthSet = JSON.parse(readFileSync(setPath, "utf8"));
const cases = truthSet.cases;

if (!Array.isArray(cases) || cases.length === 0) {
  console.error("ADIM: ANCHOR_PARITY — doğruluk kümesi boş ya da okunamadı");
  process.exit(1);
}

const failures = [];
for (const entry of cases) {
  const got = positionAt(entry.anchor, Date.parse(entry.now));
  if (got !== entry.expected_ms) {
    failures.push(`  ${entry.name}\n    beklenen ${entry.expected_ms}, bulunan ${got}`);
  }
}

if (failures.length > 0) {
  console.error(`ADIM: ANCHOR_PARITY — ${failures.length}/${cases.length} vaka kaydı:`);
  console.error(failures.join("\n"));
  process.exit(1);
}

console.log(`${cases.length}/${cases.length} vaka geçti`);
