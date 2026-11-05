// Web Games: juegos que se juegan en el navegador. Dársena los abre en su propia ventana, sin descargar nada.
// Se carga después de app.js y comparte sus variables y funciones.

const webPorts = () => [...state.byId.values()].filter((p) => p.web);
const webNeedsFiles = (p) => !!p.needs?.length;

function webCardHtml(p) {
  const gm = state.games.get(p.game);
  if (!gm) return "";
  const img = coverImg(gm, "cover-img");
  const pill = webNeedsFiles(p) ? t("TUS ARCHIVOS") : t("ABRE DIRECTO");
  return `<div class="card" data-webgame="${esc(p.id)}">
    <div class="cover ${img ? "has-img" : ""}" style="${coverStyle(gm)}">
      ${img}${img ? `<div class="cover-caption">${esc(gm.title)}</div>` : ""}
      <div class="pills">${isOwned(gm.id) ? `<span class="pill owned">${t("EN BIBLIOTECA")}</span>` : ""}</div>
      <div class="cover-console">WEB · ${pill}</div>
      <div class="cover-title">${esc(gm.title)}</div>
      <div class="cover-port">${esc(p.port)}</div>
    </div>
  </div>`;
}

function renderWebgames() {
  const box = $("#webgames");
  const q = (state.webQuery || "").trim().toLowerCase();
  const ports = webPorts()
    .filter((p) => !q || `${p.title} ${p.port} ${p.author}`.toLowerCase().includes(q))
    .sort((a, b) => a.title.localeCompare(b.title, LANG()));
  box.innerHTML = `<div class="webgames">
    <div class="com-head">
      <div><h1>${t("Web Games")}</h1><p class="muted">${t("Juegos que se juegan en el navegador. Dársena los abre en su propia ventana, sin descargar nada.")}</p></div>
      <input id="web-search" type="search" placeholder="${t("Buscar juego web…")}" value="${esc(state.webQuery || "")}" autocomplete="off" />
    </div>
    <div class="grid">${ports.length ? ports.map(webCardHtml).join("")
      : emptyHtml(t("Sin resultados"), t("Prueba con otro nombre."), "", "grid-column:1/-1")}</div>
  </div>`;
  const input = $("#web-search");
  input.addEventListener("input", () => {
    state.webQuery = input.value;
    const pos = input.selectionStart;
    renderWebgames();
    const again = $("#web-search");
    again.focus();
    again.setSelectionRange(pos, pos);
  });
}

function renderWebgame() {
  const p = state.byId.get(state.webPort);
  const gm = p && state.games.get(p.game);
  if (!p || !gm) { show("webgames"); return; }
  const syn = synopsisOf(gm.id)?.text;
  const owned = isOwned(gm.id);
  const running = state.running.has(p.id);
  $("#webgame").innerHTML = `<div class="webgame">
    ${libHeroHtml(gm).replace(/<\/div>\s*$/, `<button class="web-back" data-web-back="">← ${t("Web Games")}</button></div>`)}
    <div class="lib-bar">
      <button class="btn play" data-web-play="${esc(p.id)}" ${running ? "disabled" : ""}>${running ? t("EJECUTANDO") : icon("play") + t("JUGAR")}</button>
      ${owned ? `<button class="btn" disabled>${icon("check")}${t("EN TU BIBLIOTECA")}</button>`
        : `<button class="btn" data-web-add="${esc(p.id)}">${icon("plus")}${t("AÑADIR A LA BIBLIOTECA")}</button>`}
      <div class="lib-stat"><span>${t("PORT")}</span><b>${esc(p.port)}</b><span class="muted small">${t("por")} ${esc(p.author)}</span></div>
      <div class="lib-stat"><span>${t("REQUISITO")}</span><b>${webNeedsFiles(p) ? t("Tus archivos del juego") : t("Abre directo")}</b></div>
    </div>
    ${syn ? `<p class="synopsis">${esc(syn)}</p>` : ""}
    <p class="req">${t("Este juego se juega en el navegador: Dársena lo abre en su propia ventana y no descarga nada. Tus partidas se guardan en el navegador de Dársena.")}</p>
    ${p.needsNote ? `<p class="muted small">${esc(p.needsNote)}</p>` : ""}
    <p class="muted small">${t("Página del juego")}: <a data-url="${esc(p.web)}">${esc(new URL(p.web).host)}</a>
      ${p.homepage && p.homepage !== p.web ? ` · <a data-url="${esc(p.homepage)}">${t("Proyecto")}</a>` : ""}</p>
  </div>`;
}

document.addEventListener("click", async (e) => {
  const b = e.target.closest("[data-webgame],[data-web-play],[data-web-add],[data-web-back]");
  if (!b) return;
  const d = b.dataset;
  if (d.webgame) { state.webPort = d.webgame; show("webgame"); window.scrollTo(0, 0); }
  else if (d.webBack !== undefined) show("webgames");
  else if (d.webPlay) {
    b.disabled = true;
    try { await call("launch_game", { id: d.webPlay }); } finally { b.disabled = false; }
  } else if (d.webAdd) {
    const p = state.byId.get(d.webAdd);
    if (p) { await call("add_to_library", { game: p.game }); toast(t("Añadido a tu biblioteca"), "ok"); }
  }
});

// La ficha se repinta cuando el juego se abre o se cierra (botón JUGAR) o cambia la biblioteca.
listen("game-state", () => { if (state.view === "webgame") renderWebgame(); });
listen("library-changed", () => { if (state.view === "webgame") renderWebgame(); else if (state.view === "webgames") renderWebgames(); });
