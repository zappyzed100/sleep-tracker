# Python 解析・予測・UIツール (src_python)

このフォルダには、睡眠データの処理、機械学習モデルによる睡眠時間予測、およびログを表示するためのユーザーインターフェース（GUI）が含まれています。また、C++監視サービスの代わりに動作するPythonフォールバック監視スクリプトも含まれています。

---

## 役割と目的

C++ 監視サービス（または Python 監視スクリプト）が生成した生ログ（ハートビートと状態イベント）をパースし、確定した睡眠セッションデータへと変換・蓄積します。
さらに、蓄積したデータから「今眠ったら何時間眠るか」を予測する機械学習モデルを構築し、それらを直感的なGUIでユーザーに提示します。

- **自動 Git Push 機能 [NEW]**:
  PC操作開始を検知してログ同期が行われたタイミングで、生ログファイル (`src_cpp/sleep_events.txt`) を自動的に GitHub プライベートリポジトリへ push します。
- **連続覚醒時間を考慮した予測 [NEW]**:
  「今眠ったら何時間眠るか」を予測する特徴量として、入眠仮定時刻（現在時刻）と、直前の睡眠終了時刻（最後の起床時刻）の差である「連続覚醒時間」を特徴量に追加し、より高い精度で予測を行います。

---

## フォルダ内のファイル構成

- [README.md](file:///c:/code/lifestyle/sleep-tracker/src_python/README.md): 本ドキュメント（モジュール概要と使用方法）
- [main.py](file:///c:/code/lifestyle/sleep-tracker/src_python/main.py): アプリケーションのエントリーポイント（GUIの起動、ショートカットからの呼び出し先）
- [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py): 生のテキストログをパースして SQLite へ保存するロジック、および非同期での自動 Git Push 処理。
- [analyzer.py](file:///c:/code/lifestyle/sleep-tracker/src_python/analyzer.py): 過去の睡眠履歴（入眠時刻、曜日、連続覚醒時間）から将来の睡眠時間を予測するモデル（scikit-learnを使用）
- [monitor.py](file:///c:/code/lifestyle/sleep-tracker/src_python/monitor.py): C++がビルドできない場合のWindows向けフォールバック監視スクリプト
- [setup_shortcuts.py](file:///c:/code/lifestyle/sleep-tracker/src_python/setup_shortcuts.py): 自動起動およびデスクトップショートカットのセットアップスクリプト

---

## 実行環境のセットアップと起動方法

### 1. 依存関係のインストール
```bash
uv pip install -r pyproject.toml
```

### 2. アプリケーションの起動（GUI）
```bash
uv run python src_python/main.py
```

### 3. フォールバック監視プロセスの起動
```bash
uv run pythonw src_python/monitor.py
```
