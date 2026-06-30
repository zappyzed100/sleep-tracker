// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// logger.ts — 呼び出し回数カウンター・ログユーティリティ
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : 操作ごとの累計呼び出し回数を管理する callCount 関数と、
//        デバッグ用に全カウントをダンプする dumpCounts 関数を提供する。
//        各フォルダはこれを import して #N 付きログを出力する。
//
// 公開 : callCount, dumpCounts
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

const _counts: Record<string, number> = {};

/**
 * 操作の累計呼び出し回数をインクリメントして返す。
 * ログに `#N` を付けるために各フォルダから呼び出す。
 */
export function callCount(tag: string, op: string): number {
  const key = `${tag}:${op}`;
  return (_counts[key] = (_counts[key] ?? 0) + 1);
}

/**
 * 全操作の呼び出し回数を一括出力する（デバッグ用）。
 */
export function dumpCounts(): void {
  console.log("[core] call counts:", JSON.stringify(_counts, null, 2));
}
