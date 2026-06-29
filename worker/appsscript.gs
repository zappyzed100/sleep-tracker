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

function getBackupFolder() {
  const ss = SpreadsheetApp.getActiveSpreadsheet();
  return DriveApp.getFileById(ss.getId()).getParents().next();
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
      Logger.log("既存ファイルを上書き: " + files.next().getId());
      files.next ? null : null; // reset iterator (already consumed)
      folder.getFilesByName(fileName).next().setContent("testBackup: " + new Date());
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

  // iPhone / Android event: URL params
  const tag = (e.parameter.tag ?? "").trim();
  const ts  = (e.parameter.ts  ?? "").trim();
  if (!tag || !ts) {
    return ContentService.createTextOutput("missing tag or ts");
  }

  SpreadsheetApp.getActiveSpreadsheet()
    .getSheetByName("events")
    .appendRow([ts, tag]);

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

  // Restore: return sleep_events_backup.txt content
  if (e.parameter.action === "restore") {
    const files = getBackupFolder().getFilesByName("sleep_events_backup.txt");
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
