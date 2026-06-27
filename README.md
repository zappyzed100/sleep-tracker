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
