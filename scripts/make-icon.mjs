/**
 * Generates src-tauri/icon-source.png — a 512x512 placeholder app icon.
 *
 * Written by hand rather than pulled from a dependency so the scaffold has no
 * image toolchain. Run `npm run icon` afterwards to expand it into the .ico
 * and .png set Tauri bundles.
 */
import { deflateSync } from 'node:zlib';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const SIZE = 512;
const OUT = resolve(dirname(fileURLToPath(import.meta.url)), '../src-tauri/icon-source.png');

// Tokens from src/app.css.
const SURFACE = [0x1a, 0x1b, 0x1e];
const LANE_1 = [0x58, 0xa6, 0xff];
const LANE_2 = [0x56, 0xd3, 0x64];

const pixels = new Float32Array(SIZE * SIZE * 4);

function blend(x, y, [r, g, b], alpha) {
  if (alpha <= 0 || x < 0 || y < 0 || x >= SIZE || y >= SIZE) return;
  const a = Math.min(alpha, 1);
  const i = (y * SIZE + x) * 4;
  pixels[i] = pixels[i] * (1 - a) + r * a;
  pixels[i + 1] = pixels[i + 1] * (1 - a) + g * a;
  pixels[i + 2] = pixels[i + 2] * (1 - a) + b * a;
  pixels[i + 3] = pixels[i + 3] * (1 - a) + 255 * a;
}

/** Coverage from a signed distance, giving cheap antialiasing. */
function coverage(distance) {
  return Math.min(Math.max(0.5 - distance, 0), 1);
}

function roundedRect(x0, y0, x1, y1, radius, colour) {
  for (let y = Math.floor(y0) - 1; y <= Math.ceil(y1) + 1; y++) {
    for (let x = Math.floor(x0) - 1; x <= Math.ceil(x1) + 1; x++) {
      const px = x + 0.5;
      const py = y + 0.5;
      const dx = Math.max(x0 + radius - px, 0, px - (x1 - radius));
      const dy = Math.max(y0 + radius - py, 0, py - (y1 - radius));
      blend(x, y, colour, coverage(Math.hypot(dx, dy) - radius));
    }
  }
}

function circle(cx, cy, radius, colour) {
  for (let y = Math.floor(cy - radius) - 1; y <= Math.ceil(cy + radius) + 1; y++) {
    for (let x = Math.floor(cx - radius) - 1; x <= Math.ceil(cx + radius) + 1; x++) {
      blend(x, y, colour, coverage(Math.hypot(x + 0.5 - cx, y + 0.5 - cy) - radius));
    }
  }
}

function segment(x0, y0, x1, y1, width, colour) {
  const half = width / 2;
  const minX = Math.floor(Math.min(x0, x1) - half) - 1;
  const maxX = Math.ceil(Math.max(x0, x1) + half) + 1;
  const minY = Math.floor(Math.min(y0, y1) - half) - 1;
  const maxY = Math.ceil(Math.max(y0, y1) + half) + 1;
  const vx = x1 - x0;
  const vy = y1 - y0;
  const lengthSq = vx * vx + vy * vy || 1;

  for (let y = minY; y <= maxY; y++) {
    for (let x = minX; x <= maxX; x++) {
      const px = x + 0.5 - x0;
      const py = y + 0.5 - y0;
      const t = Math.min(Math.max((px * vx + py * vy) / lengthSq, 0), 1);
      const distance = Math.hypot(px - vx * t, py - vy * t);
      blend(x, y, colour, coverage(distance - half));
    }
  }
}

// A git graph in miniature: one lane with three commits, one branch that
// leaves the first and rejoins at the last.
roundedRect(16, 16, SIZE - 16, SIZE - 16, 96, SURFACE);

const laneX = 190;
const branchX = 322;
const top = 120;
const middle = 256;
const bottom = 392;

segment(laneX, top, laneX, bottom, 22, LANE_1);
segment(laneX, top, branchX, middle, 22, LANE_2);
segment(branchX, middle, laneX, bottom, 22, LANE_2);

circle(laneX, top, 38, LANE_1);
circle(laneX, middle, 38, LANE_1);
circle(laneX, bottom, 38, LANE_1);
circle(branchX, middle, 38, LANE_2);

// --- PNG encoding ---------------------------------------------------------

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function crc32(buffer) {
  let c = 0xffffffff;
  for (const byte of buffer) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

// One filter byte (0 = None) per scanline, then RGBA.
const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
let offset = 0;
for (let y = 0; y < SIZE; y++) {
  raw[offset++] = 0;
  for (let x = 0; x < SIZE; x++) {
    const i = (y * SIZE + x) * 4;
    raw[offset++] = Math.round(pixels[i]);
    raw[offset++] = Math.round(pixels[i + 1]);
    raw[offset++] = Math.round(pixels[i + 2]);
    raw[offset++] = Math.round(pixels[i + 3]);
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // colour type: RGBA
// bytes 10-12: deflate compression, adaptive filtering, no interlace — all 0.

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk('IHDR', ihdr),
  chunk('IDAT', deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]);

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, png);
console.log(`wrote ${OUT} (${SIZE}x${SIZE}, ${png.length} bytes)`);
