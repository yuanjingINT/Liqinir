import QtQuick
import Quickshell
import qs.Services
import qs.Modules.Plugins

// DMS owns org.freedesktop.Notifications, so the bridge consumes the already
// grouped notification stream instead of registering a competing server.
PluginComponent {
    id: root
    property int forwardedThisSecond: 0
    property double lastSecond: 0

    property var seen: ({})

    Timer {
        interval: 250
        repeat: true
        running: true
        onTriggered: {
            // DMS owns the NotificationServer. Polling its already grouped
            // popup list avoids registering a competing freedesktop server.
            const popups = NotificationService.popups || []
            for (const wrapper of popups) {
                const notification = wrapper && wrapper.notification
                if (!notification || root.seen[notification.id]) continue
                root.seen[notification.id] = Date.now()
                root.forward(notification)
            }
            const cutoff = Date.now() - 30000
            for (const key in root.seen) if (root.seen[key] < cutoff) delete root.seen[key]
        }
    }

    function forward(notification) {
            const now = Date.now() / 1000
            if (now - root.lastSecond >= 1) { root.lastSecond = now; root.forwardedThisSecond = 0 }
            if (root.forwardedThisSecond++ >= 20) return
            if (!notification || !notification.summary) return
            const urgency = notification.urgency === 2 ? 3 : notification.urgency === 0 ? 0 : 1
            const event = {
                kind: "notification",
                title: String(notification.summary).slice(0, 160),
                body: String(notification.body || "").replace(/<[^>]+>/g, " ").slice(0, 1000),
                source: String(notification.appName || "DMS").slice(0, 160),
                priority: urgency,
                ttl_ms: urgency >= 3 ? 12000 : 6000
            }
            Quickshell.execDetached(["liqinir", "emit", JSON.stringify(event)])
    }
}
