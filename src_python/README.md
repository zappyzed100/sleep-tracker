# Python 解析・予測・UIツール (src_python)

このフォルダには、睡眠データの処理、機械学習モデルによる睡眠時間予測、およびログを表示するためのユーザーインターフェース（GUI）が含まれています。また、C++監視サービスの代わりに動作するPythonフォールバック監視スクリプトも含まれています。

---

## 役割と目的

C++ 監視サービス（または Python 監視スクリプト）が生成した生ログ（ハートビートと状態イベント）をパースし、確定した睡眠セッションデータへと変換・蓄積します。
さらに、蓄積したデータから「今眠ったら何時間眠るか」を予測する機械学習モデルを構築し、それらを直感的なGUIでユーザーに提示します。

---

## フォルダ内のファイル構成

- [README.md](file:///c:/code/lifestyle/sleep-tracker/src_python/README.md): 本ドキュメント（モジュール概要と使用方法）
- [main.py](file:///c:/code/lifestyle/sleep-tracker/src_python/main.py): アプリケーションのエントリーポイント（GUIの起動、ショートカットからの呼び出し先）
- [database.py](file:///c:/code/lifestyle/sleep-tracker/src_python/database.py): 生のテキストログ（ハートビートやイベント）を解析し、確定した睡眠時間を算出してデータベース（SQLite）へ保存するロジック
- [analyzer.py](file:///c:/code/lifestyle/sleep-tracker/src_python/analyzer.py): 過去の睡眠履歴から将来の睡眠時間を予測するモデル（scikit-learnを使用）
- [monitor.py](file:///c:/code/lifestyle/sleep-tracker/src_python/monitor.py): C++がビルドできない場合のWindows向けフォールバック監視スクリプト（ctypesを使用し、低負荷でアイドル監視とハートビート保存を実行）

---

## 実行環境のセットアップと起動方法

本プロジェクトはパッケージマネージャーとして `uv` を使用します。

### 1. 依存関係のインストール
プロジェクトのルートディレクトリで以下のコマンドを実行します：
```bash
uv pip install -r pyproject.toml
```
または、必要なパッケージを個別にインストールします：
```bash
uv add pandas numpy scikit-learn matplotlib
```

### 2. アプリケーションの起動（GUI）
タスクバーのショートカットやダブルクリックによる起動のエントリーポイントです：
```bash
uv run python src_python/main.py
```

### 3. フォールバック監視プロセスの起動
C++バイナリがビルドできない場合、バックグラウンドでの監視はこちらで起動します：
```bash
uv run pythonw src_python/monitor.py
```
※ `pythonw` を使用することで、コマンドプロンプトの黒い画面を出さずにバックグラウンドで動作させることができます。
