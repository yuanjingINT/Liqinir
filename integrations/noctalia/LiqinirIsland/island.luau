local state = {
    current = nil,
    status = {},
    agent = {},
}

local function current_event()
    return state.current
end

local function icon_for(kind)
    local icons = {
        boot = "sparkles",
        network_connected = "wifi",
        network_disconnected = "wifi-off",
        power_connected = "plug-connected",
        power_disconnected = "plug-off",
        unlocked = "lock-open",
        device_connected = "usb",
        device_disconnected = "usb-off",
        notification = "bell",
        agent = "sparkles",
    }
    return icons[kind] or "sparkles"
end

local function idle_label()
    local network = state.status.network or "未连接"
    local battery = state.status.battery
    if battery ~= nil then
        return network .. " · " .. tostring(battery) .. "%"
    end
    return network
end

local function shorten(text, limit)
    text = tostring(text or "")
    if #text <= limit then return text end
    return string.sub(text, 1, limit - 1) .. "…"
end

local function render()
    local event = current_event()
    local agent = state.agent or {}
    local busy = agent.busy == true
    local active = event ~= nil or busy
    local title = event and (event.title or "Liqinir") or (busy and "Dito 正在思考" or idle_label())
    local body = event and (event.body or "") or (agent.reply or "")
    local kind = event and event.kind or (busy and "agent" or "boot")
    local detail = active and shorten(body, 120) or ""

    barWidget.render(ui.row({
        gap = 6,
        align = "center",
        paddingH = active and 12 or 10,
        minWidth = active and 220 or 108,
        fill = "#000000",
        radius = 22,
        border = active and "#5c5c5c" or "#242424",
        borderWidth = 1,
    }, {
        ui.glyph({ name = icon_for(kind), size = 13, color = active and "#ffffff" or "#b8b8b8" }),
        ui.label({
            text = title,
            maxWidth = active and 170 or 260,
            maxLines = 1,
            fontWeight = active and "bold" or "normal",
            color = "#ffffff",
        }),
        ui.label({
            text = detail,
            maxWidth = 210,
            maxLines = 1,
            color = "#c7c7c7",
        }),
    }))

    barWidget.setTooltip(body ~= "" and body or "Liqinir · 点击打开 Dito 终端")
end

function onClick()
    noctalia.runInTerminal("liqinir dito")
end

function onRightClick()
    noctalia.runAsync({ "liqinir", "dismiss" })
end

local function on_line(line)
    local value = noctalia.json.decode(line)
    if type(value) == "table" and value.type == "state" then
        state = value
        render()
    end
end

noctalia.setUpdateInterval(1000)
noctalia.runStream("liqinir watch", on_line)

function update()
    render()
end

render()
