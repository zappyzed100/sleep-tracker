// Sleep Tracker — Google Apps Script Web App (hardened)
//
// Setup:
//   1. New Google Sheet with header row: [Timestamp, Tag] in row 1
//   2. Open Extensions → Apps Script, paste this code
//   3. Project Settings → Script Properties → Add: SECRET = <your secret>
//   4. Deploy → New Deployment → Web App
//      Execute as: Me / Who has access: Anyone
//   5. Copy the deployment URL into the sleep tracker app settings
//
// ── 防御策 ─────────────────────────────────────────────────────────
//  A. 内容検証: バックアップ内容がイベント形式(YYYY-MM-DD HH:MM:SS,TAG)かを
//     検証し、HTML/JS(Googleログインページ等)なら拒否する
//     （sleep_events.txtの実際の行順は「タイムスタンプが先」なので注意）
//  B. 縮小ガード: 既存より大幅に小さい内容での上書きを拒否
//     （「データを圧縮」「全データ削除」等の意図的な縮小はforce=1を付けて送信）
//  C. 世代バックアップ: 上書き前に既存内容を backup_history/ に退避し、
//     直近 GENERATIONS_TO_KEEP 世代を保持
//  D. LockService: トリガー/リクエストの多重実行による競合書き込みを防止
//  E. clear_all に confirm=yes を必須化 + 消去前に退避
//  F. 世代番号(GENERATION): 「クラウドも含めて全データ削除」「データを圧縮」が
//     もう一方の端末で実行されたかを判定するための、LockServiceで排他的に
//     払い出されるカウンタ（アプリ側 core/cloud.rs 参照）
//  G. 内容ハッシュ(expected_hash): pull〜push間の割り込み書き込みによる
//     ロスト・アップデートを防ぐ楽観的並行性制御。force=1のときはスキップ
// ──────────────────────────────────────────────────────────────────

const SECRET = PropertiesService.getScriptProperties().getProperty("SECRET");
const GENERATION_PROP = "GENERATION";
const HISTORY_FOLDER_ID_PROP = "HISTORY_FOLDER_ID";

const BACKUP_FILE         = "sleep_events_backup.txt";
const MANUAL_BACKUP_FILE  = "sleep_manual_backup.txt";
const SETTINGS_FILE       = "sync_settings.json";
const HISTORY_FOLDER      = "backup_history";
const GENERATIONS_TO_KEEP = 10;   // 世代バックアップの保持数
const SHRINK_RATIO_LIMIT  = 0.5;  // 既存の50%未満に縮む上書きは拒否（force=1で回避可）

// ── G. 内容ハッシュ（楽観的並行性制御） ──────────────────────────────
//
// 「pullしてからpushするまでの間に、別端末（またはこのendpointへの直接操作）が
// 割り込んで書き込むと、その内容がマージされずに上書きされて消える」という
// 事故が発生したため追加した。世代番号（F）は全削除・圧縮のような一括リセット
// しか検知できず、通常のイベント追記の競合は検知できないため、別の仕組みが必要。
//
// push時にクライアントが「pull時点で見ていた内容のSHA-256」を expected_hash
// として一緒に送り、GASは「今実際に保存されている内容のハッシュ」と比較する。
// 一致すれば書き込みを許可（＝pullしてから誰も書き込んでいない証拠）。
// 不一致ならreject（＝pullとpushの間に誰かが書き込んだ）し、クライアントは
// pullからやり直す。ハッシュは保存せず、比較のたびに現在の内容から都度計算する
// （別途保持すると、更新し忘れるバグ＝Fの世代番号で起きたのと同じ種類のバグの
// 温床になるため）。
// force=1（圧縮・全削除後の強制上書き）の時はこのチェックをスキップする
// （force自体が「ガードを承知の上で上書きする」という明示的な意思表示のため）。
function computeHash_(content) {
  const raw = Utilities.computeDigest(Utilities.DigestAlgorithm.SHA_256, content, Utilities.Charset.UTF_8);
  return raw.map(b => (b < 0 ? b + 256 : b).toString(16).padStart(2, "0")).join("");
}

// ── F. 世代番号 ────────────────────────────────────────────────────

// クラウドの「世代番号」を排他制御しながら1つ進めて返す。
// 「クラウドも含めて全データ削除」「データを圧縮」等、ローカルの通常マージでは
// 対応しきれない全体リセット系の操作でのみ呼ぶ。LockServiceにより、2台の端末が
// ほぼ同時に呼んでも同じ番号が重複して払い出されることはない
// （番号を各端末が自己申告すると、オフライン時に両方が独立に同じ番号を
// 採番して衝突する問題があったため、ここで一元管理する）。
function advanceGeneration() {
  const lock = LockService.getScriptLock();
  lock.waitLock(10000);
  try {
    const props = PropertiesService.getScriptProperties();
    const next = (parseInt(props.getProperty(GENERATION_PROP) || "0", 10)) + 1;
    props.setProperty(GENERATION_PROP, String(next));
    return next;
  } finally {
    lock.releaseLock();
  }
}

function getGeneration() {
  const props = PropertiesService.getScriptProperties();
  return parseInt(props.getProperty(GENERATION_PROP) || "0", 10);
}

function getBackupFolder() {
  const ss = SpreadsheetApp.getActiveSpreadsheet();
  const parents = DriveApp.getFileById(ss.getId()).getParents();
  if (!parents.hasNext()) {
    throw new Error("スプレッドシートの親フォルダが見つかりません");
  }
  return parents.next();
}

// ── A. 内容検証 ────────────────────────────────────────────────────

// HTML / JavaScript の混入検知(Googleログインページ等の誤保存対策)
function looksLikeHtmlOrJs_(content) {
  const head = content.slice(0, 5000);
  return /<!doctype|<html|<head|<script|<meta|\(function\s*\(\)|Error\.captureStackTrace|document\.querySelector/i
    .test(head);
}

// イベントバックアップ形式の検証: 各行 "YYYY-MM-DD HH:MM:SS,TAG"
// （sleep_events.txtは「タイムスタンプが先、タグが後」の順序）。
// TAG部分にはUSAGE_APP_SEEN:pkg|labelのような : や | 、日本語ラベルも入りうるため、
// 改行と山括弧(<>、HTML混入対策)以外は許容する。空行は許容。
// 非空行の90%以上が形式に一致しなければ不正とみなす。
function looksLikeEventsContent_(content) {
  content = content.replace(/^﻿/, ""); // BOM除去（一部ツールが付与することがある）
  if (looksLikeHtmlOrJs_(content)) return false;
  const lines = content.split(/\r?\n/).map(l => l.trim()).filter(l => l !== "");
  if (lines.length === 0) return false;
  const eventRe = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2},[^\r\n<>]{1,200}$/;
  const valid = lines.filter(l => eventRe.test(l)).length;
  return valid / lines.length >= 0.9;
}

// ── C. 世代バックアップ ────────────────────────────────────────────

// backup_history フォルダを取得する。毎回名前で検索すると遅い上、同名フォルダが
// 複数できるリスクもあるため、一度見つけた（または作成した）フォルダのIDを
// Script Propertiesに保存し、以後はIDで直接アクセスする。
function getHistoryFolder_(folder) {
  const props = PropertiesService.getScriptProperties();
  const savedId = props.getProperty(HISTORY_FOLDER_ID_PROP);
  if (savedId) {
    try {
      return DriveApp.getFolderById(savedId);
    } catch (e) {
      // 保存済みIDが無効（手動削除等）だった場合は名前検索にフォールバック
    }
  }

  const it = folder.getFoldersByName(HISTORY_FOLDER);
  const found = it.hasNext() ? it.next() : folder.createFolder(HISTORY_FOLDER);
  props.setProperty(HISTORY_FOLDER_ID_PROP, found.getId());
  return found;
}

// 上書き前に既存内容を退避し、古い世代を削除する
function archiveExisting_(folder, fileName) {
  const files = folder.getFilesByName(fileName);
  if (!files.hasNext()) return;
  const existing = files.next();
  const oldContent = existing.getBlob().getDataAsString();
  if (oldContent === "") return; // 空なら退避不要

  const history = getHistoryFolder_(folder);
  const stamp = Utilities.formatDate(new Date(), Session.getScriptTimeZone(), "yyyyMMdd_HHmmss");
  const base = fileName.replace(/\.txt$/, "");
  history.createFile(base + "." + stamp + ".txt", oldContent, MimeType.PLAIN_TEXT);

  // 古い世代を刈り取る(同一ベース名のみ対象)
  const generations = [];
  const it = history.getFiles();
  while (it.hasNext()) {
    const f = it.next();
    if (f.getName().indexOf(base + ".") === 0) generations.push(f);
  }
  generations.sort((a, b) => b.getName().localeCompare(a.getName())); // 新しい順
  for (let i = GENERATIONS_TO_KEEP; i < generations.length; i++) {
    generations[i].setTrashed(true);
  }
}

// 検証 + 退避 + 書き込みをまとめた安全な保存処理
// validator: content を受けて true/false を返す関数(null なら形式検証なし)
// expectedHash: 非null かつ force=false のとき、現在保存されている内容のハッシュと
// 一致しなければ拒否する（G. 内容ハッシュ参照）。
function safeWriteBackup_(folder, fileName, content, force, validator, expectedHash) {
  if (looksLikeHtmlOrJs_(content)) {
    throw new Error("rejected: content looks like HTML/JavaScript (login page?)");
  }
  if (validator && !validator(content)) {
    throw new Error("rejected: content failed format validation");
  }

  const files = folder.getFilesByName(fileName);
  const existing = files.hasNext() ? files.next() : null;

  if (existing && !force) {
    const oldContent = existing.getBlob().getDataAsString();
    if (expectedHash) {
      const actualHash = computeHash_(oldContent);
      if (actualHash !== expectedHash) {
        throw new Error("conflict: content changed since last pull (expected " +
          expectedHash.slice(0, 8) + ", actual " + actualHash.slice(0, 8) + "). Pull and retry.");
      }
    }
    const oldLen = oldContent.length;
    if (oldLen > 0 && content.length < oldLen * SHRINK_RATIO_LIMIT) {
      throw new Error(
        "rejected: new content (" + content.length + " bytes) is much smaller than existing (" +
        oldLen + " bytes). Pass force=1 to override.");
    }
  }

  archiveExisting_(folder, fileName);

  if (existing) {
    existing.setContent(content);
    return "updated: " + existing.getId();
  }
  const f = folder.createFile(fileName, content, MimeType.PLAIN_TEXT);
  return "created: " + f.getId();
}

// エディタ上で直接「実行」して Drive アクセスを検証する
function testBackup() {
  try {
    const ss = SpreadsheetApp.getActiveSpreadsheet();
    Logger.log("スプレッドシート名: " + ss.getName());
    const folder = getBackupFolder();
    Logger.log("フォルダ名: " + folder.getName() + " / ID: " + folder.getId());
    // 本番ファイルは触らず、テスト専用ファイルに書く(誤上書き防止)
    const result = safeWriteBackup_(folder, "sleep_backup_test.txt",
      "2026-01-01 00:00:00,TEST", true, null, null);
    Logger.log(result);
    Logger.log("完了");
  } catch (err) {
    Logger.log("ERROR: " + err.message + "\n" + err.stack);
  }
}

function doPost(e) {
  if (SECRET && e.parameter.secret !== SECRET) {
    return ContentService.createTextOutput("Unauthorized");
  }

  // ── D. 多重実行防止 ──
  const lock = LockService.getScriptLock();
  if (!lock.tryLock(20 * 1000)) {
    return ContentService.createTextOutput("error: could not acquire lock");
  }

  try {
    // PC/Android backup: raw body = sleep_events.txt content
    if (e.parameter.action === "backup") {
      try {
        const content = e.postData ? e.postData.getDataAsString() : "";
        Logger.log("[backup] content length: " + content.length);
        const result = safeWriteBackup_(
          getBackupFolder(), BACKUP_FILE, content,
          e.parameter.force === "1", looksLikeEventsContent_, e.parameter.expected_hash || null);
        Logger.log("[backup] " + result);
        return ContentService.createTextOutput("ok");
      } catch (err) {
        Logger.log("[backup] ERROR: " + err.message + "\n" + err.stack);
        return ContentService.createTextOutput("error: " + err.message);
      }
    }

    // Manual sessions backup: PC/Android → Drive
    // (形式が自由なため厳密検証はせず、HTML/JS混入と縮小のみガード)
    if (e.parameter.action === "backup_manual") {
      try {
        const content = e.postData ? e.postData.getDataAsString() : "";
        const result = safeWriteBackup_(
          getBackupFolder(), MANUAL_BACKUP_FILE, content,
          e.parameter.force === "1", null, e.parameter.expected_hash || null);
        Logger.log("[backup_manual] " + result);
        return ContentService.createTextOutput("ok");
      } catch (err) {
        Logger.log("[backup_manual] ERROR: " + err.message);
        return ContentService.createTextOutput("error: " + err.message);
      }
    }

    // Sync settings (idle_threshold_minutes, target_wake_time): PC → Drive
    if (e.parameter.action === "set_settings") {
      try {
        const content = e.postData ? e.postData.getDataAsString() : "{}";
        JSON.parse(content); // パースできなければ拒否(ログインページ混入等の対策)
        const folder = getBackupFolder();
        const files = folder.getFilesByName(SETTINGS_FILE);
        if (files.hasNext()) {
          files.next().setContent(content);
        } else {
          folder.createFile(SETTINGS_FILE, content, MimeType.PLAIN_TEXT);
        }
        return ContentService.createTextOutput("ok");
      } catch (err) {
        return ContentService.createTextOutput("error: " + err.message);
      }
    }

    // 全データ削除（クラウド側）: confirm=yes 必須。消去前に世代退避し、
    // 世代番号を進めてから消去する。ローカルファイルの削除はアプリ側で別途行う。
    if (e.parameter.action === "clear_all") {
      try {
        if (e.parameter.confirm !== "yes") {
          return ContentService.createTextOutput("error: clear_all requires confirm=yes");
        }
        // 世代番号を先に確定させてから削除する（削除内容のpushが後で失敗しても、
        // 世代だけは進んだ状態になるため、他端末は次回同期時に必ず「自分は
        // 遅れている」と気づける）。
        const newGen = advanceGeneration();
        const folder = getBackupFolder();
        for (const fileName of [BACKUP_FILE, MANUAL_BACKUP_FILE]) {
          const files = folder.getFilesByName(fileName);
          if (files.hasNext()) {
            archiveExisting_(folder, fileName); // 消す前に退避
            files.next().setContent("");
          }
        }
        const sheet = SpreadsheetApp.getActiveSpreadsheet().getSheetByName("events");
        const lastRow = sheet.getLastRow();
        if (lastRow > 1) {
          sheet.getRange(2, 1, lastRow - 1, sheet.getLastColumn()).clearContent();
        }
        return ContentService.createTextOutput(String(newGen));
      } catch (err) {
        Logger.log("[clear_all] ERROR: " + err.message);
        return ContentService.createTextOutput("error: " + err.message);
      }
    }

    // iPhone / Android event: URL params
    const tag = (e.parameter.tag ?? "").trim();
    const ts  = (e.parameter.ts  ?? "").trim();
    if (!tag || !ts) {
      return ContentService.createTextOutput("missing tag or ts");
    }

    // tsはAndroid（Unix msエポック、例: "1783068576551"）とiPhoneショートカット
    // （"yyyy-MM-dd HH:mm:ss"形式の整形済み文字列）の2種類の形式がありうる。
    // parseInt(ts, 10)は非数値文字で止まるため、"2026-07-03 18:55:00"のような
    // 文字列に対しても先頭の"2026"だけを数値として誤って解釈してしまう
    // （結果、1970-01-01付近の日時になるバグがあった）。数字のみで構成される
    // 場合だけエポックとして扱い、それ以外は整形済み文字列としてそのまま使う。
    const tz = Session.getScriptTimeZone();
    const tsStr = /^\d+$/.test(ts)
      ? Utilities.formatDate(new Date(parseInt(ts, 10)), tz, "yyyy-MM-dd HH:mm:ss")
      : ts;

    SpreadsheetApp.getActiveSpreadsheet()
      .getSheetByName("events")
      .appendRow([tsStr, tag]);

    return ContentService.createTextOutput("ok");
  } finally {
    lock.releaseLock();
  }
}

function doGet(e) {
  if (SECRET && e.parameter.secret !== SECRET) {
    return ContentService.createTextOutput("Unauthorized");
  }

  // Health check (used by the desktop app's connection test)
  if (e.parameter.action === "health") {
    return ContentService.createTextOutput("ok");
  }

  // 世代番号取得: 「クラウドも含めて全データ削除」「データを圧縮」が
  // もう一方の端末で実行されたかどうかを判定するための、唯一の権威ある
  // カウンタ（advanceGeneration経由でのみ進む）。
  if (e.parameter.action === "get_generation") {
    return ContentService.createTextOutput(String(getGeneration())).setMimeType(ContentService.MimeType.TEXT);
  }

  // Sync settings: Android reads PC-pushed settings
  if (e.parameter.action === "get_settings") {
    const files = getBackupFolder().getFilesByName(SETTINGS_FILE);
    if (!files.hasNext()) return ContentService.createTextOutput("not found").setMimeType(ContentService.MimeType.TEXT);
    return ContentService.createTextOutput(files.next().getBlob().getDataAsString()).setMimeType(ContentService.MimeType.JSON);
  }

  // Restore: return sleep_events_backup.txt content
  // 保存されている内容が壊れている(HTML等)場合はエラーを返し、
  // クライアントがゴミデータでローカルを上書きしないようにする
  if (e.parameter.action === "restore") {
    const files = getBackupFolder().getFilesByName(BACKUP_FILE);
    const content = files.hasNext() ? files.next().getBlob().getDataAsString() : "";
    if (content !== "" && !looksLikeEventsContent_(content)) {
      return ContentService.createTextOutput("error: stored backup is corrupted")
        .setMimeType(ContentService.MimeType.TEXT);
    }
    return ContentService.createTextOutput(content).setMimeType(ContentService.MimeType.TEXT);
  }

  // Restore manual sessions
  // (形式は自由なため、HTML/JS混入チェックのみ行う)
  if (e.parameter.action === "restore_manual") {
    const files = getBackupFolder().getFilesByName(MANUAL_BACKUP_FILE);
    const content = files.hasNext() ? files.next().getBlob().getDataAsString() : "";
    if (looksLikeHtmlOrJs_(content)) {
      return ContentService.createTextOutput("error: stored backup is corrupted")
        .setMimeType(ContentService.MimeType.TEXT);
    }
    return ContentService.createTextOutput(content).setMimeType(ContentService.MimeType.TEXT);
  }

  // Return all rows as "TAG,TIMESTAMP" lines (PC skips duplicates internally)
  const sheet = SpreadsheetApp.getActiveSpreadsheet().getSheetByName("events");
  const rows  = sheet.getDataRange().getValues();

  const tz = Session.getScriptTimeZone();
  const lines = rows
    .slice(1)                              // skip header
    .filter(r => r[0] && r[1])            // skip empty rows
    .map(r => {
      const ts = Utilities.formatDate(new Date(r[0]), tz, "yyyy-MM-dd HH:mm:ss");
      return `${r[1]},${ts}`;             // TAG,YYYY-MM-DD HH:MM:SS
    });

  return ContentService
    .createTextOutput(lines.join("\n"))
    .setMimeType(ContentService.MimeType.TEXT);
}
