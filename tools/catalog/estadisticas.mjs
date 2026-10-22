// Actualiza el campo "downloads" de cada port del catálogo con las descargas totales de sus releases
// en GitHub. Alimenta "Más descargados" en Explorar.
//
// Uso:  node tools\catalog\estadisticas.mjs        (necesita GITHUB_TOKEN en tools/.env)

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { graphql, ROOT, TOOLS } from "../comun.mjs";

const catalogFile = join(ROOT, "catalog", "catalog.json");
const catalog = JSON.parse(readFileSync(catalogFile, "utf8"));
const BATCH = 40;

const q = (s) => JSON.stringify(s);
let updated = 0;
const missing = [];

// Los ports externos (no están en GitHub) no tienen estadísticas de descargas.
const onGithub = catalog.games.filter((g) => g.origin !== "external" && g.repo);
for (let i = 0; i < onGithub.length; i += BATCH) {
  const batch = onGithub.slice(i, i + BATCH);
  const fields = batch.map((g, k) => {
    const [owner, name] = g.repo.split("/");
    return `r${k}: repository(owner: ${q(owner)}, name: ${q(name)}) {
      releases(first: 50, orderBy: {field: CREATED_AT, direction: DESC}) {
        nodes { releaseAssets(first: 40) { nodes { downloadCount } } }
      }
    }`;
  });
  let data;
  try {
    data = await graphql(`query { ${fields.join("\n")} }`);
  } catch (e) {
    if (process.env.DEBUG) console.log("lote:", e.message.slice(0, 300));
    // Si el lote falla (p. ej. un repo borrado rompe la consulta entera), se pide de uno en uno.
    data = {};
    for (let k = 0; k < batch.length; k++) {
      try { Object.assign(data, await graphql(`query { ${fields[k]} }`)); } catch (e) { if (process.env.DEBUG) console.log(batch[k].repo, e.message.slice(0, 200)); data[`r${k}`] = null; }
    }
  }
  batch.forEach((g, k) => {
    const repo = data[`r${k}`];
    if (!repo) {
      missing.push(g.repo);
      return;
    }
    const total = (repo.releases?.nodes ?? []).reduce(
      (a, r) => a + (r?.releaseAssets?.nodes ?? []).reduce((b, x) => b + (x?.downloadCount ?? 0), 0), 0);
    if (g.downloads !== total) updated++;
    g.downloads = total;
  });
  await new Promise((r) => setTimeout(r, 1500)); // sin prisas: evita el límite secundario de GitHub
  process.stdout.write(`\r${Math.min(i + BATCH, onGithub.length)}/${onGithub.length} ports consultados`);
}

writeFileSync(catalogFile, JSON.stringify(catalog));
// Reordena y valida con el formato de siempre.
execFileSync("node", [join(TOOLS, "catalog", "actualizar.mjs")], { stdio: "inherit" });

const top = [...catalog.games].sort((a, b) => (b.downloads ?? 0) - (a.downloads ?? 0)).slice(0, 10);
console.log(`\n✔ Descargas actualizadas en ${updated} ports.`);
console.log("Top 10:\n" + top.map((g, n) => `  ${String(n + 1).padStart(2)}. ${g.title} (${g.port}) — ${g.downloads.toLocaleString("es")}`).join("\n"));
if (missing.length) console.log(`\n⚠ ${missing.length} repos no encontrados (borrados o renombrados):\n  ${missing.join("\n  ")}`);
