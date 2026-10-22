// Publica una versión nueva de Dársena: compila, firma y crea la Release en GitHub con el instalador
// y el latest.json que consulta el actualizador de los launchers ya instalados.
//
// Uso:  node tools\release\publicar-version.mjs 0.2.0 "Notas de la versión"
//       node tools\release\publicar-version.mjs 0.2.0 "Notas" --sin-compilar   (reusar el último build)
//
// Necesita: GITHUB_TOKEN en tools/.env y la clave de firma en D:\tools\keys\darsena.key (+ .password).

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fail, gh, OWNER, REPO, ROOT, TOOLS } from "../comun.mjs";

const [version, notes = ""] = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const skipBuild = process.argv.includes("--sin-compilar");
const KEY = "D:/tools/keys/darsena.key";
const BUNDLE = "D:/tools/target/darsena/release/bundle/nsis";
const confFile = join(ROOT, "src-tauri", "tauri.conf.json");

if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) fail("Indica la versión: node tools\\release\\publicar-version.mjs 0.2.0 \"notas\"");
if (!existsSync(KEY)) fail(`No está la clave de firma ${KEY}`);
const current = JSON.parse(readFileSync(confFile, "utf8")).version;
const num = (v) => v.split(".").map(Number).reduce((a, x) => a * 1000 + x, 0);
if (num(version) < num(current)) {
  fail(`La versión ${version} no puede ser menor que la actual (${current})`);
}
// Si la release ya existe (p. ej. una subida anterior se cortó), se reanuda: solo se suben los archivos que falten.
let existing = null;
try {
  existing = await gh(`/repos/${OWNER}/${REPO}/releases/tags/v${version}`);
  const names = existing.assets.map((a) => a.name);
  if (names.includes("latest.json")) fail(`La release v${version} ya está publicada y completa`);
  console.log(`Reanudando la release v${version} (ya tiene: ${names.join(", ") || "nada"})`);
} catch (e) {
  if (e.status !== 404) throw e;
}

// 1) Versión en los tres sitios donde vive.
const setVersion = (file, re, repl) => {
  const p = join(ROOT, file);
  writeFileSync(p, readFileSync(p, "utf8").replace(re, repl));
};
setVersion("src-tauri/tauri.conf.json", /"version": "[^"]+"/, `"version": "${version}"`);
setVersion("src-tauri/Cargo.toml", /^version = "[^"]+"/m, `version = "${version}"`);
setVersion("package.json", /"version": "[^"]+"/, `"version": "${version}"`);
console.log(`Versión ${current} → ${version}`);

// 2) Catálogo validado (va embebido como respaldo sin conexión) y build firmado.
execFileSync("node", [join(TOOLS, "catalog", "actualizar.mjs")], { stdio: "inherit" });
if (!skipBuild) {
  console.log("Compilando en modo release (tarda varios minutos)…");
  execFileSync("npm", ["run", "build"], {
    cwd: ROOT,
    stdio: "inherit",
    shell: true,
    env: {
      ...process.env,
      RUSTUP_HOME: "D:\\tools\\rustup",
      CARGO_HOME: "D:\\tools\\cargo",
      npm_config_cache: "D:\\tools\\npm-cache",
      PATH: `D:\\tools\\cargo\\bin;D:\\tools\\winlibs\\mingw64\\bin;${process.env.PATH}`,
      TAURI_SIGNING_PRIVATE_KEY: readFileSync(KEY, "utf8"),
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD: readFileSync(`${KEY}.password`, "utf8").trim(),
    },
  });
}

// 3) Instalador y firma generados por Tauri.
const setup = readdirSync(BUNDLE).find((f) => f.endsWith(`_${version}_x64-setup.exe`));
if (!setup || !existsSync(join(BUNDLE, `${setup}.sig`))) fail(`No se encontró el instalador firmado de ${version} en ${BUNDLE}`);
const assetName = `Darsena_${version}_x64-setup.exe`; // sin tildes: GitHub cambia los nombres no ASCII
const signature = readFileSync(join(BUNDLE, `${setup}.sig`), "utf8").trim();
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature,
      url: `https://github.com/${OWNER}/${REPO}/releases/download/v${version}/${assetName}`,
    },
  },
};

// 4) Release en GitHub con los tres archivos.
const release = existing ?? await gh(`/repos/${OWNER}/${REPO}/releases`, {
  method: "POST",
  body: { tag_name: `v${version}`, target_commitish: "main", name: `Dársena ${version}`, body: notes || `Dársena ${version}` },
});
const upload = async (name, data, type) => {
  const old = (release.assets ?? []).find((a) => a.name === name);
  if (old?.state === "uploaded" && old.size === data.length) return console.log(`  = ${name} (ya estaba)`);
  if (old) await gh(`/repos/${OWNER}/${REPO}/releases/assets/${old.id}`, { method: "DELETE" });
  const url = release.upload_url.replace(/\{.*\}$/, `?name=${encodeURIComponent(name)}`);
  await gh(url, { method: "POST", body: data, raw: true, headers: { "Content-Type": type } });
  console.log(`  ↑ ${name} (${(data.length / 1e6).toFixed(1)} MB)`);
};
await upload(assetName, readFileSync(join(BUNDLE, setup)), "application/octet-stream");
await upload(`${assetName}.sig`, Buffer.from(signature), "text/plain");
await upload("latest.json", Buffer.from(JSON.stringify(latest, null, 2)), "application/json");

console.log(`\nOK: Dársena ${version} publicada: ${release.html_url}`);
console.log("  Los launchers instalados la verán en la ventana de comprobación al abrirse.");
