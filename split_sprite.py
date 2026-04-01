try:
    from PIL import Image
except ImportError:
    raise SystemExit("Pillow não instalado. Execute: pip install pillow")

from pathlib import Path

INPUT = Path("src/images/Terran.png")
OUTPUT_DIR = Path("src/images")
NUM_COLS = 4

UNITS = [
    "auto_turret", "marauder", "raven", "viking",
    "banshee", "marine", "reaper", "hellbat",
    "battlecruiser", "medivac_dropship", "scv", "widow_mine",
    "ghost", "mule", "siege_tank", "cyclone",
    "hellion", "point_defense_drone", "thor", "liberator",
    "unknown_21", "unknown_22", "unknown_23", "unknown_24",
]

img = Image.open(INPUT)
cell_w = img.width // NUM_COLS
num_rows = img.height // cell_w

assert img.width % NUM_COLS == 0, f"Largura {img.width} não divisível por {NUM_COLS}"
assert img.height % cell_w == 0, f"Altura {img.height} não divisível por {cell_w}"
assert len(UNITS) == num_rows * NUM_COLS, (
    f"Lista de unidades ({len(UNITS)}) não bate com grid {num_rows}x{NUM_COLS}={num_rows * NUM_COLS}"
)

print(f"Imagem: {img.width}x{img.height} | Grid: {num_rows} linhas x {NUM_COLS} colunas | Célula: {cell_w}x{cell_w}px")

for idx, name in enumerate(UNITS):
    row, col = divmod(idx, NUM_COLS)
    box = (col * cell_w, row * cell_w, (col + 1) * cell_w, (row + 1) * cell_w)
    cell = img.crop(box)
    out = OUTPUT_DIR / f"{name}.png"
    cell.save(out)
    print(f"  Salvo: {out}")

print(f"\nConcluído: {len(UNITS)} imagens salvas em {OUTPUT_DIR}/")
