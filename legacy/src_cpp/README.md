# C++ 監視サービス (src_cpp)

このフォルダには、Windowsのバックグラウンドで常駐し、PCの操作状態と電源イベントを監視するC++プログラムが含まれています。

---

## 役割と目的

ユーザーがPCを操作していない時間（アイドル時間）およびPCがオフ（シャットダウン・スリープ）になっている時間を監視し、睡眠検知の元データとなる「ログ」を出力します。

- **ハートビート機能**: 1分ごとに `sleep_heartbeat.txt` に現在時刻を上書き記録します。これにより、突然の電源オフやイベント通知の取りこぼしが発生した場合でも、次回起動時に「最後にPCがアクティブだった時刻」を判定できます。
- **イベント記録機能**: 起動、終了、スリープ移行、スリープ復帰などのイベントを `sleep_events.txt` に追記記録します。
- **低オーバーヘッド**: Windows APIを直接呼び出すネイティブバイナリとして動作し、CPU・メモリ消費を極限まで抑えます。

---

## フォルダ内のファイル構成

- [README.md](file:///c:/code/lifestyle/sleep-tracker/src_cpp/README.md): 本ドキュメント（モジュール概要とビルド方法）
- [main.cpp](file:///c:/code/lifestyle/sleep-tracker/src_cpp/main.cpp): 監視プロセスのメインロジック（メッセージループ、電源イベント処理、アイドル監視）
- [build.bat](file:///c:/code/lifestyle/sleep-tracker/src_cpp/build.bat): Windows環境でビルドを行うためのバッチスクリプト

---

## ビルド方法

### 1. MSVC (Microsoft Visual C++) でのビルド
開発者コマンドプロンプト（Developer Command Prompt for VS）を開き、以下のコマンドを実行します：
```cmd
cl.exe /EHsc /O2 main.cpp /link user32.lib advapi32.lib /OUT:sleep_monitor.exe
```

### 2. MinGW (g++) でのビルド
コマンドプロンプトやPowerShellで以下のコマンドを実行します：
```bash
g++ -O2 main.cpp -o sleep_monitor.exe -luser32 -ladvapi32 -mwindows
```
※ `-mwindows` フラグを付与することで、起動時に黒いコンソールウィンドウが表示されないバックグラウンドプロセスとしてビルドできます。
