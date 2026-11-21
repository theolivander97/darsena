// Busca juegos web nuevos en la lista pública de GameWave (https://gamewave.digital/ports/webgames.json).
// Solo se aceptan los que tienen un repositorio de GitHub con código real (historial, forks, sin archivar), cuya página responde
// y que no sirven un juego comercial entero desde su servidor: abren directo con contenido libre o piden tus archivos.
//
// Uso:  node tools\catalog\gamewave-web.mjs     ->  catalog/candidatos-web.json (para revisar a mano)

import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { graphql, ROOT } from "../comun.mjs";
import { loadAll } from "./cargar.mjs";

const all = loadAll();
const knownRepos = new Set(all.map((g) => g.repo?.toLowerCase()).filter(Boolean));
const knownHosts = new Set(all.filter((g) => g.web).map((g) => new URL(g.web).host));

const res = await fetch("https://gamewave.digital/ports/webgames.json");
const apps = (await res.json()).apps;
const rows = [];
for (const a of apps) {
  if (a.enabled === false || !["instant", "own-files"].includes(a.playMode)) continue;
  const m = (a.repository || "").trim().match(/^(?:https?:\/\/github\.com\/)?([\w.-]+\/[\w.-]+?)(?:\.git)?\/?$/);
  if (!m || !a.playUrl?.startsWith("https://")) continue;
  if (knownRepos.has(m[1].toLowerCase()) || knownHosts.has(new URL(a.playUrl).host)) continue;
  rows.push({ repo: m[1], name: a.name.replace(/�/g, "—"), playUrl: a.playUrl, playMode: a.playMode, platform: a.sourcePlatform, required: a.requiredFiles ?? "" });
}
console.log(`${apps.length} juegos web en GameWave -> ${rows.length} con repositorio y página https, aún sin añadir`);

const out = [];
for (let i = 0; i < rows.length; i += 40) {
  const batch = rows.slice(i, i + 40);
  const q = `{${batch.map((r, k) => {
    const [owner, name] = r.repo.split("/");
    return `r${k}: repository(owner: ${JSON.stringify(owner)}, name: ${JSON.stringify(name)}) { stargazerCount isArchived forkCount description primaryLanguage { name } createdAt defaultBranchRef { target { ... on Commit { history { totalCount } } } } }`;
  }).join("\n")}}`;
  let data;
  try { data = await graphql(q); } catch (e) { console.log(`  lote ${i}: ${e.message.slice(0, 80)}`); continue; }
  for (const [k, r] of batch.entries()) {
    const g = data[`r${k}`];
    if (!g || g.isArchived) continue;
    const commits = g.defaultBranchRef?.target?.history?.totalCount ?? 0;
    const ageDays = Math.floor((Date.now() - Date.parse(g.createdAt)) / 86400000);
    const solid = ageDays >= 60 && commits >= 20 && !!g.primaryLanguage && g.forkCount >= 2 && g.stargazerCount >= 10;
    out.push({ ...r, stars: g.stargazerCount, forks: g.forkCount, commits, ageDays, solid, description: (g.description ?? "").slice(0, 120) });
  }
}

// La página tiene que responder de verdad.
const alive = [];
await Promise.all(out.filter((c) => c.solid).map(async (c) => {
  try {
    const r = await fetch(c.playUrl, { method: "GET", redirect: "follow", signal: AbortSignal.timeout(15000) });
    if (r.ok) alive.push({ ...c, status: r.status });
  } catch {}
}));
alive.sort((a, b) => b.stars - a.stars);
writeFileSync(join(ROOT, "catalog", "candidatos-web.json"), JSON.stringify(alive, null, 1));
console.log(`OK: ${alive.length} candidatos con código sólido y página que responde -> catalog/candidatos-web.json`);
for (const c of alive) console.log(`${String(c.stars).padStart(5)}★ ${c.playMode.padEnd(9)} ${c.name.slice(0, 44).padEnd(44)} ${c.repo}`);
