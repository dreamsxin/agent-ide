"""生成最小占位 PNG 图标"""
import struct
import zlib
import os.path

def create_png(path, w, h):
    """创建纯黑 PNG"""

    def make_chunk(chunk_type, data):
        chunk = chunk_type + data
        return (
            struct.pack(">I", len(data))
            + chunk
            + struct.pack(">I", zlib.crc32(chunk) & 0xFFFFFFFF)
        )

    raw = b"\x00" * (w * h * 4)  # RGBA 全黑
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)  # 8-bit RGBA
    idat = zlib.compress(raw)
    iend = b""

    png_data = (
        b"\x89PNG\r\n\x1a\n"
        + make_chunk(b"IHDR", ihdr)
        + make_chunk(b"IDAT", idat)
        + make_chunk(b"IEND", iend)
    )

    with open(path, "wb") as f:
        f.write(png_data)
    print(f"  Created: {os.path.basename(path)}")


base = r"d:\work\agent-ide\src-tauri\icons"
for size in [(32, 32), (128, 128), (256, 256)]:
    create_png(f"{base}\\{size[0]}x{size[1]}.png", *size)

# Copy 256 as 128x128@2x
import shutil
shutil.copy(f"{base}\\256x256.png", f"{base}\\128x128@2x.png")
print("  Created: 128x128@2x.png (copy)")

# Create minimal .ico (just a header + 1 entry pointing to 32x32 png data)
png32 = open(f"{base}\\32x32.png", "rb").read()
# ICO format: header(6 bytes) + direntry(16 bytes) + image data
ico_header = struct.pack("<HHH", 0, 1, 1)  # reserved=0, type=1(ico), count=1
# direntry: w,h,colors,reserved,planes,bpp,size,offset
direntry = struct.pack("<BBBBHHII", 32, 32, 0, 0, 1, 32, len(png32), 22)
ico_data = ico_header + direntry + png32
with open(f"{base}\\icon.ico", "wb") as f:
    f.write(ico_data)
print("  Created: icon.ico")
