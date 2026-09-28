"""Preserve the supplied artwork; put the largest ICO frame first for Tauri."""
import struct
from io import BytesIO
from PIL import Image
from pathlib import Path
from zipfile import ZipFile

root = Path(__file__).resolve().parents[1]
archive = root / 'brand/perch-bird-icons.zip'
files = {
    'perch-app.ico': 'icon.ico',
    'perch-app-32.png': '32x32.png',
    'perch-app-128.png': '128x128.png',
    'perch-app-256.png': 'icon.png',
}
with ZipFile(archive) as package:
    # Read only the named icon files; never extract archive paths directly.
    assets = {target: package.read('perch-bird-icons/' + source) for source, target in files.items()}
for target, data in assets.items():
    if target == 'icon.ico':
        # tauri-codegen 2.x decodes entries()[0] as its window icon. The supplied
        # ICO starts at 16 px; scaling that frame up makes the taskbar blurry.
        # Reorder the directory only. Each original image payload stays intact.
        count = struct.unpack_from('<H', data, 4)[0]
        frames = []
        for n in range(count):
            entry = data[6 + n * 16:22 + n * 16]
            size, offset = struct.unpack_from('<II', entry, 8)
            frames.append((entry, data[offset:offset + size]))
        frames.sort(key=lambda item: (item[0][0] or 256) * (item[0][1] or 256), reverse=True)
        offset = 6 + 16 * count
        entries, payloads = [], []
        for entry, payload in frames:
            entries.append(entry[:12] + struct.pack('<I', offset))
            payloads.append(payload)
            offset += len(payload)
        data = data[:6] + b''.join(entries) + b''.join(payloads)
    else:
        # Tauri's include_image! requires explicit RGBA, not an indexed PNG.
        output = BytesIO()
        Image.open(BytesIO(data)).convert('RGBA').save(output, format='PNG')
        data = output.getvalue()
    (root / 'src-tauri/icons' / target).write_bytes(data)
print('Imported original icon frames; largest frame first:', ', '.join(files.values()))
