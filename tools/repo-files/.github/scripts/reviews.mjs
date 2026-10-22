// Lee las reseñas de la categoría "Reseñas" de Discussions y escribe reviews.json.
// Mismo formato que entiende Dársena (src-tauri/src/reviews.rs):
//   { updatedAt, games: { "<id-juego>": { average, count, discussionUrl, reviews: [{ user, avatar, url, rating, text, date, replies }] } } }
// replies: respuestas de los desarrolladores del port (dueño o contribuidores de su repositorio, según catalog.json).
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const CATEGORY = "reseñas";
const [owner, name] = process.env.REPO.split("/");
const MARK = /<!--\s*darsena:review rating=(\d)\s*-->/;
const REPLY_MARK = "<!-- darsena:reply -->";
// «¿Funciona en tu PC?»: <!-- darsena:compat {"result":"good|ok|bad","port","cpu","gpu","ram"} -->
const COMPAT = /<!--\s*darsena:compat\s*(\{[\s\S]*?\})\s*-->/;

// Desarrolladores de cada juego: dueño y contribuidores de los repos de sus ports.
const catalog = existsSync("catalog.json") ? JSON.parse(readFileSync("catalog.json", "utf8")).games : [];
const contributorsCache = new Map();
async function contributors(repo) {
  if (contributorsCache.has(repo)) return contributorsCache.get(repo);
  const set = new Set([repo.split("/")[0].toLowerCase()]);
  try {
    const res = await fetch(`https://api.github.com/repos/${repo}/contributors?per_page=100`, {
      headers: { Authorization: `Bearer ${process.env.GITHUB_TOKEN}`, "User-Agent": "darsena-reviews" },
    });
    if (res.ok) for (const c of await res.json()) if (c.login) set.add(c.login.toLowerCase());
  } catch {}
  contributorsCache.set(repo, set);
  return set;
}
async function devsOf(gameId) {
  const all = new Set();
  for (const p of catalog.filter((g) => g.game === gameId && g.repo && g.origin !== "external")) {
    for (const l of await contributors(p.repo)) all.add(l);
  }
  return all;
}

async function gql(query, variables) {
  const res = await fetch("https://api.github.com/graphql", {
    method: "POST",
    headers: { Authorization: `Bearer ${process.env.GITHUB_TOKEN}`, "Content-Type": "application/json", "User-Agent": "darsena-reviews" },
    body: JSON.stringify({ query, variables }),
  });
  const json = await res.json();
  if (!res.ok || json.errors) throw new Error(JSON.stringify(json.errors ?? json).slice(0, 500));
  return json.data;
}

function parse(body) {
  const m = body.match(MARK);
  const rating = m && Number(m[1]);
  if (!rating || rating < 1 || rating > 5) return null;
  const text = body
    .split("\n")
    .filter((l) => !l.includes("darsena:review") && !(/^[★☆]/.test(l) && l.includes("/5**")))
    .join("\n")
    .trim();
  return { rating, text };
}

const cats = await gql(
  "query($o:String!,$n:String!){repository(owner:$o,name:$n){discussionCategories(first:25){nodes{id name}}}}",
  { o: owner, n: name },
);
const cat = cats.repository.discussionCategories.nodes.find((c) => c.name.toLowerCase() === CATEGORY);
if (!cat) throw new Error('Falta la categoría "Reseñas" en Discussions');

const games = {};
let after = null;
do {
  const d = await gql(
    `query($o:String!,$n:String!,$c:ID!,$a:String){repository(owner:$o,name:$n){
      discussions(first:30,after:$a,categoryId:$c){pageInfo{hasNextPage endCursor}
        nodes{title url comments(last:100){nodes{body createdAt isMinimized author{login avatarUrl url}
          replies(first:20){nodes{body createdAt isMinimized author{login avatarUrl url}}}}}}}}}`,
    { o: owner, n: name, c: cat.id, a: after },
  );
  const page = d.repository.discussions;
  for (const disc of page.nodes) {
    const id = disc.title.match(/\[([^\[\]]+)\]\s*$/)?.[1];
    if (!id) continue;
    // Una reseña por usuario: la más reciente.
    const byUser = new Map();
    const compatByUser = new Map();
    const devs = await devsOf(id);
    for (const c of disc.comments.nodes) {
      if (c.isMinimized || !c.author) continue;
      const cm = c.body.match(COMPAT);
      if (cm) {
        try {
          const x = JSON.parse(cm[1]);
          if (["good", "ok", "bad"].includes(x.result)) {
            compatByUser.set(c.author.login, {
              result: x.result, port: String(x.port ?? "").slice(0, 80), cpu: String(x.cpu ?? "").slice(0, 120),
              gpu: String(x.gpu ?? "").slice(0, 120), ram: Number(x.ram) || 0, date: c.createdAt,
            });
          }
        } catch {}
        continue;
      }
      const r = parse(c.body);
      if (!r) continue;
      byUser.set(c.author.login, {
        user: c.author.login,
        avatar: c.author.avatarUrl,
        url: c.author.url,
        rating: r.rating,
        text: r.text,
        date: c.createdAt,
        replies: (c.replies?.nodes ?? [])
          .filter((x) => !x.isMinimized && x.author && devs.has(x.author.login.toLowerCase()))
          .map((x) => ({ user: x.author.login, avatar: x.author.avatarUrl, url: x.author.url, text: x.body.replace(REPLY_MARK, "").trim(), date: x.createdAt })),
      });
    }
    const reviews = [...byUser.values()].sort((a, b) => b.date.localeCompare(a.date));
    const reports = [...compatByUser.values()].sort((a, b) => b.date.localeCompare(a.date));
    if (!reviews.length && !reports.length) continue;
    const average = reviews.length ? Math.round((reviews.reduce((a, r) => a + r.rating, 0) / reviews.length) * 100) / 100 : 0;
    games[id] = { average, count: reviews.length, discussionUrl: disc.url, reviews };
    if (reports.length) {
      const n = (r) => reports.filter((x) => x.result === r).length;
      games[id].compat = { good: n("good"), ok: n("ok"), bad: n("bad"), reports: reports.slice(0, 40) };
    }
  }
  after = page.pageInfo.hasNextPage ? page.pageInfo.endCursor : null;
} while (after);

// Solo se reescribe si cambian las reseñas (así no hay un commit cada hora).
const prev = existsSync("reviews.json") ? JSON.parse(readFileSync("reviews.json", "utf8")) : null;
if (JSON.stringify(prev?.games) === JSON.stringify(games)) {
  console.log("reviews.json ya está al día");
} else {
  writeFileSync("reviews.json", JSON.stringify({ updatedAt: new Date().toISOString(), games }, null, 1) + "\n");
  console.log(`reviews.json: ${Object.keys(games).length} juegos con reseñas`);
}
