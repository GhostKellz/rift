import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

Kirigami.FormLayout {
    // `cfg_<key>` aliases are how plasmoid config pages bind to main.xml entries.
    property alias cfg_showCounts: showCounts.checked

    QQC2.CheckBox {
        id: showCounts
        Kirigami.FormData.label: i18n("Window counts:")
        text: i18n("Show a window-count badge on each desktop")
    }
}
