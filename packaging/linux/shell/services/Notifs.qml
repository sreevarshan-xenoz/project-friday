pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications

// The desktop notification server (org.freedesktop.Notifications). FRIDAY's
// answers arrive this way (`notify-send -a FRIDAY`, with actions), so they get
// their own look and a longer stay on screen.
//
// `popups` is what is on screen now; `list` is the history kept until
// dismissed. Do-not-disturb holds back popups except critical ones.
//
// IPC target `notifications`: dismissOne, dismissAll, invokeLast, toggleDnd, setDnd <true|false>.
Singleton {
  id: root

  property bool dnd: false
  property var popups: []
  readonly property var list: server.trackedNotifications.values

  function isFriday(n) { return n && n.appName === "FRIDAY"; }

  function hidePopup(n) { root.popups = root.popups.filter(p => p !== n); }
  function dismiss(n) {
    hidePopup(n);
    if (n)
      n.dismiss();
  }
  function clearAll() {
    for (const n of server.trackedNotifications.values.slice())
      n.dismiss();
    root.popups = [];
  }

  function newest() {
    const all = server.trackedNotifications.values;
    return all.length > 0 ? all[all.length - 1] : null;
  }

  IpcHandler {
    target: "notifications"
    function dismissOne(): void { const n = root.newest(); if (n) root.dismiss(n); }
    function dismissAll(): void { root.clearAll(); }
    function invokeLast(): void {
      const n = root.newest();
      if (!n)
        return;
      const a = n.actions.find(x => x.identifier === "default") || n.actions[0];
      if (a)
        a.invoke();
    }
    function toggleDnd(): void { root.dnd = !root.dnd; }
    function setDnd(on: bool): void { root.dnd = on; }
  }

  NotificationServer {
    id: server
    keepOnReload: false
    bodySupported: true
    bodyMarkupSupported: true
    bodyHyperlinksSupported: true
    actionsSupported: true
    imageSupported: true
    persistenceSupported: true

    onNotification: n => {
      n.tracked = true;
      n.closed.connect(() => root.hidePopup(n));
      if (!root.dnd || n.urgency === NotificationUrgency.Critical)
        root.popups = [n].concat(root.popups).slice(0, 5);
    }
  }
}
