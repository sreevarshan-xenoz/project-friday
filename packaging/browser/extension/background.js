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
    if (msg?.type !== "tab") return;
    port.postMessage({ id: msg.id, ...(await activeTab()) });
  });
  port.onDisconnect.addListener(() => {
    port = null;
    setTimeout(connect, 10000);
  });
}

connect();
