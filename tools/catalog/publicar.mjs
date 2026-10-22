// Publica el catálogo en GitHub (repo darsena-launcher): los launchers lo descargan al abrirse.
//
// Se puede editar catalog.json en local o directamente en la web de GitHub. Este script lo sincroniza:
//   - si cambió en GitHub desde la última publicación (y en local no), trae esos cambios a tu copia local;
//   - si cambió en los dos sitios, se para y te avisa (no pisa nada);
//   - valida con actualizar.mjs y sube catalog.json, games-overrides.json, info.json e info-overrides.json,
//     más todo lo que haya en tools/repo-files/ (portada README, plantillas de issues, Action de reseñas, capturas).
//
// Uso:  node tools\catalog\publicar.mjs ["mensaje para lo que no tenga descripción fija"]

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fail, git, GIT_USER, REPO_DIR, REPO_URL, ROOT, TOOLS } from "../comun.mjs";

const LOCAL = join(ROOT, "catalog");
const LAST_SYNC = join(TOOLS, "catalog", ".ultima-publicacion");
// La portada del repo (README.md) sale de tools/repo-files/; la guía del catálogo vive en la wiki (tools/wiki/).
const FILES = [["catalog.json", "catalog.json"], ["games-overrides.json", "games-overrides.json"], ["info.json", "info.json"], ["info-overrides.json", "info-overrides.json"]];
const message = process.argv[2] || `Actualiza el catálogo (${new Date().toISOString().slice(0, 10)})`;
const read = (p) => (existsSync(p) ? readFileSync(p, "utf8").replace(/\r\n/g, "\n") : null);

// 1) Clon local del repo (o repo nuevo si aún está vacío).
if (!existsSync(join(REPO_DIR, ".git"))) {
  mkdirSync(dirname(REPO_DIR), { recursive: true });
  try {
    execFileSync("git", ["clone", "-q", REPO_URL, REPO_DIR], { stdio: "inherit" });
  } catch {
    fail(`no se pudo clonar ${REPO_URL}`);
  }
}
git(REPO_DIR, "config", "user.name", GIT_USER.name);
git(REPO_DIR, "config", "user.email", GIT_USER.email);
const hasCommits = (() => { try { git(REPO_DIR, "rev-parse", "HEAD"); return true; } catch { return false; } })();
if (hasCommits) {
  git(REPO_DIR, "pull", "-q", "--ff-only", "origin", "main");
}

// 2) ¿Se editó el catálogo en GitHub desde la última publicación?
const remote = read(join(REPO_DIR, "catalog.json"));
const last = read(LAST_SYNC);
const local = read(join(LOCAL, "catalog.json"));
if (remote && last !== null && remote !== last) {
  if (local !== last) {
    fail("catalog.json cambió en GitHub y también en local desde la última publicación.\n" +
      `  Copia de GitHub: ${join(REPO_DIR, "catalog.json")}\n` +
      "  Junta los cambios a mano en catalog/catalog.json y vuelve a ejecutar.");
  }
  console.log("↓ Trayendo los cambios hechos en GitHub a catalog/catalog.json");
  writeFileSync(join(LOCAL, "catalog.json"), remote);
}

// 3) Validar y dar formato (si hay un error, no se publica nada).
execFileSync("node", [join(TOOLS, "catalog", "actualizar.mjs")], { stdio: "inherit" });

// 3b) Portada, plantillas y demás archivos de tools/repo-files/: lo mismo que con el catálogo. Si se editaron en GitHub
// desde la última publicación (y en local no), se traen a local; si cambiaron en los dos sitios, se para.
// Así publicar nunca pisa una edición hecha desde la web.
const REPO_FILES = join(TOOLS, "repo-files");
const FILES_SYNC = join(TOOLS, "catalog", ".ultima-publicacion-archivos.json");
const sha = (p) => createHash("sha1").update(readFileSync(p)).digest("hex");
const walk = (dir, base = "") => readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
  e.isDirectory() ? walk(join(dir, e.name), `${base}${e.name}/`) : [`${base}${e.name}`]);
const repoFiles = walk(REPO_FILES);
const lastHashes = existsSync(FILES_SYNC) ? JSON.parse(readFileSync(FILES_SYNC, "utf8")) : {};
for (const rel of repoFiles) {
  const local = join(REPO_FILES, rel);
  const remote = join(REPO_DIR, rel);
  const last = lastHashes[rel];
  if (!existsSync(remote) || last === undefined) continue;
  const [l, r] = [sha(local), sha(remote)];
  if (l === r || r === last) continue;
  if (l !== last) {
    fail(`${rel} cambió en GitHub y también en local desde la última publicación.\n` +
      `  Copia de GitHub: ${remote}\n  Junta los cambios a mano en tools/repo-files/${rel} y vuelve a ejecutar.`);
  }
  console.log(`↓ Trayendo el cambio hecho en GitHub a tools/repo-files/${rel}`);
  copyFileSync(remote, local);
}

// 4) Copiar al repo, commit y push.
for (const [from, to] of FILES) copyFileSync(join(LOCAL, from), join(REPO_DIR, to));
cpSync(join(TOOLS, "repo-files"), REPO_DIR, { recursive: true });
// GitHub muestra junto a cada archivo el mensaje del último commit que lo tocó: por eso cada archivo (o carpeta)
// se sube en su propio commit con un mensaje fijo que dice qué es, no qué cambió.
const DESCRIPCIONES = {
  "catalog.json": "Catálogo de Dársena",
  "games-overrides.json": "Correcciones de agrupación de ports por juego",
  "info.json": "Sinopsis, tráilers y destacados de los juegos",
  "info-overrides.json": "Sinopsis, tráilers y destacados puestos a mano",
  "README.md": "Portada de Dársena",
  "CONTRIBUTING.md": "Cómo participar",
  ".github": "Plantillas de solicitudes y resumen automático de reseñas",
  "docs": "Capturas de pantalla de Dársena",
};
const lines = (...args) => git(REPO_DIR, "-c", "core.quotepath=off", ...args).split("\n").filter(Boolean);
const changed = [
  ...(hasCommits ? lines("diff", "--name-only", "HEAD") : []),
  ...lines("ls-files", "--others", "--exclude-standard"),
].map((p) => p.split("/")[0]);
const groups = [...new Set(changed)];
if (!groups.length) {
  console.log("OK: El catálogo publicado ya está al día.");
} else {
  for (const g of groups) {
    git(REPO_DIR, "add", "-A", "--", g);
    if (git(REPO_DIR, "diff", "--cached", "--name-only")) git(REPO_DIR, "commit", "-q", "-m", DESCRIPCIONES[g] ?? message);
  }
  git(REPO_DIR, "add", "-A");
  if (git(REPO_DIR, "status", "--porcelain")) git(REPO_DIR, "commit", "-q", "-m", message);
  if (!hasCommits) git(REPO_DIR, "branch", "-M", "main");
  execFileSync("git", ["push", "-u", "origin", "main"], { cwd: REPO_DIR, stdio: "inherit" });
  console.log("OK: Catálogo publicado. Los launchers lo verán al abrirse (GitHub tarda hasta 5 minutos en servir el cambio).");
}
writeFileSync(LAST_SYNC, read(join(REPO_DIR, "catalog.json")));
writeFileSync(FILES_SYNC, JSON.stringify(Object.fromEntries(repoFiles.map((rel) => [rel, sha(join(REPO_DIR, rel))])), null, 1));
