// Lee los mods de la categoría "Taller" de Discussions y escribe mods.json (mismo formato que src-tauri/src/workshop.rs):
//   { updatedAt, mods: [{ id, node, url, author, avatar, createdAt, likes, reports, hidden, repo, game, port, name, description, nsfw, installDir }] }
// - Una discusión cerrada no sale (así se retira un mod).
// - Denuncias: comentarios <!-- darsena:report reason=… -->, una por usuario. Con 3 o más, el mod se oculta hasta revisarlo
//   (para volver a mostrarlo, minimiza los comentarios de denuncia). Con 2 de «nsfw», se marca +18.
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const CATEGORY = "taller";
const [owner, name] = process.env.REPO.split("/");
const MARK = /<!--\s*darsena:mod\s*([\s\S]*?)-->/;
const REPORT = /<!--\s*darsena:report reason=(\w+)\s*-->/;

async function gql(query, variables) {
  const res = await fetch("https://api.github.com/graphql", {
    method: "POST",
    headers: { Authorization: `Bearer ${process.env.GITHUB_TOKEN}`, "Content-Type": "application/json", "User-Agent": "darsena-mods" },
    body: JSON.stringify({ query, variables }),
  });
  const json = await res.json();
  if (!res.ok || json.errors) throw new Error(JSON.stringify(json.errors ?? json).slice(0, 500));
  return json.data;
}

const cats = await gql(
  "query($o:String!,$n:String!){repository(owner:$o,name:$n){discussionCategories(first:25){nodes{id name}}}}",
  { o: owner, n: name },
);
const cat = cats.repository.discussionCategories.nodes.find((c) => c.name.toLowerCase() === CATEGORY);
if (!cat) {
  console.log('Falta la categoría "Taller" en Discussions: no hay mods todavía');
  process.exit(0);
}

const mods = [];
let after = null;
do {
  const d = await gql(
    `query($o:String!,$n:String!,$c:ID!,$a:String){repository(owner:$o,name:$n){
      discussions(first:50,after:$a,categoryId:$c){pageInfo{hasNextPage endCursor}
        nodes{id number url closed createdAt body author{login avatarUrl}
          reactions(content:THUMBS_UP){totalCount}
          comments(last:100){nodes{body isMinimized author{login}}}}}}}`,
    { o: owner, n: name, c: cat.id, a: after },
  );
  const page = d.repository.discussions;
  for (const disc of page.nodes) {
    if (disc.closed || !disc.author) continue;
    let meta;
    try { meta = JSON.parse(disc.body.match(MARK)?.[1] ?? ""); } catch { continue; }
    if (!meta?.repo || !meta?.game || !meta?.name) continue;
    const reporters = new Map();
    for (const c of disc.comments.nodes) {
      const m = !c.isMinimized && c.author && c.body.match(REPORT);
      if (m) reporters.set(c.author.login.toLowerCase(), m[1]);
    }
    const nsfwReports = [...reporters.values()].filter((r) => r === "nsfw").length;
    mods.push({
      id: String(disc.number),
      node: disc.id,
      url: disc.url,
      author: disc.author.login,
      avatar: disc.author.avatarUrl,
      createdAt: disc.createdAt,
      likes: disc.reactions.totalCount,
      reports: reporters.size,
      hidden: reporters.size >= 3,
      repo: meta.repo,
      game: meta.game,
      port: meta.port ?? null,
      name: String(meta.name).slice(0, 80),
      description: String(meta.description ?? "").slice(0, 3000),
      nsfw: !!meta.nsfw || nsfwReports >= 2,
      installDir: meta.installDir ?? "",
    });
  }
  after = page.pageInfo.hasNextPage ? page.pageInfo.endCursor : null;
} while (after);

mods.sort((a, b) => b.createdAt.localeCompare(a.createdAt));
const prev = existsSync("mods.json") ? JSON.parse(readFileSync("mods.json", "utf8")) : null;
if (JSON.stringify(prev?.mods) === JSON.stringify(mods)) {
  console.log("mods.json ya está al día");
} else {
  writeFileSync("mods.json", JSON.stringify({ updatedAt: new Date().toISOString(), mods }, null, 1) + "\n");
  console.log(`mods.json: ${mods.length} mods`);
}
