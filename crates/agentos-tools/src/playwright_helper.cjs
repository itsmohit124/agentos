"use strict";

const readline = require("readline");

let playwright;
try {
  playwright = require("playwright");
} catch (err) {
  process.stderr.write(`playwright unavailable: ${err.message}\n`);
  process.exit(2);
}

let browser = null;
let page = null;
const consoleLines = [];
const networkLines = [];

async function ensurePage() {
  if (page) {
    return page;
  }
  browser = await playwright.chromium.launch({ headless: true });
  page = await browser.newPage();
  page.on("console", (message) => {
    consoleLines.push(message.text());
  });
  page.on("request", (request) => {
    networkLines.push({ method: request.method(), url: request.url() });
  });
  return page;
}

async function handle(command) {
  const action = command.action;
  if (action === "close") {
    if (browser) {
      await browser.close();
      browser = null;
      page = null;
    }
    return { action, status: "completed", detail: "closed", inspection: null, artifact: null };
  }
  if (action === "open") {
    const current = await ensurePage();
    await current.goto(command.url, { waitUntil: "load" });
    return {
      action,
      status: "completed",
      detail: `opened ${command.url}`,
      inspection: null,
      artifact: null,
    };
  }
  if (!page) {
    return { action, status: "failed", detail: "no page is open", inspection: null, artifact: null };
  }
  if (action === "click") {
    await page.click(command.selector);
    return { action, status: "completed", detail: `clicked ${command.selector}`, inspection: null, artifact: null };
  }
  if (action === "fill") {
    await page.fill(command.selector, command.value);
    return { action, status: "completed", detail: `filled ${command.selector}`, inspection: null, artifact: null };
  }
  if (action === "screenshot") {
    await page.screenshot({ path: command.path });
    return { action, status: "completed", detail: "wrote screenshot", inspection: null, artifact: command.path };
  }
  if (action === "inspect") {
    const dom = await page.locator("body *").evaluateAll((elements) => {
      return elements
        .map((element) => {
          const id = element.id ? `#${element.id}` : "";
          const text = Array.from(element.childNodes)
            .filter((node) => node.nodeType === 3)
            .map((node) => node.textContent.trim())
            .filter(Boolean)
            .join(" ");
          const label = `${element.tagName.toLowerCase()}${id}`;
          return text ? `${label}=${text}` : label;
        })
        .sort();
    });
    const network = networkLines
      .slice()
      .sort((left, right) => `${left.method} ${left.url}`.localeCompare(`${right.method} ${right.url}`));
    return {
      action,
      status: "completed",
      detail: "inspected",
      inspection: {
        dom,
        console: consoleLines.slice().sort(),
        network,
      },
      artifact: null,
    };
  }
  return { action, status: "failed", detail: `unknown action ${action}`, inspection: null, artifact: null };
}

async function main() {
  const lines = readline.createInterface({ input: process.stdin });
  for await (const line of lines) {
    if (!line.trim()) {
      continue;
    }
    let result;
    try {
      result = await handle(JSON.parse(line));
    } catch (err) {
      const parsed = JSON.parse(line);
      result = {
        action: parsed.action || "unknown",
        status: "failed",
        detail: String(err && err.message ? err.message : err),
        inspection: null,
        artifact: null,
      };
    }
    process.stdout.write(`${JSON.stringify(result)}\n`);
    if (result.action === "close") {
      break;
    }
  }
  if (browser) {
    await browser.close();
  }
}

main().catch((err) => {
  process.stderr.write(`${err.stack || err.message || err}\n`);
  process.exit(1);
});
