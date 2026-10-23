// 棋盘显示用的工具函数。规则判断都在后台完成，这里只做显示所需的换算。

import type { Color, Move, Score, Square } from "../api/types";

export const START_BOARD = "rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR";
export const EMPTY_KINGS_BOARD = "4k4/9/9/9/9/9/9/9/9/4K4";

/** 坐标 → 棋子字母 */
export type BoardMap = Map<Square, string>;

export function squareOf(file: number, rank: number): Square {
  return `${String.fromCharCode(97 + file)}${rank}`;
}

export function fileOf(sq: Square): number {
  return sq.charCodeAt(0) - 97;
}

export function rankOf(sq: Square): number {
  return Number(sq[1]);
}

export function parseBoard(fen: string): BoardMap {
  const map: BoardMap = new Map();
  const placement = fen.trim().split(/\s+/)[0] ?? "";
  const rows = placement.split("/");
  rows.forEach((row, i) => {
    const rank = 9 - i;
    let file = 0;
    for (const ch of row) {
      if (/[1-9]/.test(ch)) file += Number(ch);
      else {
        if (file < 9 && rank >= 0) map.set(squareOf(file, rank), ch);
        file += 1;
      }
    }
  });
  return map;
}

export function boardToFen(map: BoardMap): string {
  const rows: string[] = [];
  for (let rank = 9; rank >= 0; rank--) {
    let row = "";
    let empty = 0;
    for (let file = 0; file < 9; file++) {
      const p = map.get(squareOf(file, rank));
      if (p) {
        if (empty) row += String(empty);
        empty = 0;
        row += p;
      } else empty += 1;
    }
    if (empty) row += String(empty);
    rows.push(row);
  }
  return rows.join("/");
}

export function sideOfFen(fen: string): Color | null {
  const side = fen.trim().split(/\s+/)[1];
  if (side === "w" || side === "r") return "red";
  if (side === "b") return "black";
  return null;
}

export function placementOf(fen: string): string {
  return fen.trim().split(/\s+/)[0] ?? "";
}

/** 在显示用的棋盘上走一步（不检查规则，走法已由后台校验）。 */
export function applyMove(map: BoardMap, mv: Move): BoardMap {
  const next = new Map(map);
  const from = mv.slice(0, 2);
  const to = mv.slice(2, 4);
  const piece = next.get(from);
  next.delete(from);
  if (piece) next.set(to, piece);
  return next;
}

export function applyLine(fen: string, moves: Move[]): string {
  let map = parseBoard(fen);
  for (const mv of moves) map = applyMove(map, mv);
  return boardToFen(map);
}

export function isRedPiece(ch: string): boolean {
  return ch === ch.toUpperCase();
}

const NAMES: Record<string, string> = {
  K: "帅",
  A: "仕",
  B: "相",
  N: "马",
  R: "车",
  C: "炮",
  P: "兵",
  k: "将",
  a: "士",
  b: "象",
  n: "马",
  r: "车",
  c: "炮",
  p: "卒",
};

export function pieceName(ch: string): string {
  return NAMES[ch] ?? "?";
}

export const PALETTE = ["K", "A", "B", "N", "R", "C", "P", "k", "a", "b", "n", "r", "c", "p"];

export function sideLabel(side: Color): string {
  return side === "red" ? "红方" : "黑方";
}

/** 着法前的回合编号，如“3.”（红）或“3…”（黑）。 */
export function moveNumber(ply: number, rootSide: Color, mover: Color): string {
  const n = Math.floor((ply - 1 + (rootSide === "black" ? 1 : 0)) / 2) + 1;
  return mover === "red" ? `${n}.` : `${n}…`;
}

export function scoreText(score: Score): string {
  if (score.kind === "mate") {
    if (score.value > 0) return `${score.value} 步杀`;
    if (score.value < 0) return `${-score.value} 步被杀`;
    return "无着可走";
  }
  return score.value > 0 ? `+${score.value}` : String(score.value);
}

const CN = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

/** 某方视角的纵线编号。 */
export function fileLabel(side: Color, file: number): string {
  return side === "red" ? CN[9 - file] : String(file + 1);
}
