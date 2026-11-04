import qs.modules.common
import qs.modules.common.widgets
import qs.services
import QtQuick

/**
 * FRIDAY status in the bar.
 *  - idle:     solid primary icon
 *  - working:  icon "breathes" (scale + opacity) and a halo ring pulses out
 *  - degraded: amber-ish tertiary tint
 *  - offline:  dimmed outline icon
 * Colour and fill changes cross-fade; the pulse eases back to rest instead of
 * snapping when a goal finishes. Click opens `friday ask` (same as SUPER+Y);
 * hover shows the detail popup.
 */
MouseArea {
    id: root

    readonly property string mode: Friday.mode
    readonly property bool working: mode === "working"

    implicitWidth: Appearance.sizes.barHeight * 0.8
    implicitHeight: Appearance.sizes.barHeight
    hoverEnabled: !Config.options.bar.tooltips.clickToShow
    acceptedButtons: Qt.LeftButton
    cursorShape: Qt.PointingHandCursor
    onClicked: Friday.ask()

    readonly property color iconColor: {
        switch (mode) {
        case "working": return Appearance.colors.colPrimary;
        case "idle": return Appearance.colors.colOnLayer1;
        case "degraded": return Appearance.m3colors.m3tertiary;
        default: return Appearance.colors.colSubtext;
        }
    }

    // Halo: expands and fades out on a loop while working.
    Rectangle {
        id: halo
        anchors.centerIn: icon
        width: icon.implicitWidth
        height: width
        radius: width / 2
        color: "transparent"
        border.width: 2
        border.color: Appearance.colors.colPrimary
        opacity: 0
        scale: 1

        ParallelAnimation {
            running: root.working
            loops: Animation.Infinite
            alwaysRunToEnd: true
            NumberAnimation { target: halo; property: "scale"; from: 0.8; to: 1.7; duration: 1400; easing.type: Easing.OutCubic }
            NumberAnimation { target: halo; property: "opacity"; from: 0.55; to: 0; duration: 1400; easing.type: Easing.OutCubic }
        }
    }

    MaterialSymbol {
        id: icon
        anchors.centerIn: parent
        text: "neurology"
        iconSize: Appearance.font.pixelSize.larger
        fill: root.mode === "offline" ? 0 : 1
        color: root.iconColor
        opacity: root.mode === "offline" ? 0.6 : 1

        Behavior on color {
            animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
        }
        Behavior on opacity {
            NumberAnimation { duration: 250; easing.type: Easing.OutCubic }
        }
        Behavior on fill {
            NumberAnimation { duration: 250; easing.type: Easing.OutCubic }
        }

        // Breathing while working; alwaysRunToEnd lets it settle at rest.
        SequentialAnimation on scale {
            running: root.working
            loops: Animation.Infinite
            alwaysRunToEnd: true
            NumberAnimation { from: 1.0; to: 1.12; duration: 700; easing.type: Easing.InOutSine }
            NumberAnimation { from: 1.12; to: 1.0; duration: 700; easing.type: Easing.InOutSine }
        }
    }

    // Press feedback, matching the bar's other buttons.
    scale: pressed ? 0.88 : 1
    Behavior on scale {
        NumberAnimation { duration: 160; easing.type: Easing.OutBack }
    }

    FridayPopup {
        hoverTarget: root
    }
}
