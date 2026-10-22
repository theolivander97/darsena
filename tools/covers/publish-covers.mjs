// Publica la base de portadas: commit + push del repo covers/ y purga en jsDelivr de lo que cambió
// (jsDelivr cachea @main hasta 12 h; sin purga, las portadas nuevas tardarían en verse).
// Uso: node publish-covers.mjs ["mensaje del commit"]

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "theolivander97/portlauncher-covers";
const BRANCH = "main";
const COVERS = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "covers");
const git = (...args) => execFileSync("git", args, { cwd: COVERS, encoding: "utf8" }).trim();

const message = process.argv[2] || `Actualiza portadas (${new Date().toISOString().slice(0, 10)})`;

let remoteHead = "";
try {
  git("fetch", "-q", "origin", BRANCH);
  remoteHead = git("rev-parse", `origin/${BRANCH}`);
} catch {
  // Primer push: el remoto aún está vacío.
}

git("add", "-A");
if (git("status", "--porcelain")) git("commit", "-q", "-m", message);
execFileSync("git", ["push", "-u", "origin", BRANCH], { cwd: COVERS, stdio: "inherit" });

if (!remoteHead) {
  console.log("Primer push: no hay nada que purgar.");
  process.exit(0);
}
// Solo hace falta purgar lo modificado: los archivos nuevos aún no están en la caché del CDN.
const changed = git("diff", "--name-only", "--diff-filter=M", remoteHead, "HEAD").split("\n").filter(Boolean);
console.log(`Purgando ${changed.length} archivo(s) en jsDelivr…`);
for (const file of changed) {
  const url = `https://purge.jsdelivr.net/gh/${REPO}@${BRANCH}/${file.split("/").map(encodeURIComponent).join("/")}`;
  const res = await fetch(url).catch((e) => ({ ok: false, status: e.message }));
  console.log(`${res.ok ? "OK" : "Error"} ${file} ${res.ok ? "" : res.status}`);
}
