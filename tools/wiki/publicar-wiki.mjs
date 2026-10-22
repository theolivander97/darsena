// Publica las páginas de tools/wiki/ en la wiki del repo darsena-launcher.
//
// GitHub no deja crear una wiki por API: la primera página hay que crearla una vez desde la web
// (https://github.com/theolivander97/darsena-launcher/wiki → "Create the first page" → guardar). Después, este script
// sube y actualiza todas las páginas.
//
// Uso:  node tools\wiki\publicar-wiki.mjs ["mensaje del commit"]

import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fail, git, GIT_USER, OWNER, REPO, TOOLS } from "../comun.mjs";

const WIKI_DIR = "D:/tools/repos/darsena-launcher.wiki";
const WIKI_URL = `https://${OWNER}@github.com/${OWNER}/${REPO}.wiki.git`;
const PAGES = join(TOOLS, "wiki");
const message = process.argv[2] || `Actualiza la wiki (${new Date().toISOString().slice(0, 10)})`;

if (!existsSync(join(WIKI_DIR, ".git"))) {
  mkdirSync(dirname(WIKI_DIR), { recursive: true });
  try {
    execFileSync("git", ["clone", "-q", WIKI_URL, WIKI_DIR], { stdio: "inherit" });
  } catch {
    fail("La wiki aún no existe. Créala una vez desde la web (pestaña Wiki → Create the first page → Save) y vuelve a ejecutar.");
  }
}
git(WIKI_DIR, "config", "user.name", GIT_USER.name);
git(WIKI_DIR, "config", "user.email", GIT_USER.email);
git(WIKI_DIR, "pull", "-q", "--ff-only");

const pages = readdirSync(PAGES).filter((f) => f.endsWith(".md"));
for (const p of pages) copyFileSync(join(PAGES, p), join(WIKI_DIR, p));
git(WIKI_DIR, "add", "-A");
if (!git(WIKI_DIR, "status", "--porcelain")) {
  console.log("✔ La wiki ya está al día.");
} else {
  git(WIKI_DIR, "commit", "-q", "-m", message);
  execFileSync("git", ["push", "-q", "origin", "HEAD"], { cwd: WIKI_DIR, stdio: "inherit" });
  console.log(`✔ Wiki publicada: ${pages.length} páginas → https://github.com/${OWNER}/${REPO}/wiki`);
}
