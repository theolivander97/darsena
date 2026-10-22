// Revisión de los ports que suben sus autores desde Dársena (Discussions, categoría "Ports subidos").
//
// Uso:
//   node tools\catalog\ports-subidos.mjs                      lista los pendientes de revisión
//   node tools\catalog\ports-subidos.mjs ver <n>              muestra todo lo que envió el autor (n = número de la discusión)
//   node tools\catalog\ports-subidos.mjs aprobar <n>          lo añade a catalog/catalog.json (+ sinopsis y tráiler en info-overrides.json),
//                                                             avisa al autor y cierra la discusión como resuelta
//   node tools\catalog\ports-subidos.mjs rechazar <n> "motivo" responde al autor con el motivo y cierra la discusión
//
// Después de aprobar: revisa la ficha y publica como siempre (portadas, informacion.mjs y publicar.mjs).

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fail, gh, OWNER, REPO, ROOT, TOOLS } from "../comun.mjs";

const CATEGORY = "Ports subidos";
const MARK = "darsena:submission";
const CATALOG = join(ROOT, "catalog", "catalog.json");
const OVERRIDES = join(ROOT, "catalog", "info-overrides.json");

async function gql(query, variables = {}) {
  const r = await gh("/graphql", { method: "POST", body: { query, variables } });
  if (r.errors) fail(`GitHub: ${r.errors[0].message}`);
  return r.data;
}

async function category() {
  const d = await gql(`query($o:String!,$r:String!){repository(owner:$o,name:$r){id discussionCategories(first:25){nodes{id name}}}}`, { o: OWNER, r: REPO });
  const cat = d.repository.discussionCategories.nodes.find((c) => c.name.toLowerCase() === CATEGORY.toLowerCase());
  if (!cat) fail(`Falta la categoría "${CATEGORY}" en las Discussions del repo.`);
  return cat.id;
}

async function discussions() {
  const d = await gql(
    `query($o:String!,$r:String!,$c:ID!){repository(owner:$o,name:$r){discussions(first:100,categoryId:$c,orderBy:{field:CREATED_AT,direction:DESC}){nodes{id number title url closed createdAt body author{login}}}}}`,
    { o: OWNER, r: REPO, c: await category() });
  return d.repository.discussions.nodes;
}

function submissionOf(body) {
  const start = body.indexOf(`<!-- ${MARK}`);
  if (start < 0) return null;
  const end = body.indexOf("-->", start);
  try { return JSON.parse(body.slice(start + MARK.length + 5, end).trim()); } catch { return null; }
}

async function find(n) {
  const d = (await discussions()).find((x) => x.number === Number(n));
  if (!d) fail(`No hay ningún port subido con el número ${n}.`);
  const s = submissionOf(d.body);
  if (!s) fail(`La discusión #${n} no tiene los datos del port.`);
  return { d, s };
}

async function reply(d, text, reason) {
  await gql(`mutation($id:ID!,$b:String!){addDiscussionComment(input:{discussionId:$id,body:$b}){comment{id}}}`, { id: d.id, b: text });
  await gql(`mutation($id:ID!,$r:DiscussionCloseReason!){closeDiscussion(input:{discussionId:$id,reason:$r}){discussion{id}}}`, { id: d.id, r: reason });
}

const youtubeId = (v) => {
  const m = String(v || "").match(/(?:v=|youtu\.be\/|\/embed\/|\/shorts\/)([\w-]{11})/) || String(v || "").match(/^([\w-]{11})$/);
  return m ? m[1] : null;
};

const [cmd, n, ...rest] = process.argv.slice(2);

if (!cmd) {
  const open = (await discussions()).filter((d) => !d.closed);
  if (!open.length) console.log("No hay ports pendientes de revisión.");
  for (const d of open) {
    const s = submissionOf(d.body);
    console.log(`#${d.number}  ${s?.title ?? d.title}  ·  ${s?.console ?? "?"}  ·  ${s?.repo ?? "?"}  ·  @${d.author?.login}  ·  ${d.createdAt.slice(0, 10)}`);
  }
  if (open.length) console.log(`\nVer:      node tools\\catalog\\ports-subidos.mjs ver <n>\nAprobar:  node tools\\catalog\\ports-subidos.mjs aprobar <n>\nRechazar: node tools\\catalog\\ports-subidos.mjs rechazar <n> "motivo"`);
} else if (cmd === "ver") {
  const { d, s } = await find(n);
  console.log(`#${d.number} · ${d.url}\nSubido por @${d.author?.login} el ${d.createdAt.slice(0, 10)}${d.closed ? " (ya cerrado)" : ""}\n`);
  console.log(JSON.stringify(s, null, 2));
} else if (cmd === "aprobar") {
  const { d, s } = await find(n);
  if (d.closed) fail(`#${n} ya está cerrado.`);
  const catalog = JSON.parse(readFileSync(CATALOG, "utf8"));
  if (catalog.games.some((g) => g.repo?.toLowerCase() === s.repo.toLowerCase())) fail(`${s.repo} ya está en el catálogo.`);
  const entry = {
    title: s.title, repo: s.repo, console: s.console,
    ...(s.franchise ? { franchise: s.franchise } : {}),
    ...(s.aliases?.length ? { aliases: s.aliases } : {}),
    ...(s.needs?.length ? { needs: s.needs } : {}),
    ...(s.needsNote ? { needsNote: s.needsNote } : {}),
    platforms: s.platforms?.length ? s.platforms : ["Windows"],
    experimental: !!s.experimental,
    ...(s.ai ? { ai: s.ai } : {}),
    // Sin build en releases: Dársena instala el código de la rama y juega con su ejecutable.
    ...(s.appInRepo ? { branch: s.appInRepo.branch, exe: s.appInRepo.exe } : {}),
    ...(s.appInRepo?.setupExe ? { setup: { exe: s.appInRepo.setupExe, done: ["CAMBIAR: archivo que existe cuando ya está preparado"] } } : {}),
  };
  if (s.appInRepo?.setupExe) console.log(`⚠ Revisa "setup.done" en el catálogo: pon un archivo que solo exista cuando ${s.appInRepo.setupExe} terminó.`);
  // Una línea más al principio de la lista; actualizar.mjs valida, completa y ordena.
  const text = readFileSync(CATALOG, "utf8").replace(/"games": \[\n/, `"games": [\n${JSON.stringify(entry)},\n`);
  writeFileSync(CATALOG, text);
  execFileSync("node", [join(TOOLS, "catalog", "actualizar.mjs")], { stdio: "inherit" });
  // Sinopsis y tráiler del autor: a mano en info-overrides.json (ganan a Wikipedia).
  const port = JSON.parse(readFileSync(CATALOG, "utf8")).games.find((g) => g.repo?.toLowerCase() === s.repo.toLowerCase());
  const yt = youtubeId(s.trailer);
  if (port && (s.synopsis || yt)) {
    const ov = JSON.parse(readFileSync(OVERRIDES, "utf8"));
    const g = (ov.games[port.game] ||= {});
    if (s.synopsis && !g.synopsis) g.synopsis = { text: s.synopsis, source: { name: `@${d.author?.login}` } };
    if (yt && !g.trailers) g.trailers = [{ kind: "youtube", id: yt, title: "Tráiler", titleEn: "Trailer" }];
    writeFileSync(OVERRIDES, JSON.stringify(ov, null, 2) + "\n");
  }
  await reply(d, `✅ **Aprobado.** ${s.title} ya está en el catálogo de Dársena: aparecerá en los launchers en cuanto se publique el catálogo. ¡Gracias por tu port!`, "RESOLVED");
  console.log(`\n✔ #${n} aprobado: ${s.title} (${s.repo}) añadido al catálogo como ${port?.id}.`);
  console.log("  Siguiente: portadas (tools\\covers), node tools\\catalog\\informacion.mjs y node tools\\catalog\\publicar.mjs");
} else if (cmd === "rechazar") {
  const reason = rest.join(" ").trim();
  if (!reason) fail('Escribe el motivo: node tools\\catalog\\ports-subidos.mjs rechazar <n> "motivo"');
  const { d, s } = await find(n);
  if (d.closed) fail(`#${n} ya está cerrado.`);
  await reply(d, `❌ **No aprobado por ahora.**\n\n${reason}\n\nCuando lo corrijas, puedes volver a subirlo desde Dársena.`, "OUTDATED");
  console.log(`✔ #${n} rechazado (${s.title}). El autor verá el motivo en «Mis ports subidos».`);
} else {
  fail(`Orden desconocida: ${cmd}. Usa: (nada), ver, aprobar o rechazar.`);
}
