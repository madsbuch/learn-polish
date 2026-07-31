// Generates src-tauri/icons/source.png (1024x1024) with no image dependencies.
// `bun tauri icon` derives every platform icon from it.
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE = 1024;
const out = resolve(dirname(fileURLToPath(import.meta.url)), "../src-tauri/icons/source.png");

/** Signed distance to a rounded rectangle centred at (cx, cy). */
function roundRect(x, y, cx, cy, hw, hh, r) {
  const dx = Math.abs(x - cx) - (hw - r);
  const dy = Math.abs(y - cy) - (hh - r);
  const ox = Math.max(dx, 0);
  const oy = Math.max(dy, 0);
  return Math.min(Math.max(dx, dy), 0) + Math.hypot(ox, oy) - r;
}

/** Antialiased coverage from a signed distance (1 inside, 0 outside). */
const cov = (d) => Math.min(1, Math.max(0, 0.5 - d));

const over = (dst, src, a) => dst * (1 - a) + src * a;

const px = Buffer.alloc(SIZE * SIZE * 4);

for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    const t = y / SIZE;

    // Background: indigo -> violet vertical gradient inside a squircle.
    let r = 0x4f + (0x7c - 0x4f) * t;
    let g = 0x46 + (0x3a - 0x46) * t;
    let b = 0xe5 + (0xed - 0xe5) * t;

    // Flag plate: white over crimson, the Polish colours.
    const plate = cov(roundRect(x, y, SIZE / 2, SIZE / 2, 300, 208, 44));
    if (plate > 0) {
      const white = y < SIZE / 2;
      r = over(r, white ? 0xff : 0xdc, plate);
      g = over(g, white ? 0xff : 0x14, plate);
      b = over(b, white ? 0xff : 0x3c, plate);
    }

    // Speech-bubble tail, so the mark reads as "conversation" not "flag".
    const tail = cov(roundRect(x, y, SIZE / 2 - 150, SIZE / 2 + 250, 74, 74, 24));
    if (tail > 0) {
      r = over(r, 0xdc, tail);
      g = over(g, 0x14, tail);
      b = over(b, 0x3c, tail);
    }

    const alpha = cov(roundRect(x, y, SIZE / 2, SIZE / 2, SIZE / 2, SIZE / 2, 220)) * 255;
    const i = (y * SIZE + x) * 4;
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = alpha;
  }
}

// Raw PNG scanlines: one filter byte (0 = None) per row.
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  raw[y * (SIZE * 4 + 1)] = 0;
  px.copy(raw, y * (SIZE * 4 + 1) + 1, y * SIZE * 4, (y + 1) * SIZE * 4);
}

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // RGBA

mkdirSync(dirname(out), { recursive: true });
writeFileSync(
  out,
  Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]),
);

console.log("wrote", out);
