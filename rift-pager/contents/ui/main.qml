// Rift Pager — a numbered virtual-desktop switcher.
//
// v1 is deliberately decoupled from riftd: it reads desktop state from
// libtaskmanager's VirtualDesktopInfo and switches with its requestActivate,
// so the pager works whether or not the daemon is running. Per-desktop window
// counts (optional, behind a config toggle) come from a TasksModel. Daemon-fed
// cell counts are a future enhancement.

import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import org.kde.taskmanager as TaskManager

PlasmoidItem {
    id: root

    // Lay the boxes along the panel: a row on a horizontal panel, a column on a
    // vertical one. Desktop applets report Planar; treat that as horizontal.
    readonly property bool horizontal: Plasmoid.formFactor !== PlasmaCore.Types.Vertical

    // The pager is always shown inline; it has no compact/expanded distinction.
    preferredRepresentation: fullRepresentation

    TaskManager.VirtualDesktopInfo {
        id: vdi
    }

    // Only used when window counts are enabled; ungrouped so each window is one
    // row, making the per-desktop tally a simple scan.
    TaskManager.TasksModel {
        id: tasksModel
        groupMode: TaskManager.TasksModel.GroupDisabled
    }

    // Windows assigned to `desktopId`, counting "on all desktops" windows for
    // every desktop. Best-effort: refreshed when the model size or the current
    // desktop changes (see the binding dependencies below).
    function windowsOnDesktop(desktopId) {
        var n = 0;
        for (var i = 0; i < tasksModel.count; i++) {
            var idx = tasksModel.index(i, 0);
            if (tasksModel.data(idx, TaskManager.AbstractTasksModel.IsOnAllVirtualDesktops)) {
                n++;
                continue;
            }
            var vds = tasksModel.data(idx, TaskManager.AbstractTasksModel.VirtualDesktops);
            if (vds && vds.indexOf(desktopId) !== -1) {
                n++;
            }
        }
        return n;
    }

    fullRepresentation: GridLayout {
        id: pager

        rowSpacing: Kirigami.Units.smallSpacing
        columnSpacing: Kirigami.Units.smallSpacing
        flow: root.horizontal ? GridLayout.LeftToRight : GridLayout.TopToBottom
        rows: root.horizontal ? 1 : vdi.desktopIds.length
        columns: root.horizontal ? vdi.desktopIds.length : 1

        // Square boxes sized to the panel thickness.
        readonly property int box: Math.max(
            Kirigami.Units.gridUnit,
            root.horizontal ? root.height : root.width)

        Repeater {
            model: vdi.desktopIds

            delegate: Rectangle {
                id: cell

                required property int index
                required property var modelData

                readonly property bool current: modelData === vdi.currentDesktop

                Layout.preferredWidth: pager.box
                Layout.preferredHeight: pager.box
                Layout.fillWidth: !root.horizontal
                Layout.fillHeight: root.horizontal

                radius: 3
                color: current
                    ? Kirigami.Theme.highlightColor
                    : (hover.hovered ? Kirigami.Theme.alternateBackgroundColor
                                     : Kirigami.Theme.backgroundColor)
                border.width: 1
                border.color: current
                    ? Kirigami.Theme.highlightColor
                    : Kirigami.Theme.disabledTextColor

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 0

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: cell.index + 1
                        font.bold: cell.current
                        color: cell.current
                            ? Kirigami.Theme.highlightedTextColor
                            : Kirigami.Theme.textColor
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        visible: Plasmoid.configuration.showCounts
                        // Touch the deps so the tally refreshes on window churn
                        // and desktop switches, then return the count.
                        text: {
                            tasksModel.count;
                            vdi.currentDesktop;
                            return root.windowsOnDesktop(cell.modelData);
                        }
                        font: Kirigami.Theme.smallFont
                        color: cell.current
                            ? Kirigami.Theme.highlightedTextColor
                            : Kirigami.Theme.disabledTextColor
                    }
                }

                HoverHandler {
                    id: hover
                }

                TapHandler {
                    onTapped: vdi.requestActivate(cell.modelData)
                }
            }
        }
    }
}
