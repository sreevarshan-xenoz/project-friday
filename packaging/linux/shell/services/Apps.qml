pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Wayland
import qs.common

// The apps the dock and the taskbar show: the pinned ones ("dockApps",
// desktop entry ids, in their order), then the running ones not pinned.
Singleton {
  id: root

  readonly property var pinned: (Config.pick("dockApps", []) || []).map(id => DesktopEntries.byId(String(id))).filter(e => e)
  readonly property var items: {
    const out = pinned.map(e => ({ entry: e, id: e.id }));
    for (const t of ToplevelManager.toplevels.values) {
      const e = DesktopEntries.heuristicLookup(t.appId);
      const id = e ? e.id : t.appId;
      if (id && !out.some(o => o.id === id))
        out.push({ entry: e, id: id, appId: t.appId });
    }
    return out;
  }

  // An item's open windows.
  function windowsOf(item) {
    const want = String(item.appId || item.id).toLowerCase();
    return ToplevelManager.toplevels.values.filter(t => {
      const a = String(t.appId).toLowerCase();
      return a === want || (item.entry && DesktopEntries.heuristicLookup(t.appId) === item.entry);
    });
  }

  // Running: its next window (the first if none is in front); else start it.
  function activate(item) {
    const wins = windowsOf(item);
    if (wins.length > 0) {
      const i = wins.findIndex(w => w.activated);
      wins[(i + 1) % wins.length].activate();
    } else if (item.entry) {
      item.entry.execute();
    }
  }

  function name(item) { return item.entry ? item.entry.name : item.id; }
  function icon(item) { return AppIcons.source(item.entry ? item.entry.icon : item.appId); }
}
