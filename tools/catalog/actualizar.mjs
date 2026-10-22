// Actualiza el catálogo después de editarlo a mano.
//
//   1. Comprueba que catalog/catalog.json es JSON válido (si no, dice en qué línea está el error).
//   2. Valida cada port: campos obligatorios, ids repetidos, regex del asset…
//   3. Rellena el id si falta, y el campo "game" (a qué juego pertenece cada port).
//   4. Ordena por título y guarda con un port por línea (fácil de editar y de ver en git).
//
// Uso:  node tools\catalog\actualizar.mjs
// Correcciones de agrupación a mano: catalog/games-overrides.json  { "<id del port>": "<id del juego>" }

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const catalogFile = join(ROOT, "catalog", "catalog.json");
const overridesFile = join(ROOT, "catalog", "games-overrides.json");

const CONSOLES = [
  "Nintendo 64", "Super Nintendo", "NES", "Game Boy", "Game Boy Color", "Game Boy Advance", "Nintendo DS", "Nintendo 3DS",
  "GameCube", "Wii", "Wii U", "Switch", "Virtual Boy", "PlayStation", "PlayStation 2", "PlayStation 3",
  "PlayStation 4", "PSP", "Xbox", "Xbox 360", "Sega", "Arcade", "PC", "Otros",
];
const PLATFORMS = ["Windows", "Linux", "macOS"];
// Orden de los campos al guardar (los desconocidos van al final).
const FIELD_ORDER = ["id", "title", "port", "author", "origin", "repo", "gitlab", "homepage", "console", "consoles", "franchise", "aliases", "asset", "exe", "branch", "setup", "requirements", "saves",
  "experimental", "ai", "platforms", "needs", "needsNote", "addedAt", "downloads", "game"];
const TODAY = new Date().toISOString().slice(0, 10);

function fail(msg) {
  console.error(`\nError: ${msg}\n`);
  process.exit(1);
}

// ---------- 1. Leer con errores claros ----------
const text = readFileSync(catalogFile, "utf8");
let catalog;
try {
  catalog = JSON.parse(text);
} catch (e) {
  const pos = Number(e.message.match(/position (\d+)/)?.[1]);
  let where = "";
  if (!Number.isNaN(pos)) {
    const before = text.slice(0, pos);
    const line = before.split("\n").length;
    const lineText = text.split("\n")[line - 1] ?? "";
    where = `\n  Línea ${line}: ${lineText.trim().slice(0, 160)}` +
      `\n  Revisa esa línea y el final de la anterior: suele ser una coma que falta o sobra entre juegos, o comillas sin cerrar.`;
  }
  fail(`catalog.json no es JSON válido: ${e.message}${where}`);
}
if (!Array.isArray(catalog.games)) fail(`catalog.json debe tener una lista "games"`);

// ---------- 2. Validar ----------
const slug = (s) => s.normalize("NFD").replace(/[̀-ͯ]/g, "")
  .replace(/·.*$/, " ").replace(/\([^)]*\)/g, " ")
  .replace(/['’`´]/g, "").replace(/&/g, " and ")
  .toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "juego";
const repoId = (repo) => repo.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

const errors = [];
const warnings = [];
const seenIds = new Map();
const seenRepos = new Map();

catalog.games.forEach((g, i) => {
  const name = g.title || g.repo || `juego nº ${i + 1}`;
  const err = (m) => errors.push(`${name}: ${m}`);

  // "origin": "github" (por defecto, releases del repo) o "external" (no está en GitHub: solo desde fuentes).
  if (g.origin != null && !["github", "external"].includes(g.origin)) err(`"origin" debe ser "github" o "external" (es "${g.origin}")`);
  // "gitlab": "grupo/proyecto" de gitlab.com. Se guarda como origin "external" (las versiones antiguas de Dársena no
  // conocen GitLab y lo tratarán como un port que necesita una fuente); las nuevas lo bajan de sus releases.
  if (g.gitlab != null) {
    if (!/^[\w.-]+(\/[\w.-]+)+$/.test(g.gitlab)) err(`"gitlab" debe ser "grupo/proyecto" de gitlab.com (es "${g.gitlab}")`);
    g.origin = "external";
    const parts = g.gitlab.split("/");
    g.port ||= parts.at(-1);
    g.author ||= parts[0];
    g.homepage ||= `https://gitlab.com/${g.gitlab}`;
  }
  // "branch": el port es el código de esa rama (sin builds en releases). Como con GitLab, se guarda como
  // origin "external" para que las versiones antiguas de Dársena no instalen por error lo que haya en sus releases.
  // "setup": { exe, done: [...] }: hasta que exista algo de "done", JUGAR abre "exe" (p. ej. el instalador que recompila el juego).
  if (g.branch != null) {
    if (!/^[\w./-]+$/.test(g.branch) || !g.repo) err(`"branch" necesita "repo" y un nombre de rama válido`);
    g.origin = "external";
    g.homepage ||= `https://github.com/${g.repo}`;
    g.port ||= g.repo?.split("/")[1];
    g.author ||= g.repo?.split("/")[0];
  }
  if (g.setup != null && (typeof g.setup.exe !== "string" || !Array.isArray(g.setup.done) || !g.setup.done.length)) err(`"setup" debe ser { "exe": "...", "done": ["...", ...] }`);
  // "requirements": { minimum, recommended }, cada uno con os, cpu, ram, gpu, storage, other (texto o { es, en }).
  if (g.requirements != null) {
    const KEYS = ["os", "cpu", "ram", "gpu", "storage", "other"];
    const ok = (v) => typeof v === "string" || (v && typeof v === "object" && ["es", "en"].some((l) => typeof v[l] === "string"));
    for (const [level, set] of Object.entries(g.requirements)) {
      if (!["minimum", "recommended"].includes(level) || typeof set !== "object") { err(`"requirements" solo admite "minimum" y "recommended"`); continue; }
      for (const [k, v] of Object.entries(set)) if (!KEYS.includes(k) || !ok(v)) err(`"requirements.${level}.${k}" no es válido (claves: ${KEYS.join(", ")})`);
    }
  }
  // "saves": dónde guarda las partidas (para la nube): rutas relativas al port o con %APPDATA%, %LOCALAPPDATA%…
  if (g.saves != null && (!Array.isArray(g.saves) || g.saves.some((x) => typeof x !== "string" || x.includes("..")))) err(`"saves" debe ser una lista de rutas`);
  // "ai": uso de IA en el desarrollo del port ("none", "assisted" o "generated").
  if (g.ai != null && !["none", "assisted", "generated"].includes(g.ai)) err(`"ai" debe ser "none", "assisted" o "generated"`);
  const external = g.origin === "external";
  const required = external ? ["title", "port", "console"] : ["title", "repo", "console"];
  for (const f of required) {
    if (typeof g[f] !== "string" || !g[f].trim()) err(`falta "${f}"${external ? " (los ports externos necesitan el nombre del proyecto en \"port\")" : ""}`);
  }
  if (g.homepage != null && !/^https?:\/\//.test(g.homepage)) err(`"homepage" debe ser una URL http(s)`);
  if (external) {
    g.author ||= "autor desconocido";
    g.id ||= slug(`${g.author} ${g.port}`);
    if (g.repo && !g.branch) warnings.push(`${name}: es "external" pero tiene "repo"; Dársena no descargará de GitHub`);
  }
  if (g.repo && !/^[\w.-]+\/[\w.-]+$/.test(g.repo)) err(`"repo" debe ser "usuario/repositorio" de GitHub (es "${g.repo}")`);
  if (g.repo && !external) {
    const [author, port] = g.repo.split("/");
    g.id ||= repoId(g.repo);
    g.author ||= author;
    g.port ||= port;
  }
  // "consoles": cuando un port reúne juegos de varias consolas (p. ej. una app con Pokémon de GB, GBC y GBA).
  // "console" sigue siendo la principal: es la que ven las versiones antiguas de Dársena.
  if (g.consoles != null) {
    if (!Array.isArray(g.consoles) || g.consoles.some((c) => !CONSOLES.includes(c))) err(`"consoles" debe ser una lista de consolas válidas`);
    else if (g.console && !g.consoles.includes(g.console)) g.consoles.unshift(g.console);
  }
  if (g.console && !CONSOLES.includes(g.console)) {
    err(`consola "${g.console}" no válida. Usa una de: ${CONSOLES.join(", ")}`);
  }
  if (g.asset != null) {
    try { new RegExp(g.asset); } catch (e) { err(`"asset" no es una regex válida: ${e.message}`); }
  }
  for (const p of g.platforms ?? []) {
    if (!PLATFORMS.includes(p)) err(`plataforma "${p}" no válida (usa ${PLATFORMS.join(", ")})`);
  }
  if (g.aliases != null && !Array.isArray(g.aliases)) err(`"aliases" debe ser una lista: ["…", "…"]`);
  if (g.experimental != null && typeof g.experimental !== "boolean") err(`"experimental" debe ser true o false`);
  if (g.needs != null && (!Array.isArray(g.needs) || g.needs.some((n) => typeof n !== "string" || n !== n.toLowerCase()))) {
    err(`"needs" debe ser una lista en minúsculas, p. ej. ["iso", "carpeta"]`);
  }

  // Valores por defecto para los campos opcionales.
  g.origin ??= "github";
  g.franchise ??= null;
  g.aliases ??= [];
  g.asset ??= null;
  g.exe ??= null;
  g.experimental ??= false;
  g.platforms ??= ["Windows"];
  // Fecha en que entra al catálogo: alimenta "Añadidos recientemente" en Explorar.
  g.addedAt ??= TODAY;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(g.addedAt)) err(`"addedAt" debe ser una fecha AAAA-MM-DD (es "${g.addedAt}")`);

  if (g.id) {
    if (seenIds.has(g.id)) err(`id "${g.id}" repetido (también lo usa ${seenIds.get(g.id)})`);
    seenIds.set(g.id, name);
  }
  if (g.repo) {
    const r = g.repo.toLowerCase();
    if (seenRepos.has(r)) warnings.push(`${name}: el repo ${g.repo} ya está en el catálogo (${seenRepos.get(r)})`);
    seenRepos.set(r, name);
  }
});

for (const w of warnings) console.warn(`Aviso: ${w}`);
if (errors.length) fail(`Hay ${errors.length} error(es); no se ha guardado nada:\n  - ${errors.join("\n  - ")}`);

// ---------- 3. Agrupar ports por juego ----------
const overrides = existsSync(overridesFile) ? JSON.parse(readFileSync(overridesFile, "utf8")) : {};
// Clave de agrupación: como el slug pero sin "the" ("Minish Cap" = "The Minish Cap").
const key = (t) => slug(t).split("-").filter((w) => w !== "the").join("-");

const groups = new Map();
for (const g of catalog.games) {
  const k = `${key(g.title)}|${g.console}`;
  if (!groups.has(k)) groups.set(k, []);
  groups.get(k).push(g);
}
// Mismo título en consolas distintas = juegos distintos: se añade la consola al id solo si hace falta.
const consolesByKey = new Map();
for (const [k] of groups) {
  const [t, c] = k.split("|");
  if (!consolesByKey.has(t)) consolesByKey.set(t, new Set());
  consolesByKey.get(t).add(c);
}
for (const [k, ports] of groups) {
  const base = ports.map((g) => slug(g.title)).sort()[0];
  const id = consolesByKey.get(k.split("|")[0]).size > 1 ? `${base}-${slug(ports[0].console)}` : base;
  for (const g of ports) g.game = overrides[g.id] ?? id;
}
for (const id of Object.keys(overrides)) {
  if (!catalog.games.some((g) => g.id === id)) console.warn(`Aviso: games-overrides.json: el port "${id}" no existe`);
}

// ---------- 4. Ordenar y guardar (un port por línea) ----------
catalog.games.sort((a, b) => a.title.localeCompare(b.title, "es") || a.id.localeCompare(b.id));
const ordered = (g) => Object.fromEntries([
  ...FIELD_ORDER.filter((f) => f in g).map((f) => [f, g[f]]),
  ...Object.keys(g).filter((f) => !FIELD_ORDER.includes(f)).map((f) => [f, g[f]]),
]);
const out = `{\n"schemaVersion": ${catalog.schemaVersion ?? 1},\n"generatedAt": "${new Date().toISOString().slice(0, 10)}",\n"games": [\n` +
  catalog.games.map((g) => JSON.stringify(ordered(g))).join(",\n") + "\n]\n}\n";
writeFileSync(catalogFile, out);

const byGame = {};
for (const g of catalog.games) (byGame[g.game] ??= []).push(g);
const multi = Object.values(byGame).filter((v) => v.length > 1).length;
console.log(`OK: Catálogo actualizado: ${catalog.games.length} ports -> ${Object.keys(byGame).length} juegos (${multi} con varios ports)`);
