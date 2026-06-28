import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { Session } from "./types";

interface AppConfig {
  gist_id: string | null;
  github_token: string | null;
  idle_threshold_minutes: number | null;
}

interface SectionProps {
  title: string;
  children: React.ReactNode;
}

function Section({ title, children }: SectionProps) {
  return (
    <div className="settings-section">
      <div className="settings-section-title">{title}</div>
      {children}
    </div>
  );
}

interface Props {
  sessions: Session[];
}

export default function Settings({ sessions }: Props) {
  // Config state
  const [gistId, setGistId] = useState("");
  const [token, setToken] = useState("");
  const [showToken, setShowToken] = useState(false);
  const [threshold, setThreshold] = useState(60);
  const [configSaved, setConfigSaved] = useState(false);

  // Startup
  const [startup, setStartup] = useState(false);

  // GitHub test
  const [testStatus, setTestStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [testing, setTesting] = useState(false);

  // CSV / data
  const [csvMsg, setCsvMsg] = useState<string | null>(null);

  // Gist sync
  const [syncing, setSyncing] = useState(false);
  const [syncMsg, setSyncMsg] = useState<string | null>(null);

  useEffect(() => {
    invoke<AppConfig>("get_config").then((cfg) => {
      setGistId(cfg.gist_id ?? "");
      setToken(cfg.github_token ?? "");
      setThreshold(cfg.idle_threshold_minutes ?? 60);
    }).catch(console.error);

    invoke<boolean>("get_startup_enabled").then(setStartup).catch(console.error);
  }, []);

  async function handleSaveConfig() {
    try {
      await invoke("save_config", {
        gistId,
        githubToken: token,
        idleThresholdMinutes: threshold,
      });
      setConfigSaved(true);
      setTimeout(() => setConfigSaved(false), 2000);
    } catch (e) {
      console.error(e);
    }
  }

  async function handleTestConnection() {
    setTesting(true);
    setTestStatus(null);
    try {
      const msg = await invoke<string>("test_github_connection", {
        gistId,
        githubToken: token,
      });
      setTestStatus({ ok: true, msg });
    } catch (e) {
      setTestStatus({ ok: false, msg: String(e) });
    } finally {
      setTesting(false);
    }
  }

  async function handleStartupToggle() {
    const next = !startup;
    try {
      await invoke("set_startup", { enable: next });
      setStartup(next);
    } catch (e) {
      console.error(e);
    }
  }

  async function handleSyncGist() {
    setSyncing(true);
    setSyncMsg(null);
    try {
      const msg = await invoke<string>("sync_gist");
      setSyncMsg(msg);
    } catch (e) {
      setSyncMsg(`エラー: ${e}`);
    } finally {
      setSyncing(false);
    }
  }

  async function handleClearAll() {
    const ok = window.confirm(
      "全ての睡眠データを削除します。この操作は元に戻せません。\n本当に削除しますか？"
    );
    if (!ok) return;
    try {
      await invoke("clear_all_data");
      setCsvMsg("全データを削除しました。アプリを再起動すると反映されます。");
    } catch (e) {
      setCsvMsg(`エラー: ${e}`);
    }
  }

  async function handleCreateShortcut() {
    try {
      await invoke("create_desktop_shortcut");
    } catch (e) {
      setCsvMsg(`ショートカット: ${e}`);
    }
  }

  async function handleExportCsv() {
    setCsvMsg(null);
    try {
      const csv = await invoke<string>("export_csv", { sessions });
      const defaultName = `sleep_data_${new Date().toISOString().slice(0, 10)}.csv`;
      const path = await save({
        filters: [{ name: "CSV", extensions: ["csv"] }],
        defaultPath: defaultName,
      });
      if (!path) return;
      await invoke("write_csv_file", { path, content: csv });
      setCsvMsg(`${sessions.length} 件をエクスポートしました → ${path}`);
    } catch (e) {
      setCsvMsg(`エラー: ${e}`);
    }
  }

  async function handleImportCsv() {
    setCsvMsg(null);
    try {
      const path = await open({
        filters: [{ name: "CSV", extensions: ["csv"] }],
        multiple: false,
      });
      if (!path) return;
      const text = await invoke<string>("read_text_file", { path });
      const count = await invoke<number>("import_csv", { csv: text });
      setCsvMsg(`${count} 件をインポートしました（アプリを再起動すると反映されます）`);
    } catch (e) {
      setCsvMsg(`エラー: ${e}`);
    }
  }

  return (
    <div className="settings-page">

      {/* 起動設定 */}
      <Section title="起動設定">
        <label className="settings-check-row">
          <input
            type="checkbox"
            checked={startup}
            onChange={handleStartupToggle}
            className="settings-checkbox"
          />
          <span>PC 起動時に自動起動する</span>
        </label>
        <button className="settings-btn" onClick={handleCreateShortcut} style={{ alignSelf: "flex-start" }}>
          デスクトップにショートカットを作成
        </button>
      </Section>

      {/* 睡眠判定時間 */}
      <Section title="睡眠判定時間">
        <div className="settings-row">
          <span>キーボード / マウス操作がない状態が</span>
          <input
            type="number"
            className="settings-number"
            value={threshold}
            min={1}
            max={9999}
            onChange={(e) => setThreshold(Number(e.target.value))}
          />
          <span>分以上続いたら睡眠と判定</span>
        </div>
        <div className="settings-note">変更後はタスクトレイのアイコンを右クリック →「終了」して再起動すると反映されます</div>
      </Section>

      {/* GitHub 連携 */}
      <Section title="GitHub 連携">
        <div className="settings-field">
          <label className="settings-label">Gist ID</label>
          <input
            className="settings-input"
            value={gistId}
            onChange={(e) => setGistId(e.target.value)}
            placeholder="bfdc8b9bd96f083d85c6f04380e38b4a"
          />
        </div>
        <div className="settings-field">
          <label className="settings-label">Personal Access Token</label>
          <div className="settings-input-wrap">
            <input
              className="settings-input"
              type={showToken ? "text" : "password"}
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="ghp_..."
            />
            <button
              className="settings-eye-btn"
              onClick={() => setShowToken((v) => !v)}
              title={showToken ? "隠す" : "表示"}
            >
              {showToken ? "🙈" : "👁"}
            </button>
          </div>
          <div className="settings-note">Gist の read/write 権限のみ付与されたトークン</div>
        </div>

        <div className="settings-btn-row">
          <button className="settings-btn primary" onClick={handleSaveConfig}>
            {configSaved ? "✓ 保存しました" : "保存"}
          </button>
          <button
            className="settings-btn"
            onClick={handleTestConnection}
            disabled={testing}
          >
            {testing ? "テスト中..." : "接続テスト"}
          </button>
          <button
            className="settings-btn"
            onClick={handleSyncGist}
            disabled={syncing}
          >
            {syncing ? "同期中..." : "今すぐ同期"}
          </button>
        </div>

        {testStatus && (
          <div className={`settings-status ${testStatus.ok ? "ok" : "err"}`}>
            {testStatus.ok ? "✓" : "✗"} {testStatus.msg}
          </div>
        )}
        {syncMsg && (
          <div className={`settings-status ${syncMsg.startsWith("エラー") ? "err" : "ok"}`}>
            {syncMsg}
          </div>
        )}

        <div className="settings-note">設定は config.json にローカル保存されます</div>
      </Section>

      {/* データ管理 */}
      <Section title="データ管理">
        <div className="settings-btn-row">
          <button className="settings-btn" onClick={handleExportCsv}>
            CSV エクスポート
          </button>
          <button className="settings-btn" onClick={handleImportCsv}>
            CSV インポート
          </button>
        </div>
        <div className="settings-btn-row">
          <button className="settings-btn settings-btn-danger" onClick={handleClearAll}>
            全データを削除
          </button>
        </div>
        {csvMsg && <div className="settings-csv-msg">{csvMsg}</div>}
      </Section>

    </div>
  );
}
