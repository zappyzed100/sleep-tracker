# Sleep Tracker

PC のキーボード・マウス操作の有無からアイドル時間を自動検知し、「PC を長時間触っていない ＝ 睡眠中」という仮定で睡眠時間を記録・可視化・予測する Windows アプリです。

---

## 機能

- **自動記録** — PC 操作がない時間をリアルタイムで睡眠として検知（シャットダウン・スリープも対応）
- **週別カレンダー表示** — 日別の睡眠時間をグラフと棒カレンダーで確認
- **睡眠時間予測** — 「今眠ったら何時間眠れるか」を機械学習モデルで予測
- **GitHub Gist バックアップ** — 睡眠ログを Gist に自動保存。新 PC への移行も手間なし
- **監視中断 / 再開** — UI から手動で睡眠検知を一時停止。iPhone 未使用時や電池切れのバックアップとしても機能
- **iPhone 連携（任意）** — 外出中は誤検知を除外。iOS ショートカットと Gist 経由でリアルタイム連携
- **Android 連携（任意）** — タブレット使用中は PC アイドルを睡眠と判定しない。MacroDroid と Gist 経由で連携

---

## 動作環境

| 項目 | 要件 |
|------|------|
| OS | Windows 10 / 11 (64-bit) |
| Python | 3.11 以上 |
| パッケージ管理 | [uv](https://docs.astral.sh/uv/getting-started/installation/) |
| バックアップ | GitHub アカウント（任意だが強く推奨） |

---

## インストール

### 1. リポジトリをクローン

```bash
git clone https://github.com/zappyzed100/sleep-tracker.git
cd sleep-tracker
```

### 2. uv をインストール（未導入の場合）

```powershell
powershell -c "irm https://astral.sh/uv/install.ps1 | iex"
```

### 3. Python 仮想環境と依存パッケージを構築

```bash
uv sync
```

matplotlib・pandas・scikit-learn・pystray などが `.venv` に自動インストールされます。

### 4. アプリを起動

```bash
uv run python src_python/main.py
```

初回起動時は**設定タブ**が自動で開きます。

---

## 初期設定

### ① GitHub Gist の設定（推奨）

睡眠ログを Gist にバックアップすることで、PC 故障・買い替え時でもデータが消えません。

**Gist を作成する**

1. [gist.github.com](https://gist.github.com) にアクセスしてログイン
2. 任意のファイル名・内容で新しい Gist を作成（Public / Secret どちらでも可）
3. 作成後の URL `https://gist.github.com/ユーザー名/XXXXXXXXXX` の末尾の英数字が **Gist ID**

**Personal Access Token を作成する**

1. GitHub → Settings → Developer Settings → [Personal access tokens (classic)](https://github.com/settings/tokens/new) を開く
2. **Select scopes** で `gist` にチェックを入れる
3. 「Generate token」をクリックしてトークンをコピー（画面を閉じると二度と表示されません）

**アプリの設定タブに入力する**

1. アプリの設定タブ → 「GitHub 連携」セクションを開く
2. **Gist ID** と **Personal Access Token** を入力して「保存」
3. 「接続テスト」で「✓ 接続成功」が表示されれば完了

> `config.json` に Gist ID とトークンが平文保存されます。共有 PC では取り扱いに注意してください。

---

### ② デスクトップショートカットの作成

設定タブの「デスクトップショートカットを作成」ボタンを押すと `睡眠トラッカー.lnk` がデスクトップに作られます。

**タスクバーへのピン留め手順:**

1. ショートカットをダブルクリックしてアプリを起動
2. タスクバーのアイコンを右クリック → 「タスクバーにピン留めする」
3. 以後はタスクバーから 1 クリックで UI を呼び出せます

> アプリを起動した状態でピン留めすると、正しいアイコンで登録されます。

---

## 使い方

アプリは **バックグラウンド監視（タスクトレイ）** と **UI ウィンドウ** の 2 つで構成されます。

| コンポーネント | 役割 |
|--------------|------|
| タスクトレイアイコン（三日月） | 常駐して睡眠を自動記録。PC 起動時に自動起動 |
| UI ウィンドウ | ログ閲覧・予測・設定の確認。必要なときだけ開く |

- UI を閉じても**タスクトレイは動き続けます**（記録は継続）
- タスクトレイを右クリック → 「終了」で**タスクトレイと UI が両方閉じます**
- PC 起動時にタスクトレイが自動起動するよう、Windows スタートアップへの登録は初回起動時に自動で完了します

### 監視の中断と再開

UI のタイトルバーに「**中断する**」ボタンがあります。PC を放置するが睡眠ではない場面（外出・テレビ鑑賞など）で押すと、その間の放置時間が睡眠としてカウントされなくなります。

| 状態 | 表示 | 操作 |
|------|------|------|
| 通常 | 監視中（緑） | 「中断する」を押すと一時停止 |
| 中断中 | 中断中（黄） | 「再開する」を押すと検知が再開 |

状態は PC を再起動しても引き継がれます。iPhone で外出通知を送ったまま電池切れで帰宅できなかった場合も、このボタンで手動再開できます。

---

## iPhone 連携（任意）

外出中に PC を放置しても睡眠と誤検知しないよう、iPhone の位置情報で外出・帰宅を PC へ通知できます。

**仕組み:**

```
iPhone の「ショートカット」アプリ
  └─ 外出 / 帰宅時に GitHub Gist へ自動 PATCH
      └─ PC が定期的に Gist を読み取り
          └─ 外出中の放置時間を睡眠から除外
```

### 外出時オートメーションの作成

iPhone の「ショートカット」アプリ → 下部「オートメーション」タブ → 右上「＋」

| 手順 | 操作 |
|------|------|
| トリガー | 「出発」→ 位置情報：自宅 → **実行方法：すぐに実行** |
| アクション 1 | 「日付」を追加（現在の日付を取得） |
| アクション 2 | 「テキスト」を追加し、下記 JSON を貼り付ける |
| アクション 3 | 「URL の内容を取得」を追加し、下記の通り設定する |

**テキストアクション（JSON）の内容:**

```json
{
  "files": {
    "mobile_event.txt": {
      "content": "LEAVE,ここに日付変数を挿入"
    }
  }
}
```

`ここに日付変数を挿入` の部分を削除し、キーボード上部から「日付」（アクション 1 の出力）を挿入。挿入した `[日付]` をタップして **フォーマット：カスタム** → `yyyy-MM-dd HH:mm:ss` に設定。

**「URL の内容を取得」の設定:**

| 項目 | 値 |
|------|----|
| URL | `https://api.github.com/gists/あなたのGist ID` |
| 方法 | PATCH |
| ヘッダ: Authorization | `Bearer あなたのPersonal Access Token` |
| ヘッダ: User-Agent | `iOS-Shortcut` |
| 要求本文 | 「ファイル」→ 「テキスト」アクションの出力を選択 |

### 帰宅時オートメーションの作成

「出発」→「到着」に変更し、JSON の `LEAVE` を `ARRIVE` に変更するだけで、ほかの設定は同じです。

---

## Android 連携（任意）

Android タブレット（または Android スマートフォン）を操作中に PC を放置しても、睡眠と誤検知しないようにする機能です。MacroDroid と GitHub Gist を経由して PC にハートビートを送信します。

> **iPhone 連携との違い:** iPhone 連携は外出・帰宅で `OUT_START`/`OUT_END` を送信します。Android 連携は「デバイスが使われている」ことを 5 分ごとに PC へ通知するハートビート方式です。最後の通知から 10 分以内であれば、PC のアイドルを睡眠と判定しません。

**仕組み:**

```
Android（MacroDroid）
  └─ 5 分タイマー × 画面点灯中 の条件で GitHub Gist へ PATCH
      └─ PC が 5 分ごとに Gist を読み取り
          └─ 最後の通知から 10 分以内は IDLE_START を発火しない
```

### MacroDroid のインストールと事前設定

1. Google Play から **MacroDroid** をインストール
2. MacroDroid を開き、初回セットアップを完了する

**Xiaomi / MIUI 向けの追加設定（Xiaomi 以外は不要な場合あり）:**

| 設定場所 | 操作 |
|---------|------|
| 設定 → アプリ → MacroDroid → バッテリー | 「制限なし」に変更 |
| 設定 → プライバシー → 特別な権限 → 自動起動 | MacroDroid をオンにする |
| MacroDroid 内 → 設定 → システム | 「起動時に MacroDroid を開始」をオン |

### マクロの作成

MacroDroid を開いて「＋」でマクロを新規作成します。

**① トリガーの設定**

| 項目 | 設定値 |
|------|-------|
| トリガー | タイマー |
| 繰り返し間隔 | 5 分 |

**② 条件の設定**

| 項目 | 設定値 |
|------|-------|
| 条件 | 画面の状態 |
| 状態 | 画面が点灯している |

> 「トリガー」ではなく「条件」として追加してください。画面が消えているときはアクションが実行されません。

**③ アクションの設定**

「HTTP リクエスト」アクションを追加して以下を設定します：

| 項目 | 設定値 |
|------|-------|
| URL | `https://api.github.com/gists/あなたのGist ID` |
| HTTP メソッド | PATCH |
| ヘッダ: Authorization | `Bearer あなたのPersonal Access Token` |
| ヘッダ: Content-Type | `application/json` |
| ボディ | 下記 JSON を入力 |

**ボディの JSON:**

```json
{
  "files": {
    "mobile_event.txt": {
      "content": "SCREEN_ON,[sys_time_ms]"
    }
  }
}
```

`[sys_time_ms]` の部分は MacroDroid の変数に置き換えます。ボディ入力欄で `SCREEN_ON,` の後ろにカーソルを置き、「変数を挿入」→「システム時刻（ミリ秒）」を選択してください。

**Personal Access Token の作成（Gist 連携用トークンと共用可）:**

- GitHub → Settings → Developer Settings → Personal access tokens (classic)
- スコープは `gist` のみチェック（最小権限）
- このトークンはモバイル端末に保存されるため、権限を `gist` に限定しておくこと

### 動作確認

1. MacroDroid でマクロを保存・有効化
2. 「テスト実行」でマクロを手動実行
3. [gist.github.com](https://gist.github.com) で `mobile_event.txt` の内容が `SCREEN_ON,17xxxxxxxxxx` のような数値タイムスタンプになっていれば成功
4. PC の `logs/device_heartbeat.txt` が更新されていることを確認（次回の Gist 同期後）

> PC 側の Gist 同期は 5 分間隔で自動実行されます。動作確認はアプリ起動後 5 分ほど待つか、Gist 連携の「今すぐ同期」ボタンを押してください。

---

## 新 PC への移行

1. このリポジトリをクローンして `uv sync`
2. `uv run python src_python/main.py` で起動
3. 設定タブで Gist ID と Token を入力して保存
4. 過去の睡眠ログが Gist から自動ダウンロードされ、データが復元されます

---

## アンインストール

1. タスクトレイアイコンを右クリック → 「終了」
2. スタートアップ登録を解除（設定タブ → 「PC起動時に自動起動」チェックを外す）
3. デスクトップのショートカット `睡眠トラッカー.lnk` を削除
4. リポジトリフォルダごと削除

---

## 仕組み

### 睡眠検知の状態遷移

```
PC 操作あり (ACTIVE)
  ├─ 20 分間操作なし ──→ IDLE_START（睡眠開始とみなす）
  ├─ スリープ移行 ───→ SUSPEND
  └─ シャットダウン ──→ SHUTDOWN

睡眠中 (SLEEPING)
  ├─ 操作再開 ───────→ IDLE_RESUME（睡眠終了）
  ├─ スリープ復帰 ───→ RESUME
  └─ PC 起動 ────────→ STARTUP
```

### 突然の電源断への対応

1 分ごとにハートビート（最終稼働時刻）を保存しています。次回起動時に 4 時間以上のギャップがある場合、「最後のハートビート時刻 − その時のアイドル時間 ＝ 睡眠開始」として自動補正します。

### 監視中断中の除外

UI の「中断する」ボタンまたは iPhone からの `LEAVE` 受信で中断フラグが立ち、「再開する」または iPhone からの `ARRIVE` を受信するまで放置時間をすべて睡眠から除外します。iPhone の電池切れで `ARRIVE` が届かなかった場合でも、UI の「再開する」で手動解除できます。

### Android デバイス使用中の除外

Android から 5 分ごとに届く `SCREEN_ON` ハートビートを `logs/device_heartbeat.txt` に記録します。最後の記録から 10 分以内は `IDLE_START`（睡眠開始イベント）を発火しません。Android の操作をやめてから 10 分経過すると通常の睡眠検知に戻ります。

### 睡眠時間の予測

| データ量 | モデル |
|---------|-------|
| 10 件未満 | 同時刻帯の過去平均 |
| 10 件以上 | Random Forest Regressor（入眠時刻・曜日・連続覚醒時間を特徴量） |

---

## ファイル構成

```
sleep-tracker/
├── src_python/
│   ├── main.py          # UI（週別グラフ・予測・設定）
│   ├── monitor.py       # バックグラウンド監視（タスクトレイ常駐）
│   ├── database.py      # ログ解析・SQLite 保存・Gist 同期
│   ├── analyzer.py      # 睡眠時間予測モデル
│   ├── lifecycle.py     # 起動・終了同期・ショートカット作成
│   ├── calendar_ui.py   # カスタムカレンダー UI
│   └── gist_setup.py    # Gist 初期作成ヘルパー（GitHub CLI 使用・任意）
├── src_cpp/
│   └── main.cpp         # C++ 版監視サービス（参考実装。現在は Python 版を使用）
├── pyproject.toml       # Python プロジェクト設定
└── config.json          # Gist ID / Token / アイドル閾値（git 管理外）
```
