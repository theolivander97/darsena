// Genera catalog/info.json: sinopsis y tráilers de cada juego del catálogo.
//
//   Sinopsis:  Wikipedia en español > descripción de Steam (en español) > Wikipedia en inglés.
//   Tráilers:  los de la tienda de Steam (si el juego está allí) + los que pongas a mano en info-overrides.json.
//   Destacados: lista "featured" de info-overrides.json (los que salen siempre en el carrusel de Explorar).
//
// Solo se aceptan coincidencias seguras (el título tiene que coincidir y la página tiene que ser de un videojuego);
// lo que no se encuentra queda en catalog/info-pendientes.txt para completarlo a mano en info-overrides.json.
// Lo hecho a mano en info-overrides.json siempre gana y nunca se sobrescribe.
//
// Uso:  node tools\catalog\informacion.mjs [--solo <id-del-juego>] [--rehacer]
//       (los resultados se guardan en caché: volver a ejecutarlo es rápido)
//       --steam  añade la descripción y los tráilers de la tienda de Steam (lento: ~1,6 s por juego)

import { createHash } from "node:crypto";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const CATALOG = join(ROOT, "catalog", "catalog.json");
const OUT = join(ROOT, "catalog", "info.json");
const OVERRIDES = join(ROOT, "catalog", "info-overrides.json");
const PENDING = join(ROOT, "catalog", "info-pendientes.txt");
const CACHE_FILE = join(dirname(fileURLToPath(import.meta.url)), ".cache-info.json");
// Traducciones de sinopsis (es <-> en), por "idioma:sha1 del texto original". Se pueden corregir a mano:
// si el texto original cambia, su traducción deja de usarse y se vuelve a traducir.
const TRANSLATIONS = join(ROOT, "catalog", "traducciones.json");
const translations = existsSync(TRANSLATIONS) ? JSON.parse(readFileSync(TRANSLATIONS, "utf8")) : {};
let translatorDown = false;

// Traduce con MyMemory (gratis, sin clave; máx. ~500 caracteres por petición, así que se trocea por frases).
async function translate(text, from, to) {
  const key = `${to}:${createHash("sha1").update(text).digest("hex")}`;
  if (translations[key]) return translations[key];
  if (translatorDown) return null;
  const chunks = [];
  for (const sentence of text.match(/[^.!?]+[.!?]+["»”)]*\s*|[^.!?]+$/g) ?? [text]) {
    if (chunks.length && (chunks.at(-1) + sentence).length < 480) chunks[chunks.length - 1] += sentence;
    else chunks.push(sentence);
  }
  const out = [];
  for (const chunk of chunks) {
    try {
      const r = await fetch(`https://api.mymemory.translated.net/get?q=${encodeURIComponent(chunk.trim())}&langpair=${from}|${to}`);
      const j = await r.json();
      if (j.responseStatus !== 200 || !j.responseData?.translatedText || /MYMEMORY WARNING/i.test(j.responseData.translatedText)) {
        translatorDown = true;
        console.log(`\n  (el traductor no responde o se agotó el cupo diario: ${j.responseDetails ?? r.status})`);
        return null;
      }
      out.push(j.responseData.translatedText.trim());
    } catch {
      translatorDown = true;
      return null;
    }
  }
  translations[key] = out.join(" ");
  return translations[key];
}

const args = process.argv.slice(2);
const ONLY = args.includes("--solo") ? args[args.indexOf("--solo") + 1] : null;
const REDO = args.includes("--rehacer");
const STEAM = args.includes("--steam");
const UA = "Darsena/0.1 (https://github.com/theolivander97/darsena-launcher)";

const cache = !REDO && existsSync(CACHE_FILE) ? JSON.parse(readFileSync(CACHE_FILE, "utf8")) : {};
const saveCache = () => writeFileSync(CACHE_FILE, JSON.stringify(cache));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Limitador por servicio: separa las peticiones `gap` ms aunque haya varios juegos en paralelo. */
const slots = {};
async function slot(name, gap) {
  const now = Date.now();
  const at = Math.max(now, slots[name] ?? 0);
  slots[name] = at + gap;
  if (at > now) await sleep(at - now);
}

/** GET con caché. Devuelve el JSON, o null si no existe (404). Los fallos temporales (429/5xx) se reintentan
 *  respetando `Retry-After` y nunca se guardan en la caché como "no existe". */
async function get(url, service, gap) {
  if (url in cache) return cache[url];
  for (let i = 1; i <= 6; i++) {
    try {
      await slot(service, gap);
      const res = await fetch(url, { headers: { "User-Agent": UA, "Accept-Language": "es" } });
      if (res.status === 404) { cache[url] = null; return null; }
      if (res.status === 429 || res.status >= 500) {
        const wait = Math.min(60, Number(res.headers.get("retry-after")) || 5 * i);
        slots[service] = Math.max(slots[service] ?? 0, Date.now() + wait * 1000); // todos esperan, no solo esta petición
        continue;
      }
      if (!res.ok) return null;
      cache[url] = await res.json();
      return cache[url];
    } catch {
      await sleep(2000 * i);
    }
  }
  return null;
}
const wiki = (url) => get(url, "wiki", 220);
// La búsqueda de Steam aguanta bastante; la ficha (appdetails) limita a unas 200 peticiones cada 5 minutos.
const steam = (url) => get(url, url.includes("appdetails") ? "steam-detalle" : "steam-busqueda", url.includes("appdetails") ? 1600 : 250);

const norm = (s) => String(s ?? "").normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase().replace(/&/g, "and").replace(/[^a-z0-9]+/g, "");
const stripParens = (s) => s.replace(/\s*\([^)]*\)/g, "").trim();

// ---------- Candidatos de búsqueda ----------

function candidates(game) {
  const out = new Set();
  const add = (t) => { const c = stripParens(String(t ?? "")).replace(/\s*[·+].*$/, "").trim(); if (c.length >= 2) out.add(c); };
  // Solo el título del juego: los alias ("Star Wars", "Banjo-Kazooie"…) llevan a otras páginas de la misma saga.
  add(game.title);
  const year = String(game.title).match(/\((19|20)\d\d\)/)?.[0].slice(1, 5);
  return { names: [...out].slice(0, 1), year };
}

// ---------- Wikipedia ----------

const BAD_FIRST = /serie de videojuegos|franquicia|franchise|video game series|saga de videojuegos|videogame series|compilaci[oó]n|expansi[oó]n|banda sonora|soundtrack|pel[ií]cula|film\b|novela|novel\b/i;
const GAME_FIRST = /videojuego|video game|videogame|juego de|computer game|arcade game|\bgame\b.*(developed|published|for the)/i;

function acceptableWiki(page, names) {
  if (!page || page.type !== "standard" || !page.extract) return false;
  const first = page.extract.split(/(?<=[.!?])\s/)[0];
  if (!GAME_FIRST.test(first) || BAD_FIRST.test(first)) return false;
  return names.some((n) => sameTitle(page.title, n));
}

// Palabras que distinguen una edición del mismo juego ("Link's Awakening DX" = "Link's Awakening").
const EDITION = new Set(["dx", "hd", "remastered", "remaster", "remake", "classic", "enhanced", "redux", "deluxe", "goty", "definitiveedition", "specialedition", "anniversaryedition"]);

/** El título de la página tiene que ser el del juego: igual, o con una palabra de edición de diferencia.
 *  Nada de "empieza por": así "Heroes 2" no se confunde con "Heroes" ni "Goku" con "Goku II". */
function sameTitle(pageTitle, name) {
  const t = norm(stripParens(pageTitle)).replace(/^the/, "");
  const c = norm(name).replace(/^the/, "");
  if (c.length < 3) return false;
  if (t === c) return true;
  if (c.startsWith(t) && t.length >= 4 && EDITION.has(c.slice(t.length))) return true;
  return t.startsWith(c) && EDITION.has(t.slice(c.length));
}

function clip(text, max = 900) {
  const t = text.replace(/\s+/g, " ").replace(/\[\d+\]/g, "").trim();
  if (t.length <= max) return t;
  const cut = t.slice(0, max);
  const end = Math.max(cut.lastIndexOf(". "), cut.lastIndexOf("! "), cut.lastIndexOf("? "));
  return (end > max * 0.5 ? cut.slice(0, end + 1) : cut.replace(/\s+\S*$/, "") + "…").trim();
}

async function fromWikipedia(lang, { names, year }) {
  const suffix = lang === "es" ? ["", " (videojuego)", year ? ` (videojuego de ${year})` : null] : [" (video game)", year ? ` (${year} video game)` : null, ""];
  const tried = [];
  for (const n of names) for (const sfx of suffix.filter((s) => s !== null)) tried.push(n + sfx);
  for (const title of tried) {
    const page = await wiki(`https://${lang}.wikipedia.org/api/rest_v1/page/summary/${encodeURIComponent(title.replace(/ /g, "_"))}`);
    if (acceptableWiki(page, names)) return { text: clip(page.extract), lang, name: "Wikipedia", url: page.content_urls?.desktop?.page ?? `https://${lang}.wikipedia.org/wiki/${encodeURIComponent(page.title)}` };
  }
  // Búsqueda: solo se miran los primeros resultados y se aplica el mismo filtro estricto.
  const q = `${names[0]} ${lang === "es" ? "videojuego" : "video game"}`;
  const found = await wiki(`https://${lang}.wikipedia.org/w/api.php?action=query&list=search&srsearch=${encodeURIComponent(q)}&srlimit=4&format=json`);
  for (const hit of found?.query?.search ?? []) {
    const page = await wiki(`https://${lang}.wikipedia.org/api/rest_v1/page/summary/${encodeURIComponent(hit.title.replace(/ /g, "_"))}`);
    if (acceptableWiki(page, names)) return { text: clip(page.extract), lang, name: "Wikipedia", url: page.content_urls?.desktop?.page };
  }
  return null;
}

// ---------- Steam ----------

async function fromSteam({ names }) {
  for (const n of names.slice(0, 2)) {
    const s = await steam(`https://store.steampowered.com/api/storesearch/?term=${encodeURIComponent(n)}&cc=us&l=spanish`);
    const c = norm(n);
    const hit = (s?.items ?? []).find((i) => i.type === "app" && c.length >= 3 && (norm(i.name) === c || norm(i.name).startsWith(c)) && norm(i.name).length - c.length <= 30);
    if (!hit) continue;
    const d = (await steam(`https://store.steampowered.com/api/appdetails?appids=${hit.id}&l=spanish`))?.[hit.id]?.data;
    if (!d || d.type !== "game") continue;
    const desc = (d.short_description || "").replace(/<[^>]+>/g, "").trim();
    return {
      appid: hit.id,
      steamName: d.name,
      text: desc ? clip(desc) : null,
      url: `https://store.steampowered.com/app/${hit.id}`,
      trailers: (d.movies ?? []).slice(0, 2).filter((m) => m.hls_h264).map((m) => ({ kind: "steam", title: m.name, thumb: m.thumbnail?.split("?")[0], hls: m.hls_h264 })),
    };
  }
  return null;
}

// ---------- Principal ----------

const catalog = JSON.parse(readFileSync(CATALOG, "utf8")).games;
const overrides = existsSync(OVERRIDES) ? JSON.parse(readFileSync(OVERRIDES, "utf8")) : { featured: [], games: {} };
const previous = existsSync(OUT) ? JSON.parse(readFileSync(OUT, "utf8")).games ?? {} : {};

const games = new Map();
for (const g of catalog) {
  const e = games.get(g.game) ?? { id: g.game, title: g.title, aliases: [], ports: 0 };
  if (g.title.length < e.title.length) e.title = g.title;
  e.aliases.push(...(g.aliases ?? []), g.title);
  e.ports++;
  games.set(g.game, e);
}

const result = {};
const pending = [];
const list = [...games.values()];
let done = 0;

async function process_(game) {
  if (ONLY && game.id !== ONLY) { if (previous[game.id]) result[game.id] = previous[game.id]; return; }
  const ov = overrides.games?.[game.id] ?? {};
  const entry = {};
  if (ov.skip) { pending.push(`${game.id}  (omitido a mano)`); return; }

  const cand = candidates(game);
  // Steam (descripción y tráilers de su tienda) es lento por sus límites: solo con --steam.
  const [es, st] = await Promise.all([fromWikipedia("es", cand), STEAM ? fromSteam(cand) : null]);
  // La de Wikipedia en inglés se busca siempre: es la que ve quien usa Dársena en inglés (synopsisEn).
  const en = await fromWikipedia("en", cand);
  const enSyn = en ? { text: en.text, lang: "en", source: { name: en.name, url: en.url } } : null;
  let synopsis = null;
  if (es) synopsis = { text: es.text, lang: "es", source: { name: es.name, url: es.url } };
  else if (st?.text) synopsis = { text: st.text, lang: "es", source: { name: "Steam", url: st.url } };
  else synopsis = enSyn;
  if (synopsis) entry.synopsis = synopsis;
  if (enSyn) entry.synopsisEn = enSyn;
  if (st?.trailers?.length) entry.trailers = st.trailers;
  if (st) entry.steam = st.appid;

  // A mano: gana siempre.
  if (ov.synopsis) entry.synopsis = { lang: "es", ...ov.synopsis };
  if (ov.synopsisEn) entry.synopsisEn = { lang: "en", ...ov.synopsisEn };
  // Sinopsis siempre en los dos idiomas: lo que solo existe en uno se traduce al otro.
  if (entry.synopsis?.lang === "en") {
    const es = await translate(entry.synopsis.text, "en", "es");
    if (es) {
      entry.synopsisEn ||= entry.synopsis;
      entry.synopsis = { ...entry.synopsis, text: es, lang: "es", translated: "en" };
    }
  } else if (entry.synopsis && !entry.synopsisEn) {
    const en = await translate(entry.synopsis.text, "es", "en");
    if (en) entry.synopsisEn = { ...entry.synopsis, text: en, lang: "en", translated: "es" };
  }
  if (ov.trailers) entry.trailers = [...ov.trailers, ...(ov.keepSteam === false ? [] : entry.trailers ?? [])];
  if (!entry.synopsis) pending.push(`${game.id}  ${game.title}`);
  if (Object.keys(entry).length) result[game.id] = entry;
}

async function worker() {
  while (list.length) {
    const game = list.shift();
    await process_(game);
    done++;
    process.stdout.write(`
${done}/${games.size} juegos · con sinopsis ${Object.values(result).filter((e) => e.synopsis).length}   `);
    if (done % 10 === 0) saveCache();
  }
}
await Promise.all(Array.from({ length: 3 }, worker));
saveCache();

// blockedAuthorInfo: ports cuyo darsena.json (la ficha que escribe su autor) no se muestra en Dársena.
const out = { schemaVersion: 1, generatedAt: new Date().toISOString().slice(0, 10), featured: overrides.featured ?? [], blockedAuthorInfo: overrides.blockedAuthorInfo ?? [], games: result };
writeFileSync(OUT, JSON.stringify(out));
writeFileSync(TRANSLATIONS, JSON.stringify(translations, null, 1) + "\n");
writeFileSync(PENDING, `Juegos sin sinopsis (${pending.length}). Añádelos a mano en info-overrides.json:\n  "games": { "<id>": { "synopsis": { "text": "…" }, "trailers": [{ "kind": "youtube", "id": "…" }] } }\n\n${pending.join("\n")}\n`);
const withS = Object.values(result).filter((e) => e.synopsis).length;
const withT = Object.values(result).filter((e) => e.trailers?.length).length;
console.log(`\nOK: info.json: ${withS}/${games.size} con sinopsis, ${withT} con tráiler. Pendientes: ${pending.length} (ver catalog/info-pendientes.txt)`);
