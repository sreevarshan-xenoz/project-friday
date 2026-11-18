//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1

// FRIDAY Shell: the FRIDAY OS desktop surface, written for FRIDAY.
//
// One bar per screen, flush with the top edge; drawers that grow out of it
// (FRIDAY, controls, calendar, launcher, session); FRIDAY's answer pill;
// notifications; an OSD.
// Run it with `friday shell` or `qs -p /usr/share/friday/shell`.
import QtQuick
import Quickshell
import qs.answer
import qs.tour
import qs.bar
import qs.drawers
import qs.notifications
import qs.osd
import qs.settings
import qs.services

ShellRoot {
  Variants {
    model: Quickshell.screens
    Scope {
      id: perScreen
      required property ShellScreen modelData
      Bar { modelData: perScreen.modelData }
      DrawerHost { modelData: perScreen.modelData }
      Popups { modelData: perScreen.modelData }
      AnswerPill { modelData: perScreen.modelData }
      TourCard { modelData: perScreen.modelData }
      Osd { modelData: perScreen.modelData }
    }
  }

  SettingsWindow {}

  // Touch the singletons that must run from the start: the notification
  // server has to own the bus name before anything notifies.
  Component.onCompleted: [Notifs.dnd, Shell.drawer, Friday.mode, Answer.state, History.entries, Tour.active, Settings.open, Commands.all]
}
