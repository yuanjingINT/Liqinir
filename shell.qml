import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

// Liqinir glass island. The Rust daemon owns state and event policy; this file
// only renders the state stream and sends explicit user actions back to it.
PanelWindow {
    id: root
    visible: true
    color: "transparent"
    implicitWidth: screen ? screen.width : 1920
    // The shell is a real top bar. Reserve only the bar height when collapsed
    // so application content starts below it instead of behind a floating UI.
    implicitHeight: expanded ? 128 : 48
    anchors.top: true
    anchors.left: true
    anchors.right: true
    WlrLayershell.layer: WlrLayer.Top
    WlrLayershell.namespace: "liqinir-island"
    exclusionMode: ExclusionMode.Auto
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.OnDemand

    property string socketPath: (Quickshell.env("LIQINIR_RUNTIME_DIR") || (Quickshell.env("XDG_RUNTIME_DIR") + "/liqinir")) + "/island.sock"
    property var snapshot: ({ type: "state", current: null, history: [], queued: 0, status: {}, agent: {} })
    property bool expanded: false
    property bool connected: false
    property string draft: ""
    property date now: new Date()
    readonly property var current: snapshot.current || null
    readonly property bool active: !!current || (snapshot.agent && snapshot.agent.busy === true)
    readonly property bool locked: snapshot.status && snapshot.status.locked === true
    readonly property color accent: current && current.priority >= 3 ? "#ff667e" : current && current.kind === "agent" ? "#a996ff" : "#72e8dc"
    readonly property string networkLabel: (snapshot.status && snapshot.status.network) || "未连接"
    readonly property string powerLabel: !snapshot.status || snapshot.status.battery === undefined ? "电量—" : snapshot.status.battery + "%" + (snapshot.status.charging ? " 充电中" : "")

    function send(command) {
        if (!ipc.connected) return
        ipc.write(JSON.stringify(command) + "\n")
        ipc.flush()
    }
    function icon(kind) {
        return ({boot: "✦", network_connected: "⌁", network_disconnected: "×", power_connected: "ϟ", power_disconnected: "⌁", unlocked: "⌁", device_connected: "⌘", device_disconnected: "⊘", notification: "•", agent: "✧"})[kind] || "•"
    }
    function titleForCurrent() { return current ? current.title : (snapshot.agent && snapshot.agent.busy === true ? "Dito 正在思考" : "Liqinir") }
    function bodyForCurrent() { return current ? (current.body || "") : (snapshot.agent ? (snapshot.agent.reply || "") : "") }
    function refreshMotion() { expanded = !expanded; send({op: "pause", value: expanded}) }

    Timer { interval: 1000; running: true; repeat: true; onTriggered: root.now = new Date() }

    // A quiet full-width surface makes the island part of the status bar while
    // leaving the actual controls confined to the center capsule.
    Rectangle {
        anchors.fill: parent
        color: "#c90d1017"
        border.width: 1
        border.color: "#25ffffff"
    }

    Row {
        anchors.left: parent.left
        anchors.leftMargin: 16
        anchors.verticalCenter: parent.verticalCenter
        spacing: 9
        visible: !root.expanded
        Text { text: "⌁  " + root.networkLabel; color: "#b9bac7"; font.pixelSize: 11 }
        Text { text: root.powerLabel; color: "#9295a6"; font.pixelSize: 11 }
    }

    Socket {
        id: ipc
        path: root.socketPath
        connected: true
        parser: SplitParser {
            splitMarker: "\n"
            onRead: data => {
                try {
                    const value = JSON.parse(data)
                    if (value.type === "state") root.snapshot = value
                } catch (error) { console.warn("liqinir: invalid state", error) }
            }
        }
        onConnectionStateChanged: {
            root.connected = connected
            if (connected) root.send({op: "snapshot"})
        }
    }

    Rectangle {
        id: island
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: parent.top
        anchors.topMargin: expanded ? 8 : 5
        width: expanded ? Math.max(280, Math.min(620, root.width - 32)) : (root.active ? 340 : 208)
        height: expanded ? 112 : 38
        radius: height / 2
        color: root.locked ? "#e50d1118" : "#d9101522"
        border.width: 1
        border.color: root.accent
        opacity: root.locked ? 0.62 : 1
        layer.enabled: true
        Behavior on width { NumberAnimation { duration: 380; easing.type: Easing.OutCubic } }
        Behavior on height { NumberAnimation { duration: 380; easing.type: Easing.OutCubic } }
        Behavior on color { ColorAnimation { duration: 260 } }

        // Fine highlights produce the liquid-glass rim while niri performs the
        // efficient xray blur behind this transparent layer surface.
        Rectangle { anchors.fill: parent; anchors.margins: 1; radius: parent.radius - 1; color: "transparent"; border.width: 1; border.color: "#35ffffff" }
        Rectangle { x: 12; y: 6; width: parent.width - 24; height: 1; radius: 1; color: "#66ffffff"; opacity: 0.6 }
        Rectangle { anchors.fill: parent; radius: parent.radius; color: root.accent; opacity: 0.035 }

        MouseArea { anchors.fill: parent; onClicked: root.refreshMotion(); onDoubleClicked: root.send({op: "dismiss"}) }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: expanded ? 24 : 18
            anchors.rightMargin: expanded ? 24 : 18
            spacing: 11
            Rectangle {
                Layout.preferredWidth: 27; Layout.preferredHeight: 27; radius: 14
                color: root.accent; opacity: 0.92
                Text { anchors.centerIn: parent; text: root.icon(root.current ? root.current.kind : (root.snapshot.agent && root.snapshot.agent.busy ? "agent" : "boot")); color: "#10121c"; font.pixelSize: 15; font.bold: true }
            }
            ColumnLayout {
                Layout.fillWidth: true; spacing: 1
                Text { Layout.fillWidth: true; text: root.titleForCurrent(); color: "#f4f4fa"; font.pixelSize: expanded ? 15 : 14; font.weight: Font.DemiBold; elide: Text.ElideRight }
                Text { Layout.fillWidth: true; visible: root.active; text: root.bodyForCurrent(); color: "#b9bac7"; font.pixelSize: 11; elide: Text.ElideRight; maximumLineCount: 1 }
            }
            Text { visible: !expanded && root.snapshot.queued > 0; text: "+" + root.snapshot.queued; color: "#a9adbe"; font.pixelSize: 11 }
            Text { visible: !expanded; text: Qt.formatTime(root.now, "hh:mm"); color: "#a9adbe"; font.pixelSize: 10 }
            Text { visible: expanded; text: "⌄"; color: "#a9adbe"; font.pixelSize: 17 }
        }

        ColumnLayout {
            visible: root.expanded
            anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
            anchors.leftMargin: 24; anchors.rightMargin: 24; anchors.bottomMargin: 10; spacing: 7
            RowLayout {
                Layout.fillWidth: true; spacing: 7
                TextField {
                    id: prompt
                    Layout.fillWidth: true; Layout.preferredHeight: 30
                    placeholderText: "问问 Dito…"
                    text: root.draft
                    color: "#eef0fa"; placeholderTextColor: "#858899"
                    background: Rectangle { radius: 15; color: "#401b2130"; border.color: "#3bffffff" }
                    onTextChanged: root.draft = text
                    Keys.onReturnPressed: { if (text.trim().length > 0) { root.send({op: "ask", prompt: text}); text = ""; root.draft = "" } }
                }
                Button {
                    Layout.preferredWidth: 64; Layout.preferredHeight: 30
                    text: snapshot.agent && snapshot.agent.busy ? "取消" : "发送"
                    onClicked: {
                        if (snapshot.agent && snapshot.agent.busy) root.send({op: "cancel_agent"})
                        else if (prompt.text.trim().length > 0) { root.send({op: "ask", prompt: prompt.text}); prompt.text = ""; root.draft = "" }
                    }
                    background: Rectangle { radius: 15; color: root.accent; opacity: 0.86 }
                    contentItem: Text { text: parent.text; color: "#11131c"; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter; font.pixelSize: 11; font.weight: Font.DemiBold }
                }
            }
            RowLayout {
                Text { Layout.fillWidth: true; text: (root.snapshot.status.network || "未连接") + "  ·  " + (root.snapshot.status.battery === undefined ? "电量—" : root.snapshot.status.battery + "%") + (root.snapshot.status.charging ? "  充电中" : ""); color: "#9295a6"; font.pixelSize: 10 }
                Button {
                    id: openDito
                    text: "Dito 终端"
                    onClicked: root.send({op: "open_dito"})
                    background: Rectangle { radius: 12; color: "#331f2433" }
                    contentItem: Text {
                        text: openDito.text
                        color: "#c8c9d5"
                        font.pixelSize: 10
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                }
                Button {
                    id: clearIsland
                    text: "清除"
                    onClicked: root.send({op: "dismiss"})
                    background: Rectangle { radius: 12; color: "#331f2433" }
                    contentItem: Text {
                        text: clearIsland.text
                        color: "#c8c9d5"
                        font.pixelSize: 10
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                }
            }
        }
    }
}
