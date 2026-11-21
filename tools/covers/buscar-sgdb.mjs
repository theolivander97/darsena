// Busca en SteamGridDB los juegos que se quedaron sin portada y, si el nombre coincide, los fija en covers/overrides.json.
// Uso:  node buscar-sgdb.mjs id1 id2 …   (ids de juego del catálogo)

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { loadAll } from "../catalog/cargar.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const key = readFileSync(join(HERE, ".env"), "utf8").match(/STEAMGRIDDB_API_KEY=(.+)/)[1].trim();
const overridesFile = join(HERE, "..", "..", "covers", "overrides.json");
const overrides = JSON.parse(readFileSync(overridesFile, "utf8"));
const norm = (s) => s.toLowerCase().normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/[^a-z0-9]+/g, " ").trim();
const titles = new Map(loadAll().map((g) => [g.game, g.title]));

for (const id of process.argv.slice(2)) {
  const title = titles.get(id) ?? id;
  const res = await fetch(`https://www.steamgriddb.com/api/v2/search/autocomplete/${encodeURIComponent(title)}`, { headers: { Authorization: `Bearer ${key}` } });
  const hits = (await res.json()).data ?? [];
  const want = norm(title);
  const hit = hits.find((h) => norm(h.name) === want) ?? hits.find((h) => norm(h.name).startsWith(want) || want.startsWith(norm(h.name)));
  console.log(`${id}: ${hit ? `${hit.name} (${hit.id})` : `sin coincidencia [${hits.slice(0, 3).map((h) => h.name).join(" | ")}]`}`);
  if (hit && !(id in overrides)) overrides[id] = `sgdb:${hit.id}`;
}
writeFileSync(overridesFile, JSON.stringify(overrides, null, 1));
