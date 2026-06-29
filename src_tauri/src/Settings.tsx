import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { Session } from "./types";
import TimePicker from "./TimePicker";

function ConfirmDeleteModal({ onConfirm, onCancel }: { onConfirm: () => void; onCancel: () => void }) {
  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal-card confirm-modal" onClick={(e) => e.stopPropagation()}>
        <div className="confirm-modal-icon">⚠️</div>
        <div className="confirm-modal-title">全データを削除しますか？</div>
        <div className="confirm-modal-body">
          <p>記録されている全ての睡眠データが削除されます。</p>
          <p className="confirm-modal-warn">この操作は元に戻せません。</p>
        </div>
        <div className="confirm-modal-btns">
          <button className="settings-btn" onClick={onCancel}>キャンセル</button>
          <button className="settings-btn settings-btn-danger" onClick={onConfirm}>削除する</button>
        </div>
      </div>
    </div>
  );
}

interface AppConfig {
  idle_threshold_minutes: number | null;
  mobile_url: string | null;
  mobile_secret: string | null;
  target_wake_time: string | null;
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
  onRefresh?: () => void;
}

export default function Settings({ sessions, onRefresh }: Props) {
  const [threshold, setThreshold] = useState(60);
  const [configSaved, setConfigSaved] = useState(false);
  const [targetWakeEnabled, setTargetWakeEnabled] = useState(false);
  const [targetWake, setTargetWake] = useState("07:00");
  const [mobileUrl, setMobileUrl] = useState("");
  const [mobileSecret, setMobileSecret] = useState("");
  const [showMobileSecret, setShowMobileSecret] = useState(false);
  const [mobileTestStatus, setMobileTestStatus] = useState<{ ok: boolean; msg: string } | null>(null);
  const [mobileTesting, setMobileTesting] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [syncMsg, setSyncMsg] = useState<string | null>(null);
  const [startup, setStartup] = useState(false);
  const [csvMsg, setCsvMsg] = useState<string | null>(null);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [shortcutMsg, setShortcutMsg] = useState<string | null>(null);
  const [shortcutBusy, setShortcutBusy] = useState(false);

  useEffect(() => {
    invoke<AppConfig>("get_config").then((cfg) => {
      setThreshold(cfg.idle_threshold_minutes ?? 60);
      setMobileUrl(cfg.mobile_url ?? "");
      setMobileSecret(cfg.mobile_secret ?? "");
      if (cfg.target_wake_time) {
        setTargetWakeEnabled(true);
        setTargetWake(cfg.target_wake_time);
      }
    }).catch(console.error);

    invoke<boolean>("get_startup_enabled").then(setStartup).catch(console.error);
  }, []);

  async function handleSaveConfig() {
    try {
      await invoke("save_config", {
        idleThresholdMinutes: threshold,
        mobileUrl,
        mobileSecret,
        targetWakeTime: targetWakeEnabled ? targetWake : null,
      });
      setConfigSaved(true);
      setTimeout(() => setConfigSaved(false), 2000);
      onRefresh?.();
    } catch (e) {
      console.error(e);
    }
  }

  async function handleTestMobile() {
    setMobileTesting(true);
    setMobileTestStatus(null);
    try {
      const msg = await invoke<string>("test_mobile_connection", { mobileUrl, mobileSecret });
      setMobileTestStatus({ ok: true, msg });
    } catch (e) {
      setMobileTestStatus({ ok: false, msg: String(e) });
    } finally {
      setMobileTesting(false);
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
      onRefresh?.();
    } catch (e) {
      setSyncMsg(`エラー: ${e}`);
    } finally {
      setSyncing(false);
    }
  }

  async function execClearAll() {
    setShowDeleteConfirm(false);
    try {
      await invoke("clear_all_data");
      setCsvMsg("全データを削除しました。");
      onRefresh?.();
    } catch (e) {
      setCsvMsg(`エラー: ${e}`);
    }
  }

  async function handleCreateShortcut() {
    setShortcutBusy(true);
    setShortcutMsg(null);
    try {
      await invoke("create_desktop_shortcut");
      setShortcutMsg("デスクトップにショートカットを作成しました");
    } catch (e) {
      setShortcutMsg(`作成失敗: ${e}`);
    } finally {
      setShortcutBusy(false);
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

  async function handleBackup() {
    setCsvMsg(null);
    try {
      const content = await invoke<string>("get_events_content");
      const defaultName = `sleep_backup_${new Date().toISOString().slice(0, 10)}.txt`;
      const path = await save({
        filters: [{ name: "テキスト", extensions: ["txt"] }],
        defaultPath: defaultName,
      });
      if (!path) return;
      await invoke("write_csv_file", { path, content });
      setCsvMsg(`バックアップを保存しました → ${path}`);
    } catch (e) {
      setCsvMsg(`エラー: ${e}`);
    }
  }

  async function handleRestore() {
    setCsvMsg(null);
    const ok = window.confirm("現在のデータをバックアップファイルで上書きします。\n本当に復元しますか？");
    if (!ok) return;
    try {
      const path = await open({
        filters: [{ name: "テキスト", extensions: ["txt"] }],
        multiple: false,
      });
      if (!path) return;
      const content = await invoke<string>("read_text_file", { path });
      await invoke("restore_events", { content });
      setCsvMsg("バックアップから復元しました。");
      onRefresh?.();
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
        <button
          className="settings-btn"
          onClick={handleCreateShortcut}
          disabled={shortcutBusy}
          style={{ alignSelf: "flex-start" }}
        >
          {shortcutBusy ? "作成中..." : "デスクトップにショートカットを作成"}
        </button>
        {shortcutMsg && (
          <div className={`settings-status ${shortcutMsg.startsWith("作成失敗") ? "err" : "ok"}`}>
            {shortcutMsg}
          </div>
        )}
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
        <button className="settings-btn primary" onClick={handleSaveConfig} style={{ alignSelf: "flex-start" }}>
          {configSaved ? "✓ 保存しました" : "保存"}
        </button>
      </Section>

      {/* 目標起床時刻 */}
      <Section title="目標起床時刻">
        <label className="settings-check-row">
          <input
            type="checkbox"
            checked={targetWakeEnabled}
            onChange={(e) => setTargetWakeEnabled(e.target.checked)}
            className="settings-checkbox"
          />
          <span>目標起床時刻を指定する</span>
        </label>
        {targetWakeEnabled ? (
          <div className="settings-row">
            <TimePicker value={targetWake} onChange={setTargetWake} />
          </div>
        ) : (
          <div className="settings-note">未指定の場合、過去の起床時刻の中央値を自動で使用します</div>
        )}
        <button className="settings-btn primary" onClick={handleSaveConfig} style={{ alignSelf: "flex-start" }}>
          {configSaved ? "✓ 保存しました" : "保存"}
        </button>
      </Section>

      {/* クラウド連携 */}
      <Section title="クラウド連携">
        <div className="settings-field">
          <label className="settings-label">Apps Script URL</label>
          <input
            className="settings-input"
            value={mobileUrl}
            onChange={(e) => setMobileUrl(e.target.value)}
            placeholder="https://script.google.com/macros/s/SCRIPT_ID/exec"
          />
        </div>
        <div className="settings-field">
          <label className="settings-label">シークレット</label>
          <div className="settings-input-wrap">
            <input
              className="settings-input"
              type={showMobileSecret ? "text" : "password"}
              value={mobileSecret}
              onChange={(e) => setMobileSecret(e.target.value)}
              placeholder="スクリプトプロパティに設定した SECRET の値"
            />
            <button
              className="settings-eye-btn"
              onClick={() => setShowMobileSecret((v) => !v)}
              title={showMobileSecret ? "隠す" : "表示"}
            >
              {showMobileSecret ? "🙈" : "👁"}
            </button>
          </div>
        </div>
        <div className="settings-btn-row">
          <button className="settings-btn primary" onClick={handleSaveConfig}>
            {configSaved ? "✓ 保存しました" : "保存"}
          </button>
          <button className="settings-btn" onClick={handleTestMobile} disabled={mobileTesting}>
            {mobileTesting ? "テスト中..." : "接続テスト"}
          </button>
          <button className="settings-btn" onClick={handleSyncGist} disabled={syncing}>
            {syncing ? "同期中..." : "今すぐ同期"}
          </button>
        </div>
        {mobileTestStatus && (
          <div className={`settings-status ${mobileTestStatus.ok ? "ok" : "err"}`}>
            {mobileTestStatus.ok ? "✓" : "✗"} {mobileTestStatus.msg}
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
        </div>
        <div className="settings-note">Excel等で分析用。就寝・起床・睡眠時間・種別の4列。</div>
        <div className="settings-btn-row" style={{ marginTop: 8 }}>
          <button className="settings-btn" onClick={handleBackup}>
            バックアップ
          </button>
          <button className="settings-btn" onClick={handleRestore}>
            バックアップから復元
          </button>
        </div>
        <div className="settings-note">生データをそのまま保存・復元。別PCへの移行や完全バックアップに。</div>
        <div className="settings-btn-row" style={{ marginTop: 8 }}>
          <button className="settings-btn settings-btn-danger" onClick={() => setShowDeleteConfirm(true)}>
            全データを削除
          </button>
        </div>
        {csvMsg && <div className="settings-csv-msg">{csvMsg}</div>}
      </Section>

      {showDeleteConfirm && (
        <ConfirmDeleteModal
          onConfirm={execClearAll}
          onCancel={() => setShowDeleteConfirm(false)}
        />
      )}
    </div>
  );
}
