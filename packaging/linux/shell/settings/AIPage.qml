import QtQuick
import QtQuick.Layouts
import Quickshell
import qs.common
import qs.components

ColumnLayout {
  id: page
  spacing: 16

  // [label, provider id]
  readonly property var providers: [
    ["Free models", "opencode"], ["Google Gemini", "gemini"], ["Groq", "groq"],
    ["NVIDIA NIM", "nvidia-nim"], ["Runcrate", "runcrate"], ["AxForge", "axforge"], ["OpenRouter", "openrouter"], ["Cerebras", "cerebras"]
  ]
  property string provider: "opencode"
  readonly property bool free: provider === "opencode"

  function inTerminal(title, cmd) {
    Quickshell.execDetached(["foot", "-T", "FRIDAY · " + title, "bash", "-c",
      cmd + "; echo; read -rsn1 -p 'Press any key to close.'"]);
  }

  Label {
    Layout.fillWidth: true
    text: "FRIDAY thinks with a language model. Free models work with no account, but they may keep what you send. For anything private, use your own key, an account you already have, or keep everything on this computer."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.subtext
  }

  // How each provider is doing right now (friday providers): a slow or
  // missing answer usually means a free limit was hit.
  Label { text: "Right now"; font.weight: Font.DemiBold }
  Cmd { id: providers; args: ["providers"] }
  Output { text: providers.output }
  Chip { text: providers.running ? "Checking…" : "Check again"; onClicked: providers.run() }

  Label { text: "Choose the AI"; font.weight: Font.DemiBold }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Repeater {
      model: page.providers
      Chip {
        required property var modelData
        text: modelData[0]
        primary: page.provider === modelData[1]
        onClicked: page.provider = modelData[1]
      }
    }
  }

  Rectangle {
    visible: !page.free
    Layout.fillWidth: true
    implicitHeight: 38
    radius: Theme.radiusS
    color: Theme.surface
    border.width: 1
    border.color: key.activeFocus ? Theme.accent : Theme.outline
    TextInput {
      id: key
      anchors.fill: parent
      anchors.margins: 10
      echoMode: TextInput.Password
      color: Theme.text
      font.family: Theme.sans
      font.pixelSize: Theme.fontM
      clip: true
      Text {
        visible: key.text === "" && !key.activeFocus
        text: "Paste your key here"
        color: Theme.faint
        font: key.font
      }
    }
  }

  Cmd {
    id: save
    onFinished: output => {
      // Only a saved key is cleared: after a failure it stays, to be fixed and sent again.
      if (/^(Saved|Using OpenCode)/m.test(output)) key.text = "";
      page.confirmingFree = false;
    }
  }
  // Saving a key adds it: the keys already saved stay, and FRIDAY can fall back
  // to them. Going back to the free models removes them, so it asks first.
  property bool confirmingFree: false
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip {
      primary: true
      text: save.running ? "Saving…" : (page.free ? "Use the free models" : "Save my key")
      onClicked: {
        if (page.free) { page.confirmingFree = true; return; }
        save.args = ["setup-ai", "--keep-others", page.provider];
        save.run([], key.text);
      }
    }
  }
  Rectangle {
    visible: page.confirmingFree
    Layout.fillWidth: true
    implicitHeight: freeAsk.implicitHeight + 24
    radius: Theme.radiusS
    color: Theme.surface
    border.width: 1
    border.color: Theme.warn
    ColumnLayout {
      id: freeAsk
      anchors.fill: parent
      anchors.margins: 12
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: "Use only the free models? Any AI keys you saved are removed from this computer (you can add them again later)."
        wrapMode: Text.Wrap
        elide: Text.ElideNone
      }
      Row {
        spacing: 8
        Chip {
          primary: true
          text: "Yes, free models only"
          onClicked: { save.args = ["setup-ai", "--only", "opencode"]; save.run([], ""); }
        }
        Chip { text: "Cancel"; onClicked: page.confirmingFree = false }
      }
    }
  }
  Output { text: save.output }
  Label {
    Layout.fillWidth: true
    text: "A saved key is added to the ones you already have; FRIDAY can fall back to them. It is stored only on this computer and never shown again."
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }

  // Local AI: who answers first. Auto: above 4B the local model, at 4B and
  // below cloud first while online (friday local-ai order).
  Label { text: "Local AI"; font.weight: Font.DemiBold; Layout.topMargin: 8 }
  Cmd { id: localAi; args: ["local-ai", "status"] }
  Component.onCompleted: { localAi.run(); providers.run(); engines.run(); }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Repeater {
      model: [["Auto", "auto"], ["Local first", "local"], ["Cloud first", "cloud"]]
      Chip {
        required property var modelData
        text: modelData[0]
        primary: localAi.output.indexOf("Order: " + modelData[1] + ":") >= 0
        onClicked: { localAi.args = ["local-ai", "order", modelData[1]]; localAi.run(); }
      }
    }
    Chip { text: "Set up local AI"; onClicked: page.inTerminal("local AI", "friday setup-ai local") }
  }
  Output { text: localAi.output }
  // A download on its way: how far it is, refreshed every two seconds.
  readonly property int downloadPercent: {
    const m = localAi.output.match(/downloading .*: (\d+)%/);
    return m ? parseInt(m[1]) : -1;
  }
  Rectangle {
    visible: page.downloadPercent >= 0
    Layout.fillWidth: true
    implicitHeight: 6
    radius: 3
    color: Theme.glassEdge
    Rectangle { width: parent.width * page.downloadPercent / 100; height: parent.height; radius: 3; color: Theme.accent }
  }
  Timer {
    interval: 2000
    repeat: true
    running: page.visible && localAi.output.indexOf("downloading") >= 0
    onTriggered: { localAi.args = ["local-ai", "status"]; localAi.run(); }
  }

  // Engines already on this computer (Ollama, llama.cpp) and the models they
  // have: "Use" answers with one, nothing downloaded.
  Cmd {
    id: engines
    args: ["local-ai", "engines", "--json"]
    property var found: []
    onFinished: (out) => { try { found = JSON.parse(out); } catch (e) { found = []; } }
  }
  Cmd { id: useModel; args: ["local-ai", "use"]; onFinished: { localAi.args = ["local-ai", "status"]; localAi.run(); engines.run(); } }
  Repeater {
    model: engines.found
    ColumnLayout {
      id: engineRow
      required property var modelData
      Layout.fillWidth: true
      spacing: 4
      visible: modelData.installed
      Label {
        text: (modelData.engine === "ollama" ? "Ollama" : "llama.cpp") + (modelData.running ? " (running)" : " (installed, not running)")
        color: Theme.faint
        font.pixelSize: Theme.fontS + 1
      }
      Flow {
        Layout.fillWidth: true
        spacing: 8
        Repeater {
          model: modelData.models
          Chip {
            required property var modelData
            text: "Use " + modelData.name + (modelData.size_mb > 0 ? " · " + (modelData.size_mb / 1024).toFixed(1) + " GB" : "") + (modelData.vision ? " · sees screenshots" : "")
            primary: localAi.output.indexOf("on, " + modelData.name + " ") >= 0
            onClicked: useModel.run([engineRow.modelData.engine, modelData.name])
          }
        }
      }
    }
  }
  Output { text: useModel.output }
  Label {
    Layout.fillWidth: true
    text: "Auto: a model above 4B answers first; a smaller one backs up cloud, so questions go to a cloud model first while you're online. Local first keeps them on this computer whenever the local model can answer."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }

  Label { text: "More ways"; font.weight: Font.DemiBold; Layout.topMargin: 8 }
  Flow {
    Layout.fillWidth: true
    spacing: 8
    Chip { text: "Use an account I already have"; onClicked: page.inTerminal("connect an account", "friday connect") }
    Chip { text: "Keep everything on this computer"; onClicked: page.inTerminal("private mode", "friday private on") }
  }
  Label {
    Layout.fillWidth: true
    text: "These two open a small window: signing in to an account happens there, and private mode installs a local model and asks for your computer password."
    wrapMode: Text.Wrap
    elide: Text.ElideNone
    color: Theme.faint
    font.pixelSize: Theme.fontS + 1
  }
}
