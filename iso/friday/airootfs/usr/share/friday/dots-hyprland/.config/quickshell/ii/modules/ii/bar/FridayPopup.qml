import qs.modules.common
import qs.modules.common.widgets
import qs.services
import QtQuick
import QtQuick.Layouts

StyledPopup {
    id: root

    function shorten(s, n) {
        return s.length > n ? s.slice(0, n - 1) + "…" : s;
    }

    ColumnLayout {
        anchors.centerIn: parent
        spacing: 4

        StyledPopupHeaderRow {
            icon: "neurology"
            label: "FRIDAY"
        }

        StyledPopupValueRow {
            icon: Friday.present ? (Friday.running ? "check_circle" : "warning") : "power_off"
            label: Translation.tr("Daemon:")
            value: Friday.present ? Friday.status : Translation.tr("offline")
        }

        StyledPopupValueRow {
            visible: Friday.working
            icon: "progress_activity"
            label: Friday.phase !== "" ? Friday.phase + ":" : Translation.tr("Working on:")
            value: root.shorten(Friday.goal, 40)
        }

        StyledPopupValueRow {
            visible: !Friday.working && Friday.lastOutcome !== ""
            icon: Friday.lastFailed ? "error" : "task_alt"
            label: Translation.tr("Last goal:")
            value: Friday.lastOutcome
        }

        StyledText {
            visible: !Friday.working && Friday.lastDetail !== ""
            Layout.maximumWidth: 320
            wrapMode: Text.Wrap
            maximumLineCount: 3
            elide: Text.ElideRight
            color: Appearance.colors.colSubtext
            text: Friday.lastDetail
        }

        StyledPopupValueRow {
            visible: Friday.present
            icon: Friday.voiceOn ? "mic" : "mic_off"
            label: Translation.tr("Voice:")
            value: Friday.voice !== "" ? Friday.voice : "off"
        }

        StyledText {
            Layout.topMargin: 2
            color: Appearance.colors.colSubtext
            font.pixelSize: Appearance.font.pixelSize.smaller
            text: Translation.tr("Click or Super+Y to ask")
        }
    }
}
