// Publica los logos de los juegos (carpeta logos/) en su propio repo, theolivander97/darsena-logos, y purga en
// jsDelivr lo que cambió. Van aparte de las portadas porque jsDelivr no sirve repos de más de 50 MB.
// Uso: node publish-logos.mjs ["mensaje del commit"]

import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "theolivander97/darsena-logos";
const BRANCH = "main";
const LOGOS = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "logos");
const git = (...args) => execFileSync("git", args, { cwd: LOGOS, encoding: "utf8" }).trim();
const message = process.argv[2] || `Actualiza logos (${new Date().toISOString().slice(0, 10)})`;

if (!existsSync(join(LOGOS, ".git"))) {
  git("init", "-q", "-b", BRANCH);
  git("remote", "add", "origin", `https://theolivander97@github.com/${REPO}.git`);
}
git("config", "user.name", "theolivander97");
git("config", "user.email", "289308776+theolivander97@users.noreply.github.com");

let remoteHead = "";
try {
  git("fetch", "-q", "origin", BRANCH);
  remoteHead = git("rev-parse", `origin/${BRANCH}`);
} catch {
  // Primer push: el remoto aún está vacío.
}

git("add", "-A");
if (git("status", "--porcelain")) git("commit", "-q", "-m", message);
execFileSync("git", ["push", "-u", "origin", BRANCH], { cwd: LOGOS, stdio: "inherit" });

if (!remoteHead) {
  console.log("Primer push: no hay nada que purgar.");
  process.exit(0);
}
const changed = git("diff", "--name-only", "--diff-filter=M", remoteHead, "HEAD").split("\n").filter(Boolean);
console.log(`Purgando ${changed.length} archivo(s) en jsDelivr…`);
for (const file of changed) {
  const res = await fetch(`https://purge.jsdelivr.net/gh/${REPO}@${BRANCH}/${encodeURIComponent(file)}`).catch((e) => ({ ok: false, status: e.message }));
  console.log(`${res.ok ? "OK" : "Error"} ${file} ${res.ok ? "" : res.status}`);
}
