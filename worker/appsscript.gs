// Sleep Tracker — Google Apps Script Web App
//
// Setup:
//   1. New Google Sheet with header row: [Timestamp, Tag] in row 1
//   2. Open Extensions → Apps Script, paste this code
//   3. Project Settings → Script Properties → Add: SECRET = <your secret>
//   4. Deploy → New Deployment → Web App
//      Execute as: Me / Who has access: Anyone
//   5. Copy the deployment URL into the sleep tracker app settings

const SECRET = PropertiesService.getScriptProperties().getProperty("SECRET");
const GENERATION_PROP = "GENERATION";

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

// エディタ上で直接「実行」して Drive アクセスを検証する
function testBackup() {
  try {
    const ss = SpreadsheetApp.getActiveSpreadsheet();
    Logger.log("スプレッドシート名: " + ss.getName());

    const parents = DriveApp.getFileById(ss.getId()).getParents();
    if (!parents.hasNext()) { Logger.log("ERROR: 親フォルダなし"); return; }

    const folder = parents.next();
    Logger.log("フォルダ名: " + folder.getName() + " / ID: " + folder.getId());

    const fileName = "sleep_events_backup.txt";
    const files = folder.getFilesByName(fileName);
    if (files.hasNext()) {
      const existingFile = files.next();
      Logger.log("既存ファイルを上書き: " + existingFile.getId());
      existingFile.setContent("testBackup: " + new Date());
    } else {
      const f = folder.createFile(fileName, "testBackup: " + new Date(), MimeType.PLAIN_TEXT);
      Logger.log("新規作成: " + f.getId());
    }
    Logger.log("完了");
  } catch (err) {
    Logger.log("ERROR: " + err.message + "\n" + err.stack);
  }
}

function doPost(e) {
  if (SECRET && e.parameter.secret !== SECRET) {
    return ContentService.createTextOutput("Unauthorized");
  }

  // PC backup: raw body = sleep_events.txt content
  if (e.parameter.action === "backup") {
    try {
      const content = e.postData ? e.postData.getDataAsString() : "";
      Logger.log("[backup] postData type: " + (e.postData ? e.postData.type : "null"));
      Logger.log("[backup] content length: " + content.length);

      const ss = SpreadsheetApp.getActiveSpreadsheet();
      Logger.log("[backup] spreadsheet id: " + ss.getId());

      const parents = DriveApp.getFileById(ss.getId()).getParents();
      if (!parents.hasNext()) throw new Error("スプレッドシートの親フォルダが見つかりません");
      const folder = parents.next();
      Logger.log("[backup] folder: " + folder.getName() + " (" + folder.getId() + ")");

      const fileName = "sleep_events_backup.txt";
      const files = folder.getFilesByName(fileName);
      if (files.hasNext()) {
        const f = files.next();
        f.setContent(content);
        Logger.log("[backup] updated existing file: " + f.getId());
      } else {
        const f = folder.createFile(fileName, content, MimeType.PLAIN_TEXT);
        Logger.log("[backup] created new file: " + f.getId());
      }
      return ContentService.createTextOutput("ok");
    } catch (err) {
      Logger.log("[backup] ERROR: " + err.message + "\n" + err.stack);
      return ContentService.createTextOutput("error: " + err.message);
    }
  }

  // Manual sessions backup: PC/Android → Drive
  if (e.parameter.action === "backup_manual") {
    try {
      const content = e.postData ? e.postData.getDataAsString() : "";
      const folder = getBackupFolder();
      const fileName = "sleep_manual_backup.txt";
      const files = folder.getFilesByName(fileName);
      if (files.hasNext()) {
        files.next().setContent(content);
      } else {
        folder.createFile(fileName, content, MimeType.PLAIN_TEXT);
      }
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
      const folder = getBackupFolder();
      const fileName = "sync_settings.json";
      const files = folder.getFilesByName(fileName);
      if (files.hasNext()) {
        files.next().setContent(content);
      } else {
        folder.createFile(fileName, content, MimeType.PLAIN_TEXT);
      }
      return ContentService.createTextOutput("ok");
    } catch (err) {
      return ContentService.createTextOutput("error: " + err.message);
    }
  }

  // 全データ削除（クラウド側）: Driveのバックアップファイルとeventsシートの行を全消去する。
  // ローカルファイルの削除はアプリ側（PC）で別途行う。
  if (e.parameter.action === "clear_all") {
    try {
      // 世代番号を先に確定させてから削除する（削除内容のpushが後で失敗しても、
      // 世代だけは進んだ状態になるため、他端末は次回同期時に必ず「自分は
      // 遅れている」と気づける）。
      const newGen = advanceGeneration();
      const folder = getBackupFolder();
      for (const fileName of ["sleep_events_backup.txt", "sleep_manual_backup.txt"]) {
        const files = folder.getFilesByName(fileName);
        if (files.hasNext()) {
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
    const files = getBackupFolder().getFilesByName("sync_settings.json");
    if (!files.hasNext()) return ContentService.createTextOutput("not found").setMimeType(ContentService.MimeType.TEXT);
    return ContentService.createTextOutput(files.next().getBlob().getDataAsString()).setMimeType(ContentService.MimeType.JSON);
  }

  // Restore: return sleep_events_backup.txt content
  if (e.parameter.action === "restore") {
    const files = getBackupFolder().getFilesByName("sleep_events_backup.txt");
    const content = files.hasNext() ? files.next().getBlob().getDataAsString() : "";
    return ContentService.createTextOutput(content).setMimeType(ContentService.MimeType.TEXT);
  }

  // Restore manual sessions
  if (e.parameter.action === "restore_manual") {
    const files = getBackupFolder().getFilesByName("sleep_manual_backup.txt");
    const content = files.hasNext() ? files.next().getBlob().getDataAsString() : "";
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