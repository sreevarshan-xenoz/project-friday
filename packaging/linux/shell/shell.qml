//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1

// FRIDAY Shell: the FRIDAY OS desktop surface, written for FRIDAY.
//
// One bar per screen, flush with the top edge; drawers that grow out of it
// (FRIDAY, controls, calendar, launcher, session); FRIDAY's answer pill;
// notifications; an OSD; the workspace overview; the wallpaper picker;
// rounded screen corners.
// Run it with `friday shell` or `qs -p /usr/share/friday/shell`.
import QtQuick
import Quickshell
import qs.answer
import qs.tour
import qs.bar
import qs.captions
import qs.desktop
import qs.drawers
import qs.notifications
import qs.osd
import qs.overview
import qs.wallpaper
import qs.ring
import qs.settings
import qs.common
import qs.services

ShellRoot {
  Variants {
    model: Quickshell.screens
    Scope {
      id: perScreen
      required property ShellScreen modelData
      DesktopWidgets { modelData: perScreen.modelData }
      // The top bar, or the rail down the left edge (style "rail").
      // The statusline style has its own slim bar along the bottom.
      LazyLoader { active: !Theme.rail && !Theme.statusline && !Theme.taskbar; Bar { modelData: perScreen.modelData } }
      LazyLoader { active: Theme.statusline; StatuslineBar { modelData: perScreen.modelData } }
      LazyLoader { active: Theme.taskbar; TaskbarBar { modelData: perScreen.modelData } }
      LazyLoader { active: Theme.rail; RailBar { modelData: perScreen.modelData } }
      LazyLoader { active: Config.dock; Dock { modelData: perScreen.modelData } }
      LazyLoader { active: Config.shelf; Shelf { modelData: perScreen.modelData } }
      // The pet on the main screen only.
      LazyLoader { active: Config.petShown && perScreen.modelData === Quickshell.screens[0]; Pet { modelData: perScreen.modelData } }
      DrawerHost { modelData: perScreen.modelData }
      Popups { modelData: perScreen.modelData }
      AnswerPill { modelData: perScreen.modelData }
      TourCard { modelData: perScreen.modelData }
      Osd { modelData: perScreen.modelData }
      CaptionOverlay { modelData: perScreen.modelData }
      RingOverlay { modelData: perScreen.modelData }
      Overview { modelData: perScreen.modelData }
      WallpaperPicker { modelData: perScreen.modelData }
      ScreenCorners { modelData: perScreen.modelData }
      WallpaperFade { modelData: perScreen.modelData }
    }
  }

  SettingsWindow {}

  // The administrator-password card (shell.json "polkit": true).
  LazyLoader {
    active: Config.polkit
    PolkitDialog {}
  }

  // Touch the singletons that must run from the start: the notification
  // server has to own the bus name before anything notifies.
  Component.onCompleted: [Notifs.dnd, Shell.drawer, Friday.mode, Answer.state, History.entries, Tour.active, Settings.open, Commands.all, Captions.shown, Ring.shown]
}
