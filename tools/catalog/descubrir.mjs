// Busca en GitHub ports de juegos que todavía no están en el catálogo y los deja en catalog/candidatos.txt
// para revisarlos a mano (solo juegos: nada de herramientas, motores ni emuladores).
//
// Uso:  node tools\catalog\descubrir.mjs [estrellas mínimas, por defecto 25]
//
// Cada búsqueda mira un tema (topic) o unas palabras de GitHub. De cada repositorio se comprueba que tenga una release
// con una build de Windows y se descarta lo que parezca una herramienta. Lo que sale es una lista de candidatos:
// la decisión de añadirlos es de una persona.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gh, ROOT } from "../comun.mjs";

const MIN_STARS = Number(process.argv[2] ?? 25);
const catalog = JSON.parse(readFileSync(join(ROOT, "catalog", "catalog.json"), "utf8")).games;
const known = new Set(catalog.map((g) => g.repo?.toLowerCase()).filter(Boolean));

const SEARCHES = [
  "topic:decompilation",
  "topic:static-recompilation",
  "topic:recompilation",
  "topic:pc-port",
  "topic:game-port",
  "topic:decomp",
  "topic:reverse-engineering topic:game pc port",
  "static recompilation game port in:description",
  "native pc port decompilation in:description",
  "recompiled pc port in:description",
];

// Cosas que no son un juego: se descartan por su descripción o nombre.
const NOT_A_GAME = /\b(emulator|emulador|library|libreria|framework|engine|sdk|toolkit|toolchain|compiler|disassembler|decompiler tool|editor|extractor|unpacker|modding tool|mod manager|launcher|loader|libretro|plugin|wrapper|translator|lifter|analy[sz]er|runtime library|template|boilerplate)\b/i;

const pick = (name) => {
  const n = name.toLowerCase();
  return n.endsWith(".zip") || n.endsWith(".exe") || n.endsWith(".7z");
};
const windows = (name) => pick(name) && !/linux|macos|osx|android|\.apk|arm64|aarch64|source|src|switch|vita|ios/i.test(name);

const found = new Map();
for (const q of SEARCHES) {
  for (let page = 1; page <= 3; page++) {
    let res;
    try {
      res = await gh(`/search/repositories?q=${encodeURIComponent(`${q} stars:>=${MIN_STARS} archived:false fork:false`)}&sort=stars&per_page=50&page=${page}`);
    } catch (e) {
      console.log(`  (${q}: ${e.message.slice(0, 80)})`);
      break;
    }
    for (const r of res.items) if (!found.has(r.full_name)) found.set(r.full_name, r);
    if (res.items.length < 50) break;
    await new Promise((r) => setTimeout(r, 2200)); // la búsqueda de GitHub limita a ~30 peticiones por minuto
  }
  console.log(`${q}: ${found.size} repositorios en total`);
}

const rows = [];
let n = 0;
for (const r of found.values()) {
  n++;
  if (known.has(r.full_name.toLowerCase())) continue;
  const text = `${r.name} ${r.description ?? ""}`;
  if (NOT_A_GAME.test(text)) continue;
  let build = null;
  try {
    const releases = await gh(`/repos/${r.full_name}/releases?per_page=5`);
    for (const rel of releases) {
      if (rel.draft) continue;
      const asset = rel.assets.find((a) => windows(a.name));
      if (asset) { build = `${rel.tag_name} (${asset.name})`; break; }
    }
  } catch {}
  rows.push({ r, build });
  if (n % 25 === 0) console.log(`  comprobados ${n}/${found.size}`);
}

rows.sort((a, b) => b.r.stargazers_count - a.r.stargazers_count);
const line = ({ r, build }) =>
  `${r.full_name}  *${r.stargazers_count}  ${r.pushed_at.slice(0, 10)}\n    ${(r.description ?? "(sin descripción)").slice(0, 150)}\n    ${r.html_url}`;
const withBuild = rows.filter((x) => x.build);
const withoutBuild = rows.filter((x) => !x.build);
const out = [
  `Candidatos a entrar en el catálogo (${new Date().toISOString().slice(0, 10)}, mínimo ${MIN_STARS} estrellas)`,
  `Con build de Windows en sus releases: ${withBuild.length}. Sin build todavía: ${withoutBuild.length}.`,
  "",
  "=== CON BUILD DE WINDOWS ===",
  ...withBuild.map((x) => `${line(x)}\n    build: ${x.build}\n`),
  "=== SIN BUILD EN RELEASES (vigilar) ===",
  ...withoutBuild.map((x) => `${line(x)}\n`),
].join("\n");
writeFileSync(join(ROOT, "catalog", "candidatos.txt"), out);
console.log(`\nOK: ${withBuild.length} candidatos con build y ${withoutBuild.length} sin build -> catalog/candidatos.txt`);
