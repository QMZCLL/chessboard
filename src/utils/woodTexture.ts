// 棋盘木纹：在画布上生成一次半透明的纹理（深色纤维与年轮线），叠在棋盘底色上。
// 只生成一次并缓存为图片地址，之后的绘制只是贴图，不使用逐帧计算的滤镜。

const W = 480;
const H = 528;

/** 确定性的伪随机数，保证每次生成的纹理一致。 */
function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** 二维值噪声：在 cols×rows 的格点上取随机值，格点之间平滑插值；输入坐标以格为单位。 */
function valueNoise(cols: number, rows: number, rand: () => number) {
  const grid = new Float32Array((cols + 2) * (rows + 2));
  for (let i = 0; i < grid.length; i++) grid[i] = rand();
  const stride = cols + 2;
  return (x: number, y: number) => {
    const x0 = Math.floor(x);
    const y0 = Math.floor(y);
    const fx = x - x0;
    const fy = y - y0;
    const sx = fx * fx * (3 - 2 * fx);
    const sy = fy * fy * (3 - 2 * fy);
    const i = y0 * stride + x0;
    const a = grid[i] + (grid[i + 1] - grid[i]) * sx;
    const b = grid[i + stride] + (grid[i + stride + 1] - grid[i + stride]) * sx;
    return a + (b - a) * sy;
  };
}

function generate(): string {
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  const rand = mulberry32(20241023);
  // 低频扭曲让年轮线起伏；沿横向拉长的细噪声形成纤维
  const warp = valueNoise(8, 10, rand);
  const warp2 = valueNoise(20, 24, rand);
  const fiber = valueNoise(24, 480, rand);
  const img = ctx.createImageData(W, H);
  const data = img.data;
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const u = x / W;
      const v = y / H;
      const w = warp(u * 7, v * 9) * 0.75 + warp2(u * 19, v * 23) * 0.25;
      // 年轮：沿纵向的条纹，被扭曲后成为不规则的横向纹理
      const t = v * 30 + w * 2.6;
      const ring = Math.pow(Math.abs(Math.sin(t * Math.PI)), 0.35);
      const line = 1 - ring;
      const f = fiber(u * 23, v * 479);
      let a = line * 0.42 + Math.max(0, f - 0.45) * 0.8 + (w - 0.5) * 0.3;
      // 稀疏的导管孔
      if (rand() < 0.0025) a += 0.35;
      a = Math.min(1, Math.max(0, a));
      const k = (y * W + x) * 4;
      data[k] = 78;
      data[k + 1] = 44;
      data[k + 2] = 12;
      data[k + 3] = Math.round(a * 255);
    }
  }
  ctx.putImageData(img, 0, 0);
  return canvas.toDataURL("image/png");
}

let cached: string | null = null;

/** 木纹纹理的图片地址（首次调用时生成）。 */
export function woodTexture(): string {
  if (cached === null) {
    try {
      cached = generate();
    } catch {
      // 画布不可用时只显示纯色棋盘
      cached = "";
    }
  }
  return cached;
}
