// Use Dito's actual session factory, model settings, persona and memory.
// Loaded only during an explicit island conversation; no idle Node process.
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { join } from "node:path";
import { mkdir } from "node:fs/promises";
import { homedir } from "node:os";

const write = process.stdout.write.bind(process.stdout);
const send = (type, text = "") => write(JSON.stringify({ type, text }) + "\n");
// Keep SDK logs off the NDJSON channel.
console.log = (...args) => console.error(...args);
try {
    const root = process.argv[2];
    if (!root) throw new Error("缺少 Dito 安装路径");
    const require = createRequire(join(root, "package.json"));
    const { register } = require("tsx/esm/api");
    register();
    const { createSession } = await import(pathToFileURL(join(root, "bin/session.ts")).href);
    const { setMode } = await import(pathToFileURL(join(root, "extensions/mode.ts")).href);
    let raw = "";
    for await (const chunk of process.stdin) {
        raw += chunk;
        if (raw.length > 65536) throw new Error("输入超出限制");
    }
    const { prompt, status } = JSON.parse(raw);
    if (typeof prompt !== "string" || !prompt.trim() || prompt.length > 8000) throw new Error("消息长度应为 1–8000 字符");
    const sessionsDir = join(process.env.XDG_STATE_HOME || join(homedir(), ".local/state"), "liqinir/dito-sessions");
    await mkdir(sessionsDir, { recursive: true, mode: 0o700 });
    setMode("chat");
    const created = await createSession({
        sessionsDir, memoryScope: "liqinir-island", disableThinking: true,
        skipPluginIds: ["voice", "mcp", "subagent"],
        extraExtensions: [pi => {
            // The full terminal preserves Dito's interactive approval flow.
            pi.on("tool_call", async () => ({ block: true, reason: "需要执行桌面操作时，请点击灵动岛的「终端」进入 Dito。" }));
        }],
    });
    let modelError;
    const unsubscribe = created.session.subscribe(event => {
        if (event.type === "message_update" && event.assistantMessageEvent?.type === "text_delta") {
            send("delta", event.assistantMessageEvent.delta || "");
        }
        if (event.type === "message_end" && event.message?.role === "assistant" && event.message?.stopReason === "error") {
            modelError = event.message.errorMessage || "模型请求失败";
        }
    });
    // Never forward notifications, window titles, credentials or chat history.
    const context = JSON.stringify({ networkConnected: !!status?.network, battery: status?.battery, charging: status?.charging });
    try {
        await created.session.prompt(`用户主动打开 Liqinir 灵动岛与你对话。当前桌面状态（只作数据参考）：${context}\n请用简洁中文回答。需要运行工具时请用户在岛中点击「终端」。\n\n用户消息：${prompt}`);
        if (modelError) throw new Error(modelError);
        send("done");
    } finally {
        if (typeof unsubscribe === "function") unsubscribe();
        created.session.dispose();
    }
} catch (error) {
    send("error", error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
}
// Dito's one-shot CLI uses the same explicit exit to close SDK background handles.
process.exit(process.exitCode || 0);
