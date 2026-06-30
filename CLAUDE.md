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

## 7. 適用タイミング

- **新規ファイルを作るとき**：必ずヘッダーを付け、フォルダ README を更新する
- **既存ファイルを編集するとき**：そのファイルのヘッダーが未整備なら合わせて追加する
- **リファクタリングは段階的に**：一度にすべてを移行せず、触ったファイルから順次対応する
