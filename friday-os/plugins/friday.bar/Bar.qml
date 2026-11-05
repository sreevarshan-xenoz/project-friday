import QtQuick

// The bar option FRIDAY OS selects in ~/.config/omarchy/shell.json. FRIDAY
// Shell draws the real bar in its own process; this only tells Omarchy's
// panels how tall it is, so they open below it.
Item {
  property string omarchyPath: ""
  property var shell: null
  property var manifest: null
  property var barWidgetRegistry: null
  property var pluginRegistry: null
  property var barConfig: null

  readonly property int barSize: 36
  readonly property bool barHidden: false
  readonly property string position: "top"

  visible: false
}
