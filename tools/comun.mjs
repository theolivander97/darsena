// Utilidades compartidas por los scripts de publicación de Dársena.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const TOOLS = dirname(fileURLToPath(import.meta.url));
export const ROOT = join(TOOLS, "..");
export const OWNER = "theolivander97";
export const REPO = "darsena-launcher";
export const REPO_URL = `https://${OWNER}@github.com/${OWNER}/${REPO}.git`;
/** Clon local del repo publicado (catálogo + releases), fuera del proyecto. */
export const REPO_DIR = "D:/tools/repos/darsena-launcher";
/** Identidad de los commits: correo noreply de la cuenta, para no publicar correos personales. */
export const GIT_USER = { name: "theolivander97", email: "289308776+theolivander97@users.noreply.github.com" };

export function loadEnv() {
  const env = {};
  for (const file of [join(TOOLS, ".env"), join(TOOLS, "covers", ".env")]) {
    if (!existsSync(file)) continue;
    for (const line of readFileSync(file, "utf8").split(/\r?\n/)) {
      const m = line.match(/^\s*([A-Z0-9_]+)\s*=\s*(.*?)\s*$/);
      if (m) env[m[1]] = m[2];
    }
  }
  return { ...env, ...process.env };
}

export function token() {
  const t = loadEnv().GITHUB_TOKEN;
  if (!t) fail("Falta GITHUB_TOKEN en tools/.env");
  return t;
}

export function fail(msg) {
  console.error(`\nError: ${msg}\n`);
  process.exit(1);
}

/** Llamada a la API REST de GitHub. `path` puede ser absoluto (uploads.github.com).
 *  Reintenta hasta 3 veces si falla la red (subidas grandes que se cortan, etc.). */
export async function gh(path, opts = {}) {
  for (let attempt = 1; ; attempt++) {
    try {
      return await ghOnce(path, opts);
    } catch (e) {
      // Límite secundario de GitHub (demasiadas peticiones seguidas): esperar y reintentar.
      if (e.secondary && attempt < 4) {
        console.log(`  … GitHub pide ir más despacio, espero ${60 * attempt} s`);
        await new Promise((r) => setTimeout(r, 60000 * attempt));
        continue;
      }
      if (e.status || attempt >= 3) throw e; // errores HTTP de GitHub no se reintentan
      console.log(`  … fallo de red (${e.cause?.code ?? e.message}), reintento ${attempt + 1}/3`);
      await new Promise((r) => setTimeout(r, 3000 * attempt));
    }
  }
}

async function ghOnce(path, { method = "GET", body, headers = {}, raw = false } = {}) {
  const url = path.startsWith("http") ? path : `https://api.github.com${path}`;
  const res = await fetch(url, {
    method,
    headers: {
      Authorization: `Bearer ${token()}`,
      Accept: "application/vnd.github+json",
      "User-Agent": "darsena-tools",
      ...(body && !raw ? { "Content-Type": "application/json" } : {}),
      ...headers,
    },
    body: raw ? body : body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  const data = text ? (() => { try { return JSON.parse(text); } catch { return text; } })() : null;
  if (!res.ok) {
    const err = new Error(`GitHub ${method} ${path}: ${res.status} ${data?.message ?? text.slice(0, 200)}`);
    err.status = res.status;
    err.secondary = res.status === 403 && /secondary rate limit/i.test(text);
    throw err;
  }
  return data;
}

export async function graphql(query) {
  const data = await gh("/graphql", { method: "POST", body: { query } });
  if (data.errors && !data.data) throw new Error(JSON.stringify(data.errors).slice(0, 300));
  return data.data;
}

/** git en una carpeta, devolviendo la salida. */
export function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}
