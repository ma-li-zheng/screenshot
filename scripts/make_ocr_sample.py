# 生成 OCR 测试图：模拟聊天/网页截图里的中英文混排
import sys
from PIL import Image, ImageDraw, ImageFont
out = sys.argv[1]
img = Image.new("RGB", (900, 360), (255, 255, 255))
d = ImageDraw.Draw(img)
f = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 22)
s = ImageFont.truetype("C:/Windows/Fonts/msyh.ttc", 14)
lines = [
    ("截图工具支持文字识别和翻译功能。", f),
    ("Screenshot tool supports OCR and translation.", f),
    ("订单编号：20261102-8848，金额 ¥1,299.00", f),
    ("小号文字也需要能够识别出来，例如 12px 的说明。", s),
]
y = 30
for t, font in lines:
    d.text((40, y), t, fill=(30, 30, 30), font=font)
    y += 70
img.save(out)
