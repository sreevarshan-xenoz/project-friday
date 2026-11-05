pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Services.Notifications

// The desktop notification server (org.freedesktop.Notifications). FRIDAY's
// answers arrive this way (`notify-send -a FRIDAY`, with actions), so they get
// their own look and a longer stay on screen.
//
// `popups` is what is on screen now; `list` is the history kept until
// dismissed. Do-not-disturb holds back popups except critical ones.
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
