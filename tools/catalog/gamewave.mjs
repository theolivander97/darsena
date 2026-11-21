// Busca candidatos para el catálogo en la lista pública de GameWave (https://gamewave.digital/ports/catalog.json).
// Solo se usa como pista de qué repositorios mirar: de cada uno se comprueba en GitHub que sea un juego (no una herramienta),
// que no esté archivado, que tenga estrellas y que publique una build de Windows en sus releases.
//
// Uso:  node tools\catalog\gamewave.mjs [estrellas mínimas, por defecto 15]
// Deja catalog/candidatos-gamewave.json (entradas listas para pasar al catálogo) y catalog/candidatos-gamewave.txt (para leerlos).

import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { graphql, ROOT } from "../comun.mjs";
import { loadAll } from "./cargar.mjs";

const MIN_STARS = Number(process.argv[2] ?? 15);
const known = new Set(loadAll().map((g) => g.repo?.toLowerCase()).filter(Boolean));

const NOT_A_GAME = /\b(emulator|emulador|library|libreria|framework|engine|sdk|toolkit|toolchain|compiler|disassembler|decompiler tool|editor|extractor|unpacker|modding tool|mod manager|launcher|loader|libretro|plugin|wrapper|translator|lifter|analy[sz]er|runtime library|template|boilerplate|recomp kit|recompilation kit|xenia|rpcs3|dolphin|ppsspp|yuzu|ryujinx|retroarch)\b/i;
const windowsAsset = (n) => /\.(zip|exe|7z)$/i.test(n) && !/linux|macos|osx|android|\.apk|arm64|aarch64|source|src|switch|vita|ios|appimage/i.test(n);

// Plataforma de GameWave -> consola del catálogo (lo que no encaja se descarta).
function consoleOf(platform) {
  const p = (platform || "").toLowerCase();
  if (/android|ios|mobile/.test(p)) return null;
  const table = [
    [/xbox 360/, "Xbox 360"], [/original xbox|^xbox$|xbox \(|^xbox\b(?! 360| one| series)/, "Xbox"],
    [/playstation 4|ps4/, "PlayStation 4"], [/playstation 3|ps3/, "PlayStation 3"], [/playstation 2|ps2/, "PlayStation 2"],
    [/psp/, "PSP"], [/playstation|psx|ps1|ps one/, "PlayStation"],
    [/nintendo 64|^n64/, "Nintendo 64"], [/gamecube/, "GameCube"], [/wii u/, "Wii U"], [/wii/, "Wii"],
    [/switch/, "Switch"], [/3ds/, "Nintendo 3DS"], [/nintendo ds|^ds$/, "Nintendo DS"],
    [/game boy advance|gba/, "Game Boy Advance"], [/game boy color|gbc/, "Game Boy Color"], [/game boy/, "Game Boy"],
    [/super nintendo|snes/, "Super Nintendo"], [/\bnes\b|famicom/, "NES"], [/virtual boy/, "Virtual Boy"],
    [/dreamcast|saturn|genesis|mega drive|master system|sega/, "Sega"], [/arcade|naomi/, "Arcade"],
    [/pc|windows|dos|source port/, "PC"],
  ];
  for (const [re, c] of table) if (re.test(p)) return c;
  return null;
}

// «Super Mario 64 Plus», «Tekken 3 Recompiled» -> título del juego, sin «Recompiled», «PC Port»…
function cleanTitle(name) {
  let t = name.replace(/�/g, "—").split(/\s[—–-]\s/)[0].replace(/\(([^)]*)\)/g, " ").trim();
  t = t.replace(/\b(Recompiled|Recomp|Recompilation|PC Port|PC Edition|Native Port|Decomp(ilation)?|Port|ReXGlue experiment)\b/gi, " ");
  return t.replace(/\s+/g, " ").replace(/[:\s-]+$/, "").trim();
}

const res = await fetch("https://gamewave.digital/ports/catalog.json");
const apps = (await res.json()).apps;
const byRepo = new Map();
for (const a of apps) {
  const m = (a.repository || "").trim().match(/^(?:https?:\/\/github\.com\/)?([\w.-]+\/[\w.-]+?)(?:\.git)?\/?$/);
  if (!m || known.has(m[1].toLowerCase()) || !a.enabled || a.deprecated || a.sourceType === "local") continue;
  const repo = m[1];
  if (byRepo.has(repo.toLowerCase())) continue;
  const console_ = consoleOf(a.sourcePlatform) ?? (a.category && !/android/i.test(a.category) ? consoleOf(a.category) : null);
  if (!console_ || /kit/i.test(a.category || "")) continue;
  byRepo.set(repo.toLowerCase(), { repo, name: a.name, console: console_ });
}
console.log(`${apps.length} apps en GameWave -> ${byRepo.size} repositorios nuevos con consola válida`);

const repos = [...byRepo.values()];
const out = [];
for (let i = 0; i < repos.length; i += 40) {
  const batch = repos.slice(i, i + 40);
  const q = `{${batch.map((r, k) => {
    const [owner, name] = r.repo.split("/");
    return `r${k}: repository(owner: ${JSON.stringify(owner)}, name: ${JSON.stringify(name)}) {
      nameWithOwner stargazerCount isArchived isFork description pushedAt createdAt forkCount primaryLanguage { name } licenseInfo { spdxId }
      defaultBranchRef { target { ... on Commit { history { totalCount } } } }
      releases(first: 6, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { tagName isDraft releaseAssets(first: 30) { nodes { name } } } }
    }`;
  }).join("\n")}}`;
  let data;
  try { data = await graphql(q); } catch (e) { console.log(`  lote ${i}: ${e.message.slice(0, 100)}`); continue; }
  batch.forEach((r, k) => {
    const g = data[`r${k}`];
    if (!g || g.isArchived || g.stargazerCount < MIN_STARS) return;
    if (NOT_A_GAME.test(`${g.nameWithOwner.split("/")[1]} ${g.description ?? ""}`)) return;
    let build = null;
    for (const rel of g.releases.nodes) {
      if (rel.isDraft) continue;
      const a = rel.releaseAssets.nodes.find((x) => windowsAsset(x.name));
      if (a) { build = `${rel.tagName} (${a.name})`; break; }
    }
    if (!build) return;
    const title = cleanTitle(r.name);
    if (!title) return;
    const commits = g.defaultBranchRef?.target?.history?.totalCount ?? 0;
    const ageDays = Math.floor((Date.now() - Date.parse(g.createdAt)) / 86400000);
    // Señales de que es un proyecto real y no un repositorio de relleno: edad, historial, código y forks.
    const solid = ageDays >= 90 && commits >= 30 && !!g.primaryLanguage && g.forkCount >= 3 && !(g.stargazerCount > 300 && g.forkCount < 5);
    out.push({ title, repo: g.nameWithOwner, console: r.console, stars: g.stargazerCount, forks: g.forkCount, commits, ageDays, language: g.primaryLanguage?.name ?? null, license: g.licenseInfo?.spdxId ?? null, solid, pushedAt: g.pushedAt.slice(0, 10), description: (g.description ?? "").slice(0, 140), build });
  });
  console.log(`  comprobados ${Math.min(i + 40, repos.length)}/${repos.length} · válidos ${out.length}`);
}

out.sort((a, b) => b.stars - a.stars);
writeFileSync(join(ROOT, "catalog", "candidatos-gamewave.json"), JSON.stringify(out, null, 1));
writeFileSync(join(ROOT, "catalog", "candidatos-gamewave.txt"),
  out.map((c) => `${c.title}  [${c.console}]  ${c.repo}  *${c.stars}  ${c.pushedAt}\n    ${c.description}\n    ${c.build}`).join("\n\n"));
console.log(`\nOK: ${out.length} candidatos -> catalog/candidatos-gamewave.json`);
