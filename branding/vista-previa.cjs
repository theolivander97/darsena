// Genera la vista previa social de GitHub (1280x640): logo con círculo, nombre y lema.
// Uso: node branding\vista-previa.cjs   → vista-previa-github.png (inglés) y vista-previa-github-es.png
const sharp = require("D:/Proyecto PortLauncher/tools/covers/node_modules/sharp");
const { join } = require("node:path");

const HERE = __dirname;
const LEMAS = {
  "vista-previa-github.png": ["Launcher for community-made", "video game ports"],
  "vista-previa-github-es.png": ["Launcher para ports de videojuegos", "hechos por la comunidad"],
};

(async () => {
  const logo = await sharp(join(HERE, "darsena-logo.svg"), { density: 300 }).resize(360, 360).png().toBuffer();
  for (const [file, [l1, l2]] of Object.entries(LEMAS)) {
    const text = `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640">
      <rect width="1280" height="640" fill="#111111"/>
      <rect y="626" width="1280" height="14" fill="#ffc20e"/>
      <text x="542" y="302" fill="#ffffff" font-family="Segoe UI" font-weight="700" font-size="104" letter-spacing="9">DÁRSENA</text>
      <text x="544" y="372" fill="#ffc20e" font-family="Segoe UI" font-size="34">${l1}</text>
      <text x="544" y="420" fill="#ffc20e" font-family="Segoe UI" font-size="34">${l2}</text>
    </svg>`;
    await sharp(Buffer.from(text)).composite([{ input: logo, left: 120, top: 140 }]).png().toFile(join(HERE, file));
    console.log("✔", file);
  }
})();
