// FRIDAY's browser extension: when FRIDAY asks (through its native host on
// this computer, org.friday.browser), it reads the tab in front: title,
// address, selected text and the page's text. Nothing else, nothing unasked,
// and never a private window (extensions are kept out of those by default).
const api = globalThis.browser ?? globalThis.chrome;
const MAX_TEXT = 20000;
let port = null;

function readPage(max) {
  const sel = String(getSelection() ?? "");
  const text = (document.body?.innerText ?? "").slice(0, max);
  return { selection: sel.slice(0, max), text };
}

// "Where is Sign in?": the control on the page whose name the question
// holds (the longest such name), outlined for 4 s and scrolled into view.
function findOnPage(question) {
  const q = ` ${question.toLowerCase().replace(/\s+/g, " ")} `;
  const sel = "a, button, input, select, textarea, summary, label, [role=button], [role=link], [role=tab], [role=menuitem], [role=checkbox], [aria-label]";
  let best = null, bestName = "";
  for (const el of document.querySelectorAll(sel)) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 || r.height === 0 || el.type === "password") continue;
    const name = (el.getAttribute("aria-label") || el.innerText || el.value || el.title || "")
      .replace(/\s+/g, " ").trim();
    if (name.length < 2 || name.length > 60) continue;
    if (q.includes(` ${name.toLowerCase()} `) || q.includes(` ${name.toLowerCase()}?`)) {
      if (name.length > bestName.length) { best = el; bestName = name; }
    }
  }
  if (!best) return { found: false };
  best.scrollIntoView({ block: "center", behavior: "smooth" });
  const was = [best.style.outline, best.style.outlineOffset];
  best.style.outline = "3px solid #88c0d0";
  best.style.outlineOffset = "3px";
  setTimeout(() => { best.style.outline = was[0]; best.style.outlineOffset = was[1]; }, 4000);
  return { found: true, name: bestName };
}

async function findInTab(question) {
  const [tab] = await api.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab || tab.incognito) return { error: "no tab FRIDAY may read" };
  try {
    const [res] = await api.scripting.executeScript({ target: { tabId: tab.id }, func: findOnPage, args: [String(question)] });
    return res?.result ?? { found: false };
  } catch (e) {
    return { found: false, note: "this page can't be searched" };
  }
}

async function activeTab() {
  const [tab] = await api.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab || tab.incognito) return { error: "no tab FRIDAY may read" };
  const out = { title: tab.title ?? "", url: tab.url ?? "" };
  try {
    const [res] = await api.scripting.executeScript({ target: { tabId: tab.id }, func: readPage, args: [MAX_TEXT] });
    Object.assign(out, res?.result ?? {});
  } catch (e) {
    // Browser pages (settings, the store) can't be read: title and address only.
    out.note = "the page's text can't be read here";
  }
  return out;
}

function connect() {
  try {
    port = api.runtime.connectNative("org.friday.browser");
  } catch (e) {
    setTimeout(connect, 60000);
    return;
  }
  port.onMessage.addListener(async (msg) => {
    // Always an answer, so the host isn't left waiting on a failure.
    try {
      if (msg?.type === "tab") port.postMessage({ id: msg.id, ...(await activeTab()) });
      else if (msg?.type === "find") port.postMessage({ id: msg.id, ...(await findInTab(msg.question)) });
    } catch (e) {
      port?.postMessage({ id: msg?.id, error: "the browser couldn't answer" });
    }
  });
  port.onDisconnect.addListener(() => {
    port = null;
    setTimeout(connect, 10000);
  });
}

connect();
