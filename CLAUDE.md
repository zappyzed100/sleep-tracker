# Sleep Tracker — コーディング規則

## 1. ファイルサイズ

- **1ファイル 500 行以内**
- 超えた場合は責務を見直してファイルを分割する
- CSS はロジックではないため 500 行を超えても許容するが、セクション分割を推奨

---

## 2. フォルダのファイル数

- **1フォルダに README.md を除いて 7 ファイル以内**（5〜6 が理想）
- 8 ファイル以上になったらサブフォルダへの分割を検討する
- ファイル数より「関心事のまとまり」を優先する。3 ファイルでも別の関心事なら別フォルダにしてよい

---

## 3. ファイルヘッダー（先頭ブロック）

全ファイルの先頭に以下の形式でヘッダーを記述する。行数は目安 8〜10 行。

### TypeScript / TSX

```typescript
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// FileName.tsx — 一行での役割説明
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : このファイルが何をするか（1〜3 行）
//
// 依存 : 参照する他フォルダ／モジュール（ないなら省略）
// 公開 : 外部に export するもの（default export のみなら省略）
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Rust

```rust
//! FileName.rs — 一行での役割説明
//!
//! 役割 : このファイルが何をするか（1〜3 行）
//!
//! 依存 : 参照する他モジュール（ないなら省略）
//! 公開 : `pub fn` / `pub struct` として外部に提供するもの
```

### Google Apps Script / JavaScript

```javascript
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// FileName.gs — 一行での役割説明
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// 役割 : このファイルが何をするか（1〜3 行）
//
// エントリポイント : doGet / doPost など外部から呼ばれる関数
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

---

## 4. フォルダ README

**すべてのフォルダ**に `README.md` を配置する。以下の内容を含む：

```markdown
# フォルダ名

一言でこのフォルダの役割を説明する。

## 含まれるファイル

| ファイル | 役割 |
|---------|------|
| Foo.tsx | ～～コンポーネント |
| utils.ts | ～～ユーティリティ |

## 依存関係

- 依存する（import する）フォルダ：`../core`, `../ui` など
- 依存される（import される）フォルダ：`../app` など

## 公開 API（index.ts がある場合）

外部に提供するものだけを列挙する。
```

---

## 5. フォルダ独立性（モジュール指向）

フォルダを「ひとつの責務を持つモジュール」として設計する。
オブジェクト指向でいう「クラス」に相当し、インターフェース・実装・依存関係を明確にする。

### 5-1. 単一責任

- 各フォルダは **ひとつの関心事** だけを担う
- 関心事が増えたらフォルダを分割する
- 「なんでもフォルダ」（utils/, common/, shared/ に何でも詰め込む）を作らない

### 5-2. 公開インターフェース（エントリポイント）

3 ファイル以上のフォルダは **`index.ts`**（Rust は `mod.rs`）でエントリポイントを定義する。

```typescript
// chart/index.ts — このフォルダが外部に公開するものだけを re-export
export { WeeklyChart } from './WeeklyChart';
export type { DayData } from './types';
// 内部ユーティリティ (chartUtils.ts) は export しない
```

外部からのアクセスはエントリポイント経由のみ。内部ファイルへの直接 import は禁止。

```typescript
// ✓ 正しい（エントリポイント経由）
import { WeeklyChart } from '../chart';

// ✗ 禁止（内部ファイルへの直接アクセス）
import { WeeklyChart } from '../chart/WeeklyChart';
```

### 5-3. 依存の方向（レイヤー構造）

依存は **上位レイヤー → 下位レイヤーの一方向のみ**。逆方向・循環は禁止。

```
Layer 3  app/          App.tsx など — 全体を組み合わせる
Layer 2  features/     機能フォルダ — chart, settings, prediction, detail
Layer 1  ui/           汎用 UI     — TimePicker, CalendarPicker など
Layer 0  core/         基盤        — 型定義 (types.ts), ユーティリティ (utils.ts)
```

- Layer 2 は Layer 0〜1 を import してよい
- Layer 1 は Layer 0 のみ import してよい
- Layer 0 は他のレイヤーを import してはならない
- 同レイヤー間の import は原則禁止（やむを得ない場合は README に理由を記載）

### 5-4. カプセル化

- フォルダ内部の実装詳細を外に漏らさない
- `index.ts` に書かれていないものは「フォルダのプライベート」
- 外部が必要とする型・関数だけを公開する

### 5-5. 依存関係の明示

各フォルダの `README.md` に「依存するフォルダ」「依存されるフォルダ」を必ず記載する。
これにより、どこを変更するとどこに影響が出るかが一目でわかるようにする。

---

## 6. 現在の構造とリファクタリング方針

### src_tauri/src/（フロントエンド）

目標フォルダ構造：

```
src/
  core/           Layer 0: types.ts, utils.ts
  ui/             Layer 1: TimePicker, CalendarPicker（汎用 UI）
  chart/          Layer 2: WeeklyChart
  detail/         Layer 2: DayDetail
  prediction/     Layer 2: PredictionCard, PredictionTab
  settings/       Layer 2: Settings
  App.tsx         Layer 3
  App.css
  main.tsx
```

### src_tauri/src-tauri/src/（Rust バックエンド）

現在 `lib.rs` が 1000 行超のため、目標分割構成：

```
src/
  lib.rs          エントリポイント・invoke_handler 登録のみ
  config.rs       AppConfig 読み書き・Drive 設定同期
  events.rs       sleep_events.txt パース・セッション管理
  cloud.rs        モバイルイベント取得・Drive バックアップ・sync_gist
  platform.rs     スタートアップ登録・デスクトップショートカット・監視状態
  monitor.rs      バックグラウンド監視（現状維持）
  prediction.rs   睡眠予測（現状維持）
  main.rs         エントリポイント（現状維持）
```

---

## 7. ログ規則

### 7-1. フォルダタグ

すべてのログに **`[フォルダ名]`** プレフィックスを付ける。
どのフォルダで問題が起きているかをログ検索で即座に特定できるようにする。

| フォルダ / モジュール | タグ |
|----------------------|------|
| src/core/            | `[core]` |
| src/ui/              | `[ui]` |
| src/chart/           | `[chart]` |
| src/detail/          | `[detail]` |
| src/prediction/      | `[prediction]` |
| src/settings/        | `[settings]` |
| src/（App.tsx）      | `[app]` |
| src-tauri/src/events.rs  | `[events]` |
| src-tauri/src/cloud.rs   | `[cloud]` |
| src-tauri/src/config.rs  | `[config]` |
| src-tauri/src/platform.rs| `[platform]` |
| src-tauri/src/monitor.rs | `[monitor]` |
| src-tauri/src/prediction.rs | `[prediction]` |

### 7-2. ログフォーマット

```
[タグ] 操作名: 詳細  (+Xms)
```

- `[タグ]` は 7-1 の固定文字列
- 処理時間は `+Xms` の形式で末尾に付ける（計測した場合のみ）
- エラーは `ERROR` を操作名の前に付ける

**例：**

```
[cloud] fetch_from_cloud: started
[cloud] fetch_from_cloud: 2.4KB received  (+1204ms)
[events] parse_sessions: 1523 events → 47 sessions  (+83ms)
[settings] save_config: saved
[chart] render: 7 days
[app] ERROR fetch_from_cloud: HTTP 401
```

### 7-3. TypeScript での実装パターン

各フォルダに **タグ定数** を定義し、ログ関数を通じて出力する。

```typescript
// chart/index.ts（または各ファイルの先頭）
const TAG = '[chart]';

// 通常ログ
console.log(TAG, 'render:', week.length, 'days');

// エラー
console.error(TAG, 'ERROR render:', err);

// 処理時間の計測（100ms 超えたときだけ出力）
const t0 = performance.now();
// ... 処理 ...
const ms = Math.round(performance.now() - t0);
if (ms > 100) console.warn(TAG, 'slow render:', `+${ms}ms`);
```

### 7-4. Rust での実装パターン

各モジュール（ファイル）に **タグ定数** を定義する。

```rust
const TAG: &str = "[cloud]";

// 通常ログ（stderr → Windowsコンソール / Android logcat）
eprintln!("{} fetch_from_cloud: started", TAG);

// 処理時間の計測
let t0 = std::time::Instant::now();
// ... 処理 ...
let ms = t0.elapsed().as_millis();
eprintln!("{} fetch_from_cloud: {} bytes received  (+{}ms)", TAG, len, ms);

// エラー
eprintln!("{} ERROR fetch_from_cloud: {}", TAG, err);
```

### 7-5. ログ出力の基準

**必ずログを出すもの：**
- エラー・例外（常に `ERROR` プレフィックス付きで出力）
- ネットワーク通信の開始・完了（`[cloud]` タグ）
- ファイル I/O の完了（大きなファイルの読み書き）
- アプリ起動時の主要な初期化ステップ（`[app]`, `[monitor]`）

**処理時間に応じて出力するもの：**

| 処理時間 | 対応 |
|---------|------|
| < 100ms | ログ不要（通常動作） |
| 100ms〜500ms | `console.log` / `eprintln!` で記録（`+Xms` 付き） |
| 500ms〜2000ms | `console.warn` / `eprintln!("... SLOW ...")` で警告 |
| > 2000ms | `console.error` / `eprintln!("... VERY SLOW ...")` でアラート |

**出さなくていいもの：**
- 毎フレームの再レンダリング
- ポーリングの正常完了（heartbeat 書き込みなど）
- 設定の読み込み（軽量なもの）

### 7-6. 呼び出し回数のログ

ログフォーマットに **`#N`**（起動後の累計呼び出し回数）を含める。
「何度も呼ばれすぎていないか」「想定外の再呼び出しが起きていないか」を検出できる。

```
[タグ] 操作名 #N: 詳細  (+Xms)
```

**例：**
```
[cloud] fetch_from_cloud #1: started
[cloud] fetch_from_cloud #1: 2.4KB received  (+1204ms)
[cloud] fetch_from_cloud #2: started          ← 2回目の呼び出し
[events] parse_sessions #3: 47 sessions  (+83ms)
```

#### TypeScript での実装

`core/logger.ts` に共通カウンター関数を定義して全フォルダから使う。

```typescript
// core/logger.ts
const _counts: Record<string, number> = {};

/** 操作の累計呼び出し回数をインクリメントして返す */
export function callCount(tag: string, op: string): number {
  const key = `${tag}:${op}`;
  return (_counts[key] = (_counts[key] ?? 0) + 1);
}

/** 全操作の呼び出し回数を一括出力（デバッグ用） */
export function dumpCounts(): void {
  console.log('[core] call counts:', JSON.stringify(_counts, null, 2));
}
```

各フォルダでの使い方：

```typescript
import { callCount } from '../core/logger';

const TAG = '[cloud]';

async function fetchFromCloud() {
  const n = callCount(TAG, 'fetch_from_cloud');
  console.log(TAG, `fetch_from_cloud #${n}: started`);

  const t0 = performance.now();
  // ... 処理 ...
  const ms = Math.round(performance.now() - t0);
  console.log(TAG, `fetch_from_cloud #${n}: done  (+${ms}ms)`);
}
```

#### Rust での実装

関数内に `static AtomicU64` を置く。グローバルな依存なしで完結する。

```rust
fn fetch_from_cloud_inner() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;

    let t0 = std::time::Instant::now();
    eprintln!("[cloud] fetch_from_cloud #{}: started", n);

    // ... 処理 ...

    eprintln!("[cloud] fetch_from_cloud #{}: done  (+{}ms)", n, t0.elapsed().as_millis());
}
```

#### 異常な呼び出し回数の検出

回数が想定を超えたら警告を出す。

```typescript
const n = callCount(TAG, 'fetch_from_cloud');
if (n > 5) console.warn(TAG, `fetch_from_cloud #${n}: called more than expected`);
```

```rust
if n > 5 {
    eprintln!("[cloud] fetch_from_cloud #{}: WARN called more than expected", n);
}
```

---

### 7-7. Android パフォーマンス計測

Android では WebView の `console.log` が logcat に `chromium:` タグで出力される。
`[タグ]` プレフィックスがあれば `adb logcat | grep "\[cloud\]"` のように絞り込める。

Rust 側の `eprintln!` は logcat に `RustStdoutStderr` または `sleep-tracker` タグで出力される。

```bash
# 特定フォルダのログだけ確認する例
adb logcat | grep "\[cloud\]"
adb logcat | grep "\[events\]"

# パフォーマンス問題を探す
adb logcat | grep -E "\+[0-9]{3,}ms"   # 100ms 以上かかった処理
```

### 7-8. 追加で記録する指標

呼び出し回数・実行時間に加えて、以下の 3 つを必要な箇所で記録する。

#### ① 連続エラー回数

ネットワーク通信の連続失敗を検出する。1 回の失敗は許容、3 回連続でネットワーク障害と判断できる。

```typescript
// cloud/ のどこか
let consecutiveErrors = 0;
const MAX_CONSECUTIVE = 3;

try {
  const res = await fetch(...);
  consecutiveErrors = 0; // 成功したらリセット
} catch (err) {
  consecutiveErrors++;
  console.error(TAG, `fetch_from_cloud: error (${consecutiveErrors}回連続)`, err);
  if (consecutiveErrors >= MAX_CONSECUTIVE) {
    console.error(TAG, `fetch_from_cloud: WARN ${consecutiveErrors}回連続失敗 — ネットワーク障害の可能性`);
  }
}
```

```rust
static CONSECUTIVE_ERRORS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

match fetch_from_cloud_inner() {
    Ok(_)  => { CONSECUTIVE_ERRORS.store(0, Ordering::Relaxed); }
    Err(e) => {
        let n = CONSECUTIVE_ERRORS.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!("[cloud] fetch_from_cloud: error ({}回連続) {}", n, e);
        if n >= 3 {
            eprintln!("[cloud] fetch_from_cloud: WARN {}回連続失敗", n);
        }
    }
}
```

#### ② データサイズ（バイト数）

I/O 操作にバイト数を付ける。ファイルサイズと実行時間を並べることで「どこが重い原因か」がわかる。
`sleep_events.txt` が大きくなるとパース時間に比例して伸びるため、両方の記録が必要。

```typescript
const text = await readFile(...);
console.log(TAG, `read: ${(text.length / 1024).toFixed(1)}KB  (+${ms}ms)`);
// → [events] read: 48.2KB  (+31ms)
```

```rust
let content = fs::read_to_string(path)?;
eprintln!("[events] read: {}KB  (+{}ms)", content.len() / 1024, ms);
```

ネットワーク受信時も同様：

```
[cloud] fetch_from_cloud #1: 2.4KB received  (+1204ms)
```

#### ③ キャッシュヒット / ミス

セッションキャッシュ（`SESSION_CACHE` など）が有効に機能しているかを確認する。
「ミスが多い＝毎回フルパース」という無駄を検出できる。

```typescript
const cached = sessionCache.get(key);
if (cached) {
  console.log(TAG, `parse_sessions: cache HIT (${key})`);
  return cached;
}
console.log(TAG, `parse_sessions: cache MISS (${key}) — parsing...`);
// ... フルパース
```

```rust
if let Some(cached) = SESSION_CACHE.get(&key) {
    eprintln!("[events] parse_sessions: cache HIT");
    return Ok(cached.clone());
}
eprintln!("[events] parse_sessions: cache MISS — parsing file");
```

ログ例：
```
[events] parse_sessions: cache MISS — parsing file
[events] parse_sessions: 47 sessions  (+83ms)
[events] parse_sessions: cache HIT       ← 次回は一瞬
```

**メモリ使用量はログ不要。** この規模のアプリ（sleep_events.txt は数百 KB 以下）ではメモリ問題は起きにくく、問題が起きても「処理時間が遅い」として先に顕在化するため。

---

## 8. 適用タイミング

- **新規ファイルを作るとき**：必ずヘッダーを付け、フォルダ README を更新する
- **既存ファイルを編集するとき**：そのファイルのヘッダーが未整備なら合わせて追加する
- **リファクタリングは段階的に**：一度にすべてを移行せず、触ったファイルから順次対応する

---

## 9. PLAN.md

- ルートの `PLAN.md` を改修するときは [`docs/PLAN_FORMAT.md`](docs/PLAN_FORMAT.md) の様式（目的／
  アーキテクチャ／技術選定理由／運用／ロードマップ／機械可読タスクの節構成）を守る
- 節の追加・削除など様式自体を変える場合は `docs/PLAN_FORMAT.md` 側も合わせて更新する
