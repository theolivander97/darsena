// Versiones nuevas de Dársena con la app ya abierta (al arrancar se comprueba en el arranque, ver startup()).
// Una versión obligatoria se instala sola en cuanto no haya ningún juego abierto; una normal solo avisa.
// Se carga después de app.js y comparte sus variables y funciones.

const LIVE_CHECK_EVERY = 3 * 60 * 60 * 1000;
let liveUpdate = null; // la versión que ya se avisó, para no repetirlo

function showLiveUpdateOverlay(upd) {
  let box = $("#upd-live");
  if (!box) {
    box = document.createElement("div");
    box.id = "upd-live";
    box.className = "upd-live";
    document.body.appendChild(box);
  }
  box.innerHTML = `<div class="upd-live-card">
    <h2>${t("Dársena {version} está disponible (tienes la {current})", { version: esc(upd.version), current: esc(upd.current) })}</h2>
    <p class="muted">${t("Esta actualización es obligatoria.")} ${esc(upd.notes || "")}</p>
    <p class="upd-live-state" id="upd-live-state"></p>
    <div class="bar"><div id="upd-live-bar" style="width:0%"></div></div>
  </div>`;
  return box;
}

async function installLiveUpdate(upd) {
  showLiveUpdateOverlay(upd);
  const state_ = $("#upd-live-state");
  // Si hay un juego abierto se espera a que se cierre, para no cortar la partida ni el tiempo de juego.
  while (state.running.size) {
    state_.textContent = t("Cierra el juego para que Dársena pueda actualizarse.");
    await new Promise((r) => setTimeout(r, 2000));
  }
  state_.textContent = t("Descargando e instalando la actualización… Dársena se reiniciará sola.");
  await listen("app-update-progress", (ev) => {
    const [done, total] = ev.payload;
    $("#upd-live-bar").style.width = total ? `${(done / total) * 100}%` : "30%";
  });
  try {
    await invoke("install_app_update");
  } catch (e) {
    state_.textContent = `${t("No se pudo actualizar:")} ${String(e)}`;
    setTimeout(() => installLiveUpdate(upd), 15000);
  }
}

async function checkAppUpdateLive() {
  const upd = await invoke("check_app_update").catch(() => null);
  if (!upd || liveUpdate === upd.version) return;
  liveUpdate = upd.version;
  if (upd.mandatory) installLiveUpdate(upd);
  else toast(t("Hay una versión nueva de Dársena ({version}). Se instalará al abrirla de nuevo.", { version: upd.version }), "ok");
}

setTimeout(checkAppUpdateLive, 30 * 60 * 1000);
setInterval(checkAppUpdateLive, LIVE_CHECK_EVERY);
