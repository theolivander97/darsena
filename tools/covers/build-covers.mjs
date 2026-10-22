// Construye la base de datos de portadas propia de Dársena en /covers.
//
//   covers/<id>.webp       portada vertical 600x900 (estilo cápsula de Steam)
//   covers/manual/<id>.*   portadas puestas a mano (png/jpg/webp): siempre tienen prioridad
//   covers/overrides.json  id -> "Sistema/Nombre.png" (forzar libretro), "sgdb:<gameId>" (forzar SteamGridDB) o null (excluir)
//   covers/index.json      id -> { v: hash corto para invalidar caché, src: origen }
//   covers/report.txt      juegos sin portada, para completarlos a mano
//
// Carga inicial automática desde libretro-thumbnails (carátulas comunitarias de RetroArch) y,
// para lo que no está ahí, SteamGridDB (requiere STEAMGRIDDB_API_KEY en tools/covers/.env).
// Uso: node build-covers.mjs [--force] [--only <id>]

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const COVERS = join(ROOT, "covers");
const MANUAL = join(COVERS, "manual");
const CACHE = join(dirname(fileURLToPath(import.meta.url)), ".cache");
const LIBRETRO = "https://thumbnails.libretro.com";
const W = 600, H = 900;

const args = process.argv.slice(2);
const FORCE = args.includes("--force");
const ONLY = args.includes("--only") ? args[args.indexOf("--only") + 1] : null;

// Consola del catálogo -> carpetas de libretro donde buscar (en orden).
const SYSTEMS = {
  "Nintendo 64": ["Nintendo - Nintendo 64", "Nintendo - Nintendo 64DD"],
  "Super Nintendo": ["Nintendo - Super Nintendo Entertainment System", "Nintendo - Satellaview"],
  "NES": ["Nintendo - Nintendo Entertainment System", "Nintendo - Family Computer Disk System"],
  "Game Boy": ["Nintendo - Game Boy", "Nintendo - Game Boy Color"],
  "Game Boy Advance": ["Nintendo - Game Boy Advance"],
  "Nintendo DS": ["Nintendo - Nintendo DS", "Nintendo - Nintendo DSi"],
  "Nintendo 3DS": ["Nintendo - Nintendo 3DS"],
  "GameCube": ["Nintendo - GameCube"],
  "Wii": ["Nintendo - Wii"],
  "Wii U": ["Nintendo - Wii U"],
  "Virtual Boy": ["Nintendo - Virtual Boy"],
  "PlayStation": ["Sony - PlayStation"],
  "PlayStation 2": ["Sony - PlayStation 2"],
  "PlayStation 3": ["Sony - PlayStation 3"],
  "PlayStation 4": ["Sony - PlayStation 4"],
  "PSP": ["Sony - PlayStation Portable"],
  "Xbox": ["Microsoft - Xbox"],
  "Xbox 360": ["Microsoft - Xbox 360"],
  "Sega": ["Sega - Mega Drive - Genesis", "Sega - Dreamcast", "Sega - Saturn", "Sega - 32X", "Sega - Mega-CD - Sega CD", "Sega - Master System - Mark III", "Sega - Game Gear", "Sega - Naomi 2", "Sega - Naomi"],
  "Arcade": ["FBNeo - Arcade Games", "MAME", "SNK - Neo Geo", "Sega - Naomi 2", "Sega - Naomi", "Atomiswave"],
  "PC": ["DOS", "ScummVM", "DOOM", "Quake", "Quake II", "Quake III", "Wolfenstein 3D", "Tomb Raider", "Cave Story", "Flashback", "Rick Dangerous", "Commodore - Amiga"],
  "Otros": [],
};

const BAD = /\b(beta|proto|prototype|demo|debug|kiosk|sample|unl|pirate|hack|aftermarket|program|trial|competition)\b/i;
const REGION_BONUS = [[/\(usa, europe\)|\(world\)/i, 6], [/\(usa\)/i, 5], [/\(europe\)/i, 3], [/\(japan\)/i, 1]];

function tokens(s) {
  return s.normalize("NFD").replace(/[̀-ͯ]/g, "")
    .replace(/\([^)]*\)|\[[^\]]*\]/g, " ")      // etiquetas (USA) [!] …
    .replace(/·.*$/, " ")                        // "Juego · Port" -> "Juego"
    .replace(/&/g, " and ")
    .replace(/['’`´]/g, "")
    .toLowerCase().split(/[^a-z0-9]+/)
    .filter((t) => t && t !== "the");
}

// exact=true para alias: solo coincidencia exacta (los alias suelen ser genéricos: "Pokémon", "Sonic"…).
function score(title, candidate, exact = false) {
  const want = tokens(title), have = tokens(candidate);
  if (!want.length || !have.length) return 0;
  let s;
  if (want.join(" ") === have.join(" ")) s = 100;
  // Mismo título + subtítulo corto detrás: "Pokemon Platinum" -> "Pokemon - Platinum Version".
  // Un número o romano extra indica secuela ("Shrek" no es "Shrek 2").
  else if (!exact && have.length - want.length <= 2 && want.every((t, i) => have[i] === t)
    && !have.slice(want.length).some((t) => /^(\d+|ii|iii|iv|v|vi)$/.test(t))) s = 80 - (have.length - want.length) * 3;
  else return 0;
  if (BAD.test(candidate)) s -= 50;
  // Preferir el nombre más limpio: cada etiqueta extra (Rev 1), (GameCube)… resta.
  s -= Math.max(0, (candidate.match(/\(/g) || []).length - 1) * 2;
  for (const [re, b] of REGION_BONUS) if (re.test(candidate)) { s += b; break; }
  return s;
}

const listings = new Map();
function listing(system) {
  if (!listings.has(system)) listings.set(system, fetchListing(system));
  return listings.get(system);
}

async function fetchListing(system) {
  mkdirSync(CACHE, { recursive: true });
  const file = join(CACHE, system.replace(/[^\w-]+/g, "_") + ".json");
  if (existsSync(file)) return JSON.parse(readFileSync(file, "utf8"));
  const res = await fetch(`${LIBRETRO}/${encodeURIComponent(system)}/Named_Boxarts/`);
  const names = res.ok
    ? [...(await res.text()).matchAll(/href="([^"?]+\.png)"/g)].map((m) => decodeURIComponent(m[1]))
    : [];
  writeFileSync(file, JSON.stringify(names));
  return names;
}

// Portada vertical: la carátula entera (contain) sobre una versión ampliada, difuminada y oscurecida.
async function toCapsule(input) {
  const bg = await sharp(input).resize(W, H, { fit: "cover" }).blur(28).modulate({ brightness: 0.55 }).toBuffer();
  const fg = await sharp(input).resize(W - 40, H - 40, { fit: "inside" }).toBuffer();
  return sharp(bg).composite([{ input: fg, gravity: "center" }]).webp({ quality: 74 }).toBuffer();
}

const shortHash = (buf) => createHash("sha1").update(buf).digest("hex").slice(0, 10);

async function pool(items, n, fn) {
  let i = 0;
  await Promise.all(Array.from({ length: n }, async () => {
    while (i < items.length) await fn(items[i++]);
  }));
}

// Títulos a probar: los de los ports (con subtítulo permitido) y los alias (solo exactos).
// Se descartan alias que son el principio del título: "Dragon Ball Z" no vale para "Dragon Ball Z: Budokai HD".
function candidates(g) {
  const titleTokens = g.titles.map(tokens);
  const generic = (alias) => {
    const a = tokens(alias);
    return titleTokens.some((t) => t.length > a.length && a.every((w, i) => t[i] === w));
  };
  return [...g.titles.map((t) => [t, false]), ...g.aliases.filter((a) => !generic(a)).map((a) => [a, true])];
}

// ---------- SteamGridDB (segunda fuente, para lo que libretro no tiene: Xbox 360, PC…) ----------

function loadEnv() {
  const file = join(dirname(fileURLToPath(import.meta.url)), ".env");
  if (!existsSync(file)) return {};
  return Object.fromEntries(readFileSync(file, "utf8").split(/\r?\n/)
    .map((l) => l.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*?)\s*$/)).filter(Boolean).map((m) => [m[1], m[2]]));
}
const SGDB_KEY = process.env.STEAMGRIDDB_API_KEY || loadEnv().STEAMGRIDDB_API_KEY || "";
const sgdbCacheFile = join(CACHE, "sgdb.json");
const sgdbCache = existsSync(sgdbCacheFile) ? JSON.parse(readFileSync(sgdbCacheFile, "utf8")) : { search: {}, grids: {} };

async function sgdb(path) {
  for (let attempt = 0; attempt < 4; attempt++) {
    const res = await fetch(`https://www.steamgriddb.com/api/v2${path}`, { headers: { Authorization: `Bearer ${SGDB_KEY}` } });
    if (res.status === 429) { await new Promise((r) => setTimeout(r, 2000 * (attempt + 1))); continue; }
    if (res.status === 401) throw new Error("API key de SteamGridDB inválida");
    if (res.status === 404) return [];
    const json = await res.json();
    if (!json.success) throw new Error(`SteamGridDB: ${JSON.stringify(json.errors || json)}`);
    return json.data;
  }
  throw new Error("SteamGridDB: demasiadas peticiones");
}

async function sgdbSearch(term) {
  sgdbCache.search[term] ??= await sgdb(`/search/autocomplete/${encodeURIComponent(term)}`);
  return sgdbCache.search[term];
}

// Año entre paréntesis en el título ("Prey (2006)"), para no confundir con remakes.
const yearOf = (t) => Number(t.match(/\((?:[^)]*\D)?((?:19|20)\d\d)\)/)?.[1]) || null;

// Busca el juego en SteamGridDB con el mismo criterio de emparejado que libretro.
async function sgdbFind(g) {
  let best = null;
  const year = g.titles.map(yearOf).find(Boolean);
  for (const [title, exact] of candidates(g)) {
    const term = title.replace(/\([^)]*\)/g, " ").replace(/·.*$/, " ").replace(/\s+/g, " ").trim();
    if (!term) continue;
    for (const r of await sgdbSearch(term)) {
      if (year && r.release_date && Math.abs(new Date(r.release_date * 1000).getUTCFullYear() - year) > 1) continue;
      const s = score(title, r.name, exact);
      if (s >= 70 && (!best || s > best.s)) best = { s, gameId: r.id, name: r.name };
    }
    if (best?.s >= 100) break;
  }
  return best;
}

// Póster 600x900 estático mejor valorado (sin NSFW, humor ni epilepsia).
async function sgdbGrid(gameId) {
  sgdbCache.grids[gameId] ??= (await sgdb(`/grids/game/${gameId}?dimensions=600x900&types=static&nsfw=false&humor=false&epilepsy=false`))
    .map((x) => ({ id: x.id, url: x.url, score: x.score, lang: x.language }));
  const grids = sgdbCache.grids[gameId];
  return grids.find((x) => x.lang === "en") || grids[0] || null;
}

// ---------------------------------------------------------------------------

mkdirSync(MANUAL, { recursive: true });
const catalog = JSON.parse(readFileSync(join(ROOT, "catalog", "catalog.json"), "utf8")).games;
const indexFile = join(COVERS, "index.json");
const index = existsSync(indexFile) ? JSON.parse(readFileSync(indexFile, "utf8")).covers : {};
// Curación a mano: id -> "Sistema libretro/Nombre.png" para forzar una carátula, o null para excluirla.
const overridesFile = join(COVERS, "overrides.json");
const overrides = existsSync(overridesFile) ? JSON.parse(readFileSync(overridesFile, "utf8")) : {};
const manual = Object.fromEntries(readdirSync(MANUAL)
  .filter((f) => /\.(png|jpe?g|webp)$/i.test(f))
  .map((f) => [f.replace(/\.[^.]+$/, ""), join(MANUAL, f)]));

const missing = [];
let added = 0;
// Una portada por juego (campo "game" del catálogo), compartida por todos sus ports.
const byGame = new Map();
for (const p of catalog) {
  const g = byGame.get(p.game) ?? { id: p.game, title: p.title, console: p.console, titles: [], aliases: [] };
  if (!g.titles.includes(p.title)) g.titles.push(p.title);
  for (const a of p.aliases || []) if (!g.aliases.includes(a)) g.aliases.push(a);
  byGame.set(p.game, g);
}
const allGames = [...byGame.values()];
const games = ONLY ? allGames.filter((g) => g.id === ONLY) : allGames;

await pool(games, 8, async (g) => {
  const out = join(COVERS, `${g.id}.webp`);
  try {
    if (manual[g.id]) {
      const buf = await toCapsule(readFileSync(manual[g.id]));
      writeFileSync(out, buf);
      index[g.id] = { ...index[g.id], v: shortHash(buf), src: "manual" };
      added++;
      return;
    }
    const ov = overrides[g.id];
    const upToDate = ov === undefined
      || (typeof ov === "string" && (index[g.id]?.src === `libretro:${ov}` || index[g.id]?.src?.startsWith(`${ov}/`)));
    if (!FORCE && upToDate && index[g.id] && existsSync(out)) return;

    let best = null;
    if (ov === null) {
      // Excluido a mano: emparejamiento automático incorrecto y sin portada mejor.
      if (index[g.id] && index[g.id].src !== "manual") { delete index[g.id]; rmSync(out, { force: true }); }
      missing.push(g);
      return;
    }
    let sgdbGame = typeof ov === "string" && ov.startsWith("sgdb:") ? { s: 1000, gameId: Number(ov.slice(5)), name: ov } : null;
    if (typeof ov === "string" && !sgdbGame) {
      const [system, ...rest] = ov.split("/");
      best = { s: 1000, system, name: rest.join("/") };
    }
    for (const system of best || sgdbGame ? [] : SYSTEMS[g.console] ?? []) {
      for (const name of await listing(system)) {
        for (const [title, exact] of candidates(g)) {
          // A igualdad, gana el título principal sobre un alias (Raze tiene el alias "Blood").
          const s = score(title, name.replace(/\.png$/, ""), exact) - (exact ? 1 : 0);
          if (s >= 70 && (!best || s > best.s)) best = { s, system, name };
        }
      }
      if (best?.s >= 100) break;
    }
    if (!best && SGDB_KEY) {
      sgdbGame ??= await sgdbFind(g);
      const grid = sgdbGame && (await sgdbGrid(sgdbGame.gameId));
      if (grid) {
        const res = await fetch(grid.url);
        if (!res.ok) throw new Error(`HTTP ${res.status} ${grid.url}`);
        // Los pósters de SteamGridDB ya son 600x900: solo se normaliza tamaño y formato.
        const buf = await sharp(Buffer.from(await res.arrayBuffer())).resize(W, H, { fit: "cover" }).webp({ quality: 74 }).toBuffer();
        writeFileSync(out, buf);
        index[g.id] = { ...index[g.id], v: shortHash(buf), src: `sgdb:${sgdbGame.gameId}/${grid.id}` };
        added++;
        console.log(`OK: ${g.title}  ←  SteamGridDB "${sgdbGame.name}" (grid ${grid.id})  [${sgdbGame.s}]`);
        return;
      }
    }
    if (!best) {
      // Con --force, una portada que ya no tiene coincidencia válida se elimina.
      if (index[g.id] && index[g.id].src !== "manual") { delete index[g.id]; rmSync(out, { force: true }); }
      missing.push(g);
      return;
    }

    const url = `${LIBRETRO}/${encodeURIComponent(best.system)}/Named_Boxarts/${encodeURIComponent(best.name)}`;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status} ${url}`);
    const buf = await toCapsule(Buffer.from(await res.arrayBuffer()));
    writeFileSync(out, buf);
    index[g.id] = { ...index[g.id], v: shortHash(buf), src: `libretro:${best.system}/${best.name}` };
    added++;
    console.log(`OK: ${g.title}  ←  ${best.name}  [${best.s}]`);
  } catch (e) {
    console.log(`Error: ${g.title}: ${e.message}`);
    missing.push(g);
  }
});

// ---------- Heroes: artes panorámicos para el carrusel de Explorar (SteamGridDB, 1920x620) ----------
// En el índice: h = versión del hero, o null si se buscó y no hay (no se vuelve a buscar salvo con --force).
const HEROES = join(COVERS, "heroes");
mkdirSync(HEROES, { recursive: true });
let heroesAdded = 0;
if (SGDB_KEY) {
  await pool(games, 6, async (g) => {
    const out = join(HEROES, `${g.id}.webp`);
    if (!FORCE && index[g.id] && "h" in index[g.id] && (index[g.id].h === null || existsSync(out))) return;
    try {
      const ov = overrides[g.id];
      const src = index[g.id]?.src;
      let gameId = typeof ov === "string" && ov.startsWith("sgdb:") ? Number(ov.slice(5)) : null;
      if (!gameId && src?.startsWith("sgdb:")) gameId = Number(src.slice(5).split("/")[0]);
      if (!gameId && ov !== null) gameId = (await sgdbFind(g))?.gameId;
      sgdbCache.heroes ??= {};
      if (gameId) {
        sgdbCache.heroes[gameId] ??= (await sgdb(`/heroes/game/${gameId}?dimensions=1920x620&types=static&nsfw=false&humor=false&epilepsy=false`))
          .map((x) => ({ id: x.id, url: x.url, lang: x.language }));
      }
      const list = gameId ? sgdbCache.heroes[gameId] : [];
      const hero = list.find((x) => x.lang === "en") || list[0];
      if (!hero) {
        index[g.id] = { ...index[g.id], h: null };
        return;
      }
      const res = await fetch(hero.url);
      if (!res.ok) throw new Error(`HTTP ${res.status} ${hero.url}`);
      const buf = await sharp(Buffer.from(await res.arrayBuffer())).resize(1280, 413, { fit: "cover" }).webp({ quality: 72 }).toBuffer();
      writeFileSync(out, buf);
      index[g.id] = { ...index[g.id], h: shortHash(buf) };
      heroesAdded++;
    } catch (e) {
      console.log(`Error: hero ${g.title}: ${e.message}`);
    }
  });
}

// ---------- Logos: el nombre del juego con su tipografía, sobre el hero (como en Steam) ----------
// Van en su propio repo (logos/ -> theolivander97/darsena-logos): jsDelivr no sirve repos de más de 50 MB y
// el de portadas está al límite. Índice: logos/index.json { logos: { id: versión | null } } (null = se buscó y no hay).
const LOGOS = join(COVERS, "..", "logos");
mkdirSync(LOGOS, { recursive: true });
const logosIndexFile = join(LOGOS, "index.json");
const logosIndex = existsSync(logosIndexFile) ? JSON.parse(readFileSync(logosIndexFile, "utf8")).logos ?? {} : {};
let logosAdded = 0;
if (SGDB_KEY) {
  await pool(games, 6, async (g) => {
    const out = join(LOGOS, `${g.id}.webp`);
    if (!FORCE && g.id in logosIndex && (logosIndex[g.id] === null || existsSync(out))) return;
    try {
      const ov = overrides[g.id];
      const src = index[g.id]?.src;
      let gameId = typeof ov === "string" && ov.startsWith("sgdb:") ? Number(ov.slice(5)) : null;
      if (!gameId && src?.startsWith("sgdb:")) gameId = Number(src.slice(5).split("/")[0]);
      if (!gameId && ov !== null) gameId = (await sgdbFind(g))?.gameId;
      sgdbCache.logos ??= {};
      if (gameId) {
        sgdbCache.logos[gameId] ??= (await sgdb(`/logos/game/${gameId}?types=static&nsfw=false&humor=false&epilepsy=false`))
          .map((x) => ({ id: x.id, url: x.url, lang: x.language, style: x.style }));
      }
      const list = gameId ? sgdbCache.logos[gameId] : [];
      const logo = list.find((x) => x.lang === "en" && x.style === "official") || list.find((x) => x.lang === "en") || list[0];
      if (!logo) {
        logosIndex[g.id] = null;
        return;
      }
      const res = await fetch(logo.url);
      if (!res.ok) throw new Error(`HTTP ${res.status} ${logo.url}`);
      const buf = await sharp(Buffer.from(await res.arrayBuffer())).resize(640, 300, { fit: "inside", withoutEnlargement: true }).webp({ quality: 88, alphaQuality: 100 }).toBuffer();
      writeFileSync(out, buf);
      logosIndex[g.id] = shortHash(buf);
      logosAdded++;
    } catch (e) {
      console.log(`Error: logo ${g.title}: ${e.message}`);
    }
  });
}

mkdirSync(CACHE, { recursive: true });
writeFileSync(sgdbCacheFile, JSON.stringify(sgdbCache));
if (!SGDB_KEY) console.log("(Sin STEAMGRIDDB_API_KEY: solo se usó libretro. Ponla en tools/covers/.env)");

// Limpieza: entradas e imágenes de ids que ya no son juegos del catálogo (p. ej. ids antiguos por port).
if (!ONLY) {
  for (const id of Object.keys(index)) if (!byGame.has(id)) delete index[id];
  for (const f of readdirSync(COVERS)) {
    if (f.endsWith(".webp") && !byGame.has(f.slice(0, -5))) rmSync(join(COVERS, f));
  }
  for (const id of Object.keys(logosIndex)) if (!byGame.has(id)) delete logosIndex[id];
  for (const dir of [HEROES, LOGOS]) {
    for (const f of readdirSync(dir)) {
      if (f.endsWith(".webp") && !byGame.has(f.slice(0, -5))) rmSync(join(dir, f));
    }
  }
}

const sorted = Object.fromEntries(Object.entries(index).sort(([a], [b]) => a.localeCompare(b)));
writeFileSync(indexFile, JSON.stringify({ generatedAt: new Date().toISOString().slice(0, 10), covers: sorted }, null, 1));
missing.sort((a, b) => a.console.localeCompare(b.console) || a.title.localeCompare(b.title));
writeFileSync(join(COVERS, "report.txt"),
  `Juegos sin portada (${missing.length}). Pon una imagen en covers/manual/<id del juego>.png|jpg|webp y vuelve a ejecutar.\n\n` +
  missing.map((g) => `${g.id.padEnd(55)} ${g.console.padEnd(18)} ${g.title}`).join("\n") + "\n");
const withCover = Object.values(sorted).filter((e) => e.v).length;
const withHero = Object.values(sorted).filter((e) => e.h).length;
console.log(`\nPortadas: ${withCover}/${allGames.length} juegos (nuevas: ${added}, sin portada: ${missing.length})`);
console.log(`Heroes: ${withHero}/${allGames.length} juegos (nuevos: ${heroesAdded})`);
const logosSorted = Object.fromEntries(Object.entries(logosIndex).sort(([a], [b]) => a.localeCompare(b)));
writeFileSync(logosIndexFile, JSON.stringify({ generatedAt: new Date().toISOString().slice(0, 10), logos: logosSorted }, null, 1));
console.log(`Logos: ${Object.values(logosSorted).filter(Boolean).length}/${allGames.length} juegos (nuevos: ${logosAdded})`);
