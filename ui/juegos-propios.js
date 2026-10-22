// Juegos que no están en el catálogo y se añaden a mano a la biblioteca.
// Se carga después de app.js y comparte sus variables y funciones (todo es global, como en app.js).

// ---------- Juegos añadidos a mano (fuera del catálogo) ----------

// Cada juego propio de la biblioteca ("custom-…") se convierte en un juego y un port de mentira para que la biblioteca,
// el perfil y el menú lo traten como cualquier otro. No salen en Explorar (state.gameList es solo el catálogo).
function injectCustomGames() {
  if (!state.games || !state.byId) return;
  const ids = new Set();
  for (const g of state.library || []) {
    if (!g.id.startsWith("custom-")) continue;
    ids.add(g.id);
    if (state.games.has(g.id) && state.games.get(g.id).title === g.title) continue;
    const port = { id: g.id, game: g.id, title: g.title, port: g.port, author: t("tú"), origin: "custom", custom: true, console: "PC", platforms: ["Windows"], needs: [], aliases: [] };
    state.byId.set(g.id, port);
    state.games.set(g.id, { id: g.id, title: g.title, console: "PC", ports: [port], custom: true, _search: g.title.toLowerCase() });
  }
  for (const [id, gm] of state.games) if (gm.custom && !ids.has(id)) { state.games.delete(id); state.byId.delete(id); }
}

async function openAddCustomGame() {
  const exe = await openDialog({ filters: [{ name: tr("Ejecutable"), extensions: ["exe", "bat", "cmd", "lnk"] }] });
  if (!exe) return;
  const guess = exe.split(/[\\/]/).pop().replace(/\.(exe|bat|cmd|lnk)$/i, "").replace(/[-_.]+/g, " ").trim();
  $("#mod-dialog")?.remove();
  const box = document.createElement("div");
  box.id = "mod-dialog";
  box.className = "news-modal";
  box.innerHTML = `<div class="news-dialog small-dialog">
      <div class="news-head"><h2>${t("Añadir un juego")}</h2>
        <div class="muted small">${t("Aparecerá en tu biblioteca con su tiempo de juego, accesos directos y estanterías. Al quitarlo no se borra nada de tu PC.")}</div></div>
      <div class="news-body ed-form">
        <label>${t("Nombre")}<input id="cg-name" type="text" maxlength="80" value="${esc(guess)}"></label>
        <div class="muted small">${esc(exe)}</div>
      </div>
      <div class="news-foot"><button class="btn primary" id="cg-add">${icon("plus")}${t("Añadir a la biblioteca")}</button></div>
    </div>
    <div class="news-side"><button class="news-round" data-dlg-close title="${t("Cerrar")}">${icon("close")}</button></div>`;
  document.body.appendChild(box);
  $("#cg-name").select();
  box.addEventListener("click", async (e) => {
    if (e.target === box || e.target.closest("[data-dlg-close]")) { box.remove(); return; }
    const add = e.target.closest("#cg-add");
    if (!add) return;
    add.disabled = true;
    try {
      const id = await call("add_custom_game", { name: $("#cg-name").value, exe });
      box.remove();
      [state.library, state.owned] = await Promise.all([invoke("get_library"), invoke("get_owned")]);
      injectCustomGames();
      state.libGame = id;
      show("library");
      toast(t("Juego añadido a tu biblioteca"), "ok");
    } catch { add.disabled = false; }
  });
}
