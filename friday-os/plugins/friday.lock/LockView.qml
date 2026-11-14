// FRIDAY OS lock screen view: blurred, dimmed wallpaper; a large clock and the
// date; the person's picture (~/.face, else their initial); a password pill;
// a face-unlock hint while a face scan runs. Colours follow the Omarchy theme.
import QtQuick
import QtQuick.Effects
import qs.Commons as Theme

Item {
  id: root

  property string backgroundPath: ""
  property int backgroundVersion: 0
  property bool fingerprintConfigured: false
  property bool faceConfigured: false
  property bool faceAuthenticating: false
  property string faceMessage: ""
  property string userName: ""
  property bool authenticatingPassword: false
  property string failureMessage: ""
  property int failedAttempts: 0
  property bool inputEnabled: true
  property bool loadBackground: true
  property string passwordText: ""
  property bool syncingPasswordText: false

  property string home: ""
  readonly property string facePath: home + "/.face"
  readonly property bool errorState: failureMessage.length > 0
  property bool hasAvatar: false
  // The theme's lock colours, with fallbacks for the first binding pass,
  // before the theme has loaded.
  readonly property var lockColors: Theme.Color.lock
  function pick(key, fallback) {
    var value = lockColors ? lockColors[key] : undefined
    return value === undefined ? fallback : value
  }
  readonly property color textColor: pick("text", Theme.Color.foreground)
  readonly property color fieldColor: pick("background", Theme.Util.alpha(Theme.Color.background, 0.8))
  readonly property color placeholderColor: pick("placeholder", Theme.Util.alpha(Theme.Color.foreground, 0.66))
  readonly property color errorColor: pick("textError", Theme.Color.urgent)
  readonly property color borderErrorColor: pick("borderError", Theme.Color.urgent)
  readonly property color selectionColor: pick("selection", Theme.Util.alpha(Theme.Color.accent, 0.45))
  readonly property color accent: pick("borderActive", Theme.Color.accent)
  readonly property color accent2: Qt.lighter(accent, 1.25)

  signal submitPassword(string password)
  signal passwordTextEdited(string password)
  signal clearFailureRequested()
  signal wakeRequested()

  // `?v=` makes Image reload when the background changes mid-session.
  function fileUrl(path, version) {
    if (!path) return ""
    var encoded = String(path).split("/").map(encodeURIComponent).join("/")
    return "file://" + encoded + "?v=" + version
  }

  function forcePasswordFocus() {
    passwordInput.forceActiveFocus()
  }

  function syncPasswordText() {
    if (passwordInput.text === passwordText) return
    syncingPasswordText = true
    passwordInput.text = passwordText
    syncingPasswordText = false
  }

  onPasswordTextChanged: syncPasswordText()
  onInputEnabledChanged: if (inputEnabled) Qt.callLater(forcePasswordFocus)
  onFailedAttemptsChanged: if (failedAttempts > 0) shake.restart()
  Component.onCompleted: {
    syncPasswordText()
    if (inputEnabled) Qt.callLater(forcePasswordFocus)
  }

  // Ticks on the minute.
  QtObject { id: clock; property date date: new Date() }
  Timer {
    interval: (60 - clock.date.getSeconds()) * 1000
    running: true
    repeat: true
    onTriggered: clock.date = new Date()
  }

  Rectangle {
    anchors.fill: parent
    color: Theme.Color.background

    Image {
      id: wallpaper
      anchors.fill: parent
      source: root.loadBackground ? root.fileUrl(root.backgroundPath, root.backgroundVersion) : ""
      fillMode: Image.PreserveAspectCrop
      asynchronous: true
      cache: false
      sourceSize.width: width
      sourceSize.height: height
      visible: false
    }

    MultiEffect {
      anchors.fill: wallpaper
      source: wallpaper
      autoPaddingEnabled: false
      visible: wallpaper.status === Image.Ready
      blurEnabled: true
      blur: 0.7
      blurMax: 48
      brightness: -0.3
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      onClicked: { root.wakeRequested(); root.forcePasswordFocus() }
      onPositionChanged: root.wakeRequested()
    }
  }

  Column {
    id: column
    anchors.centerIn: parent
    anchors.verticalCenterOffset: -40
    spacing: 0

    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: Qt.formatTime(clock.date, "hh:mm")
      color: root.textColor
      font.family: Theme.Style.font.family
      font.pixelSize: 140
      font.weight: Font.ExtraBold
    }

    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: Qt.formatDate(clock.date, "dddd, d MMMM")
      color: Theme.Util.alpha(root.textColor, 0.85)
      font.family: Theme.Style.font.family
      font.pixelSize: 28
    }

    Item { width: 1; height: 44 }

    // The person's picture in a ring of the accent colour, or their initial.
    Item {
      anchors.horizontalCenter: parent.horizontalCenter
      width: 116
      height: 116

      Rectangle {
        anchors.fill: parent
        radius: width / 2
        gradient: Gradient {
          orientation: Gradient.Horizontal
          GradientStop { position: 0; color: root.accent }
          GradientStop { position: 1; color: root.accent2 }
        }
      }

      Rectangle {
        id: avatarBase
        anchors.fill: parent
        anchors.margins: 3
        radius: width / 2
        color: Theme.Color.background

        Text {
          anchors.centerIn: parent
          visible: avatar.status !== Image.Ready
          text: (root.userName || "?").charAt(0).toUpperCase()
          color: root.textColor
          font.family: Theme.Style.font.family
          font.pixelSize: 44
          font.weight: Font.DemiBold
        }
      }

      Image {
        id: avatar
        anchors.fill: avatarBase
        source: root.hasAvatar ? "file://" + root.facePath : ""
        fillMode: Image.PreserveAspectCrop
        sourceSize.width: width
        sourceSize.height: height
        cache: false
        visible: false
      }

      Rectangle {
        id: avatarMask
        anchors.fill: avatarBase
        radius: width / 2
        visible: false
        layer.enabled: true
      }

      MultiEffect {
        anchors.fill: avatarBase
        source: avatar
        visible: avatar.status === Image.Ready
        maskEnabled: true
        maskSource: avatarMask
        maskThresholdMin: 0.5
        maskSpreadAtMin: 1.0
      }
    }

    Item { width: 1; height: 28 }

    // Password pill.
    Rectangle {
      id: field
      anchors.horizontalCenter: parent.horizontalCenter
      width: 340
      height: 56
      radius: height / 2
      color: root.fieldColor
      border.width: 2
      border.color: root.errorState ? root.borderErrorColor : (passwordInput.activeFocus ? root.accent : Theme.Util.alpha(root.textColor, 0.25))

      Behavior on border.color { ColorAnimation { duration: 150 } }

      SequentialAnimation {
        id: shake
        loops: 1
        NumberAnimation { target: field; property: "anchors.horizontalCenterOffset"; to: -12; duration: 50 }
        NumberAnimation { target: field; property: "anchors.horizontalCenterOffset"; to: 12; duration: 70 }
        NumberAnimation { target: field; property: "anchors.horizontalCenterOffset"; to: -6; duration: 60 }
        NumberAnimation { target: field; property: "anchors.horizontalCenterOffset"; to: 0; duration: 50 }
      }

      Text {
        id: lockGlyph
        anchors.left: parent.left
        anchors.leftMargin: 22
        anchors.verticalCenter: parent.verticalCenter
        text: root.fingerprintConfigured ? "󰈷" : "󰌾"
        color: root.placeholderColor
        font.family: Theme.Style.font.family
        font.pixelSize: 20
      }

      TextInput {
        id: passwordInput
        anchors.fill: parent
        anchors.leftMargin: 56
        anchors.rightMargin: 56
        verticalAlignment: TextInput.AlignVCenter
        horizontalAlignment: TextInput.AlignHCenter
        clip: true
        enabled: root.inputEnabled && !root.authenticatingPassword
        readOnly: root.authenticatingPassword
        echoMode: TextInput.Password
        passwordCharacter: "●"
        passwordMaskDelay: 0
        color: root.textColor
        selectionColor: root.selectionColor
        font.family: Theme.Style.font.family
        font.pixelSize: 20
        font.letterSpacing: 4
        cursorDelegate: Rectangle { width: 2; color: root.textColor; visible: passwordInput.cursorVisible && passwordInput.text.length > 0 }

        onTextChanged: {
          if (!root.syncingPasswordText) root.passwordTextEdited(text)
          if (text.length > 0) root.wakeRequested()
          if (text.length > 0 && root.errorState) root.clearFailureRequested()
        }

        onAccepted: {
          var submitted = root.passwordText
          root.passwordTextEdited("")
          if (submitted.length > 0) root.submitPassword(submitted)
        }

        Keys.onPressed: function(event) {
          root.wakeRequested()
          if (event.key === Qt.Key_Escape || (event.modifiers & Qt.ControlModifier && event.key === Qt.Key_U)) {
            root.passwordTextEdited("")
            event.accepted = true
          }
        }
      }

      Text {
        anchors.fill: passwordInput
        visible: passwordInput.text.length === 0
        text: root.authenticatingPassword ? "Checking…" : (root.errorState ? root.failureMessage : "Password")
        color: root.errorState ? root.errorColor : root.placeholderColor
        font.family: Theme.Style.font.family
        font.pixelSize: 17
        font.italic: root.errorState
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
      }
    }

    Item { width: 1; height: 16 }

    // Face unlock: what the face scan is doing, with a pulse while it looks.
    Rectangle {
      anchors.horizontalCenter: parent.horizontalCenter
      visible: root.faceConfigured && root.faceMessage.length > 0
      width: faceRow.implicitWidth + 32
      height: 36
      radius: height / 2
      color: Theme.Util.alpha(root.fieldColor, 0.7)

      Row {
        id: faceRow
        anchors.centerIn: parent
        spacing: 10

        Text {
          anchors.verticalCenter: parent.verticalCenter
          text: "󰄀"
          color: root.accent
          font.family: Theme.Style.font.family
          font.pixelSize: 16

          SequentialAnimation on opacity {
            running: root.faceAuthenticating
            loops: Animation.Infinite
            alwaysRunToEnd: true
            NumberAnimation { to: 0.35; duration: 600; easing.type: Easing.InOutSine }
            NumberAnimation { to: 1.0; duration: 600; easing.type: Easing.InOutSine }
          }
        }

        Text {
          anchors.verticalCenter: parent.verticalCenter
          text: root.faceMessage
          color: root.textColor
          font.family: Theme.Style.font.family
          font.pixelSize: 14
        }
      }
    }
  }

  Text {
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: 40
    text: "󰣇  " + root.userName
    color: root.accent
    font.family: Theme.Style.font.family
    font.pixelSize: 17
  }
}
