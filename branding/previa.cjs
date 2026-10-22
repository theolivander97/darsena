const sharp = require("D:/Proyecto PortLauncher/tools/covers/node_modules/sharp");
(async () => {
  const tiles = [];
  for (const [file, bg] of [["darsena-logo.svg", "#111111"], ["darsena-marca.svg", "#111111"], ["darsena-marca.svg", "#ffffff"]]) {
    for (const size of [256, 48, 24]) {
      const img = await sharp(file, { density: 300 }).resize(size, size).png().toBuffer();
      const tile = await sharp({ create: { width: 280, height: 280, channels: 4, background: bg } })
        .composite([{ input: img, gravity: "center" }]).png().toBuffer();
      tiles.push(tile);
    }
  }
  const W = 280 * 3, H = 280 * 3;
  await sharp({ create: { width: W, height: H, channels: 4, background: "#333" } })
    .composite(tiles.map((t, i) => ({ input: t, left: (i % 3) * 280, top: Math.floor(i / 3) * 280 })))
    .png().toFile("previa.png");
  console.log("ok");
})();
