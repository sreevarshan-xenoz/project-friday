import QtQuick
import QtQuick.Layouts
import qs.common
import qs.components

// Every feature that can be switched off on its own (`friday feature`): one
// switch each, from `friday feature list`, so a new feature shows up here
// without touching this page.
ColumnLayout {
  id: page
  spacing: 16

  Label {
    Layout.fillWidth: true
    text: "Switch any of these off if you don't want it. They're all on to start with; turning one off takes effect from the next ask."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  // [{ on, name, what }] from the list's lines: "on|off  <name>  <what it does>".
  property var features: []
  Cmd {
    id: list
    args: ["feature", "list"]
    onFinished: output => {
      const out = [];
      for (const line of output.split("\n")) {
        const m = line.match(/^(on|off)\s+([a-z-]+)\s+(.+)$/);
        if (m) out.push({ on: m[1] === "on", name: m[2], what: m[3] });
      }
      page.features = out;
    }
  }
  Component.onCompleted: list.run()

  Repeater {
    model: page.features
    Toggle {
      required property var modelData
      title: modelData.what
      detail: "friday feature " + modelData.name + " on|off"
      statusArgs: ["feature", modelData.name, "status"]
      onArgs: ["feature", modelData.name, "on"]
      offArgs: ["feature", modelData.name, "off"]
      onText: modelData.name + ": on"
      checked: modelData.on
    }
  }
}
