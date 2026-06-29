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

function doPost(e) {
  if (SECRET && e.parameter.secret !== SECRET) {
    return ContentService.createTextOutput("Unauthorized");
  }

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

  // Return all rows as "TAG,TIMESTAMP" lines (PC skips duplicates internally)
  const sheet = SpreadsheetApp.getActiveSpreadsheet().getSheetByName("events");
  const rows  = sheet.getDataRange().getValues();

  const lines = rows
    .slice(1)                              // skip header
    .filter(r => r[0] && r[1])            // skip empty rows
    .map(r => `${r[1]},${r[0]}`);         // TAG,TIMESTAMP

  return ContentService
    .createTextOutput(lines.join("\n"))
    .setMimeType(ContentService.MimeType.TEXT);
}
