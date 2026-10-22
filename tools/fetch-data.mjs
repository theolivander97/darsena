// Baja los datos que Dársena incrusta al compilar (catálogo, información de juegos e índice de portadas).
// Viven en otros repositorios; este script los deja en catalog/ y covers/ para que `tauri build` los encuentre.
// Uso: node tools/fetch-data.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const FILES = [
  ["https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/catalog.json", "catalog/catalog.json"],
  ["https://raw.githubusercontent.com/theolivander97/darsena-launcher/main/info.json", "catalog/info.json"],
  ["https://cdn.jsdelivr.net/gh/theolivander97/portlauncher-covers@main/index.json", "covers/index.json"],
];

for (const [url, dest] of FILES) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status}`);
  const text = await res.text();
  JSON.parse(text); // que sea JSON válido antes de guardarlo
  mkdirSync(dirname(join(ROOT, dest)), { recursive: true });
  writeFileSync(join(ROOT, dest), text);
  console.log(`✔ ${dest} (${(text.length / 1024).toFixed(0)} KB)`);
}
