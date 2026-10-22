# Vectoriza el logo de Dársena a partir de la imagen incrustada en Darsena_logo.svg.
# Salida: darsena-logo.svg (círculo blanco + barco) y darsena-marca.svg (solo el barco y el mar, fondo transparente).
import re
from statistics import median

import vtracer
from PIL import Image, ImageFilter

HERE = r"D:\Proyecto PortLauncher\branding"
SCALE = 4  # se amplía antes de trazar: bordes más suaves

rgb = Image.open(rf"{HERE}\logo-incrustado-1.png").convert("RGB")
alpha = Image.open(rf"{HERE}\logo-incrustado-0.png").convert("L")
w, h = rgb.size

# Píxeles amarillos (y el color típico del amarillo del logo).
def is_yellow(p):
    r, g, b = p
    return r > 170 and g > 140 and b < 130 and r - b > 90

px, ax = rgb.load(), alpha.load()
yellow = [px[x, y] for y in range(h) for x in range(w) if ax[x, y] > 128 and is_yellow(px[x, y])]
color = tuple(int(median(c[i] for c in yellow)) for i in range(3))
hexcolor = "#%02x%02x%02x" % color
print("amarillo del logo:", hexcolor, "| píxeles:", len(yellow))

# Máscara del amarillo, ampliada y suavizada; negro = forma.
big = rgb.resize((w * SCALE, h * SCALE), Image.LANCZOS)
abig = alpha.resize((w * SCALE, h * SCALE), Image.LANCZOS)
bpx, bax = big.load(), abig.load()
mask = Image.new("L", big.size, 255)
mpx = mask.load()
for y in range(big.size[1]):
    for x in range(big.size[0]):
        if bax[x, y] > 128 and is_yellow(bpx[x, y]):
            mpx[x, y] = 0
mask = mask.filter(ImageFilter.MedianFilter(5))
mask.save(rf"{HERE}\mascara-amarillo.png")

vtracer.convert_image_to_svg_py(
    rf"{HERE}\mascara-amarillo.png", rf"{HERE}\trazado.svg",
    colormode="binary", mode="spline", filter_speckle=40, corner_threshold=60,
    length_threshold=4.0, splice_threshold=45, path_precision=2,
)
svg = open(rf"{HERE}\trazado.svg", encoding="utf8").read()
elements = re.findall(r'<path[^>]*>', svg)
print("trazos:", len(elements))
parts = []
for el in elements:
    d = re.search(r'd="([^"]+)"', el).group(1)
    tr = re.search(r'transform="([^"]+)"', el)
    t = f' transform="{tr.group(1)}"' if tr else ""
    parts.append(f'<path d="{d}"{t}/>')
shape = "\n    ".join(parts)

# Círculo: caja de la parte opaca de la imagen original.
bbox = alpha.point(lambda v: 255 if v > 128 else 0).getbbox()
cx = (bbox[0] + bbox[2]) / 2 * SCALE
cy = (bbox[1] + bbox[3]) / 2 * SCALE
r = max(bbox[2] - bbox[0], bbox[3] - bbox[1]) / 2 * SCALE
print("círculo:", round(cx), round(cy), "r", round(r))
side = w * SCALE

# Versión con círculo: lienzo cuadrado ajustado al círculo.
pad = 0
vb = f"{cx - r - pad:.1f} {cy - r - pad:.1f} {2 * (r + pad):.1f} {2 * (r + pad):.1f}"
open(rf"{HERE}\darsena-logo.svg", "w", encoding="utf8").write(f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}">
  <circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r:.1f}" fill="#ffffff"/>
  <g fill="{hexcolor}" fill-rule="evenodd">
    {shape}
  </g>
</svg>
''')

# Versión sin círculo: solo las formas amarillas, ajustada a su caja con un margen.
mb = Image.open(rf"{HERE}\mascara-amarillo.png").point(lambda v: 255 if v < 128 else 0).getbbox()
m = 0.06 * max(mb[2] - mb[0], mb[3] - mb[1])
size = max(mb[2] - mb[0], mb[3] - mb[1]) + 2 * m
mx = (mb[0] + mb[2]) / 2 - size / 2
my = (mb[1] + mb[3]) / 2 - size / 2
open(rf"{HERE}\darsena-marca.svg", "w", encoding="utf8").write(f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="{mx:.1f} {my:.1f} {size:.1f} {size:.1f}">
  <g fill="{hexcolor}" fill-rule="evenodd">
    {shape}
  </g>
</svg>
''')
print("listo")
