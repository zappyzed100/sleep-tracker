# Sleep Tracker (睡眠時間記録アプリ)

PCの操作状態（キーボード・マウスの入力）および電源状態（スリープ、シャットダウン）から、ユーザーがPCを触っていない時間を「睡眠時間」と仮定して自動記録し、将来の睡眠時間を予測するアプリケーションです。

---

## フォルダ構成とナビゲーション

LLMや開発者がこのリポジトリを階層的に探索しやすくするためのディレクトリ構造とドキュメントの配置情報です。

```
sleep-tracker/
│
├── .agents/
│   └── AGENTS.md             # エージェント共通コーディング規則 (Antigravity & Claude Code)
│
├── README.md                 # 本ファイル (プロジェクト全体概要)
├── pyproject.toml            # Pythonプロジェクト設定 (uv)
│
├── src_cpp/                  # C++ 監視サービス
│   ├── README.md             # C++モジュールの説明とビルド方法
│   └── (C++ソースファイル)
│
└── src_python/               # Python 解析・UI・予測ツール
    ├── README.md             # Pythonモジュールの説明と実行方法
    └── (Pythonソースファイル)
```

### ディレクトリの巡回手順
1. まずルートの [README.md](file:///c:/code/lifestyle/sleep-tracker/README.md)（本ファイル）を読み、全体の役割を理解します。
2. コーディング規則やエージェント間ルールを確認する場合は、[.agents/AGENTS.md](file:///c:/code/lifestyle/sleep-tracker/.agents/AGENTS.md) を参照します。
3. Windows APIによるPC状態の常時監視とハートビート出力のロジックは [src_cpp/README.md](file:///c:/code/lifestyle/sleep-tracker/src_cpp/README.md) に従い、`src_cpp/` ディレクトリ配下を調査します。
4. ログ解析、睡眠予測、およびユーザーインターフェース（ログ表示）は [src_python/README.md](file:///c:/code/lifestyle/sleep-tracker/src_python/README.md) に従い、`src_python/` ディレクトリ配下を調査します。

---

## 動作要件
- OS: Windows 10 / 11
- C++ビルド環境: MSVC (cl) または GCC (g++)
- Python環境: Python 3.12+ (uv ツールを使用)

---

## 要望と実装ステータス一覧

現在の実装状況。

### 核心機能（睡眠の記録と検知）
- [x] **睡眠時間の記録**
  - **実装状態**: 実装済み。PC未使用時間および電源オフ期間を自動で記録します。
  - **関連コード**: [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py)
- [x] **PC未使用時は眠っていると仮定**
  - **実装状態**: 実装済み。一定時間（20分）PC操作がない場合に睡眠とみなします。
  - **関連コード**: [main.cpp](file:///c:/code/lifestyle/sleep-tracker/src_cpp/main.cpp), [monitor.py](file:///c:/code/lifestyle/sleep-tracker/src_python/monitor.py)
- [x] **PCの電源が入っていても長時間放置されたら睡眠と判定**
  - **実装状態**: 実装済み。`IDLE_START`から`IDLE_RESUME`までの時間を抽出してSQLiteへ記録。
  - **関連コード**: [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py)
- [x] **PC電源オフ（スリープ・シャットダウン）も睡眠として扱う**
  - **実装状態**: 実装済み。電源イベント監視に加え、突然の電源断でも1分ごとの「生存信号（ハートビート）」の途絶と次回起動時刻のギャップから睡眠時間を自動逆算します。
  - **関連コード**: [main.cpp](file:///c:/code/lifestyle/sleep-tracker/src_cpp/main.cpp) (ハートビート送信), [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py) (ギャップ解析)
- [x] **iPhone GPS による外出検知と睡眠除外**
  - **実装状態**: 実装済み。GitHub Gist を中継して iPhone の位置情報トリガーから外出・帰宅をPCに同期。外出期間中にPCが放置されても、睡眠時間としてカウントせず自動除外します。
  - **関連コード**: [gist_setup.py](file:///c:/code/lifestyle/sleep-tracker/src_python/gist_setup.py) (中継自動構築), [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py) (Gist同期 & 外出除外ロジック)

### 自動化とアクセス性
- [x] **PC起動時の自動実行**
  - **実装状態**: 実装済み。Windowsスタートアップフォルダへのショートカット自動登録処理を用意。
  - **関連コード**: [setup_shortcuts.py](file:///c:/code/lifestyle/sleep-tracker/src_python/setup_shortcuts.py)
- [x] **タスクバー（またはデスクトップ）のショートカットからログ閲覧**
  - **実装状態**: 実装済み。デスクトップに起動ショートカットを作成。これをユーザーがタスクバーにドラッグ＆ドロップしてピン留めすることで要求を満たせます。
  - **関連コード**: [setup_shortcuts.py](file:///c:/code/lifestyle/sleep-tracker/src_python/setup_shortcuts.py), [main.py](file:///c:/code/lifestyle/sleep-tracker/src_python/main.py) (GUI本体)
- [x] **PC操作開始時に自動でGitへ変更（ログ）をpush**
  - **実装状態**: 実装済み。PC操作再開時のログ同期のタイミングで、生ログテキスト (`sleep_events.txt`) を非同期スレッドで自動コミット＆プッシュ（競合防止のリベースプル付き）します。
  - **関連コード**: [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py) (`git_push_logs()`)
- [x] **GitHub プライベートリポジトリの作成**
  - **実装状態**: 完了。`https://github.com/zappyzed100/sleep-tracker` を private で作成・同期済み。

### 睡眠時間予測ロジック
- [x] **今眠ったら何時間眠ることになるかを予測**
  - **実装状態**: 実装済み。データが少ない内は「同時刻帯の平均」を用いる統計モデル、ログが10件以上揃うと `scikit-learn` の Random Forest Regressor を用いた機械学習モデルによる予測に自動で切り替わります。
  - **関連コード**: [analyzer.py](file:///c:/code/lifestyle/sleep-tracker/src_python/analyzer.py)
- [x] **予測特徴量に入眠時刻と「連続覚醒時間」を導入**
  - **実装状態**: 実装済み。入眠時刻の周期特徴量（sin/cos変換）、曜日（One-Hot）、および「最後の睡眠セッションが終了（起床）してから現在までの経過時間（連続覚醒時間）」を特徴量に組み込んで予測します。
  - **関連コード**: [analyzer.py](file:///c:/code/lifestyle/sleep-tracker/src_python/analyzer.py) (`predict_with_ml()`)

### 開発プロセス・ルール
- [x] **いきなり実装せずに実装方法を議論**
  - **実装状態**: 完了。実装計画書を作成し合意を得た上で開発しました。
- [x] **Pythonはuvを使用、C++も併用**
  - **実装状態**: 完了。`uv add` で依存環境を構築し、軽量常駐監視をC++、GUI・分析・Git操作をPythonで実装しました。
- [x] **全ファイル500行以下**
  - **実装状態**: 厳守。すべてのソースコードが500行以下で構築されています。
- [x] **ファイルの先頭10行にファイル情報コメントを記述**
  - **実装状態**: 厳守。すべてのソースファイルにヘッダーコメントを記述。
- [x] **各フォルダにREADME.mdを設置し、ナビゲーションを提供**
  - **実装状態**: 厳守。リポジトリルート、`src_cpp/`、`src_python/` にそれぞれ `README.md` を設置しました。
- [x] **AIエージェント間の共通規則を同期する**
  - **実装状態**: 完了。ルートの `.agents/AGENTS.md` にルールを定義し、同期指示を明記。

---

## iPhone 外出検知（iOS ショートカット）のセットアップ手順

Macを持たない環境でも、iPhoneの位置情報トリガーと GitHub Gist API を連携させて外出検知を行うための設定方法です。

### 1. PC側での中継 Gist 作成
1. コマンドプロンプト等で、以下のコマンドを実行します：
   ```bash
   uv run python src_python/gist_setup.py
   ```
2. 画面に **Gist ID** と、iPhone設定用の **アクセストークン (Token)** が表示されます。この2つをiPhone側で設定します。

### 2. iPhone 側のオートメーション作成
iPhoneの「ショートカット」アプリを開き、「オートメーション」タブから以下の2つのオートメーションを新規作成します。

#### ① 外出時 (自宅から出発したとき)
1. 個人用オートメーションを新規作成 ➡ **「出発」** を選択。
2. 位置情報に「自宅」を指定し、「いつでも」チェックを入れて「次へ」を選択。
3. アクションとして **「URLの内容を取得」** を追加します。
4. 設定値を以下のように指定します：
   - **URL**: `https://api.github.com/gists/<GIST_ID>` （PCの画面に出力されたURL）
   - **方法 (Method)**: `PATCH`
   - **ヘッダ**: 2行追加
     - `Authorization`: `Bearer <TOKEN>` （PCの画面に出力された完全なToken）
     - `User-Agent`: `iOS-Shortcut`
   - **要求本文 (Body)**: `JSON`
     - **新規フィールドを追加** ➡ キーを `files`、タイプを `ディクショナリ` に指定。
     - **`files` の中に入力** ➡ キーを `mobile_event.txt`、タイプを `ディクショナリ` に指定。
     - **`mobile_event.txt` の中に入力** ➡ キーを `content`、タイプを `テキスト` に指定。
     - **`content` の値**: `LEAVE,` に続けて「現在の日付」を選択。
       - ※「現在の日付」をタップし、日付フォーマットを「カスタム」に設定し、フォーマット文字列を `yyyy-MM-dd HH:mm:ss` に指定します。
5. 「実行の前に尋ねる」をオフにして保存します。

#### ② 帰宅時 (自宅に到着したとき)
1. 新しい個人用オートメーションを作成 ➡ **「到着」** を選択。
2. 位置情報に「自宅」を指定。
3. アクションとして同様に **「URLの内容を取得」** を設定（URL、ヘッダ、要求本文の構造は「外出時」と同一）。
4. 要求本文 (Body) 内の `content` の値のみ、以下に変更します：
   - **`content` の値**: `ARRIVE,` に続けて「現在の日付」（カスタムフォーマット: `yyyy-MM-dd HH:mm:ss`）
5. 同様に「実行の前に尋ねる」をオフにして保存します。

