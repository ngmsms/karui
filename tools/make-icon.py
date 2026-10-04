"""karui のアイコンを描く。assets/karui.ico（exe 用）と assets/karui.png（README 用）を作る。

    python tools/make-icon.py

角丸の青いタイルに、白い山と黄色い太陽。小さいサイズでも潰れないよう形は大きく単純にし、
絵の部分はタイルの内側に収めて、明るい背景でも輪郭が消えないように青い縁を残している。
Pillow が要る。
"""

from pathlib import Path

from PIL import Image, ImageDraw

S = 1024
SS = 4  # 4 倍で描いて縮小し、輪郭をなめらかにする
W = S * SS
OUT = Path(__file__).resolve().parent.parent / "assets"


def P(*xy):
    return [v * SS for v in xy]


def mask(box, r):
    m = Image.new("L", (W, W), 0)
    ImageDraw.Draw(m).rounded_rectangle(P(*box), radius=r * SS, fill=255)
    return m


def draw():
    tile = mask((64, 64, 960, 960), 200)
    inner = mask((150, 150, 874, 874), 120)

    # 左上が明るい青、右下が濃い青（スライダーの色に近い）
    top, bottom = (110, 182, 255), (52, 86, 214)
    g = Image.linear_gradient("L").resize((W, W))
    diag = Image.blend(g, g.transpose(Image.Transpose.ROTATE_90).transpose(Image.Transpose.FLIP_LEFT_RIGHT), 0.5)
    grad = Image.composite(Image.new("RGB", (W, W), bottom), Image.new("RGB", (W, W), top), diag)

    art = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    d = ImageDraw.Draw(art)
    d.ellipse(P(600, 220, 780, 400), fill=(255, 214, 74, 255))
    d.polygon(P(380, 900, 680, 500, 960, 900), fill=(255, 255, 255, 150))
    d.polygon(P(100, 900, 100, 760, 380, 420, 770, 900), fill=(255, 255, 255, 255))
    art.putalpha(Image.composite(art.getchannel("A"), Image.new("L", (W, W), 0), inner))

    img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
    img.paste(grad, (0, 0), tile)
    img.alpha_composite(art)
    return img.resize((S, S), Image.Resampling.LANCZOS)


if __name__ == "__main__":
    img = draw()
    OUT.mkdir(exist_ok=True)
    img.resize((256, 256), Image.Resampling.LANCZOS).save(OUT / "karui.png")
    img.save(OUT / "karui.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
    print("wrote", OUT / "karui.ico", OUT / "karui.png")
