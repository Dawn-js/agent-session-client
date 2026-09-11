# 手工冒烟清单

## 准备配置

应用启动时**自动按优先级查找配置文件**，取第一个存在的文件：

1. 环境变量 `VITE_AGENT_SESSION_CONFIG` 指定的路径（可选覆盖）
2. `<当前工作目录>/config.json`
3. `<exe 所在目录>/config.json`
4. **用户配置目录** `<app_config_dir>/config.json`
   - Windows 通常为 `%APPDATA%\dev.local.agent-session-client\config.json`
   - 应用内「生成示例配置」就写到这里
5. `<exe 所在目录>/examples/config.example.json`
6. `<当前工作目录>/examples/config.example.json`

> 确切路径以应用内「未找到配置文件」面板列出的为准。

**警告：真实配置值（服务器地址、用户名、agent 命令）永远不要提交到仓库。** 仓库中的 `examples/config.example.json` 始终保持 `REPLACE_WITH_*` 占位符。

### 打包后的 exe（从 Releases 下载）

打开应用即可：

- 若**找不到任何配置**，左侧显示「未找到配置文件」面板并列出上面所有已查找路径，提供：
  - **生成示例配置**：在用户配置目录（第 4 项）写入模板，随后自动重新加载；
  - **重新加载**：手动重新查找。
- 生成后编辑该文件，把 `REPLACE_WITH_*` 换成真实值，再点「重新加载」。
- 也可手动把 `config.json` 放到 exe 同级目录（第 3 项）。

### 开发模式（`npm run tauri dev`）

在项目根目录创建 `config.json`（第 2 项）即可；也可用环境变量显式指定（这是 Vite 的**构建期**变量，必须在运行 dev/build 的同一 shell 里设置）：

```powershell
$env:VITE_AGENT_SESSION_CONFIG="config.json"
npm run tauri dev
```

### 配置校验

后端逐字段校验。若某个字段为空或格式错误，应用左侧显示「配置文件无效」面板，标出文件路径与具体错误（形如 `hosts[0].name: must not be empty`）；修正后点「重新加载」。

---

前置：服务端已装 tmux；配置中的 host 与 agent 命令已替换为真实值（见上文"准备配置"）。

## A. 基础连通
1. 启动应用，左侧出现 host / agent 按钮。
2. 点一个按钮 → 右侧终端出现远端 shell / agent 界面，状态显示 `connected`。

## B. 断线恢复（核心）
3. 在终端里运行：`while true; do date; sleep 1; done`
4. 断开本机网络（关 Wi‑Fi 或拔网线）。
   - 期望：状态变为 `retrying`，右侧保留最后画面。
5. 等待 10 秒后恢复网络。
   - 期望：状态回到 `connected`；终端由 tmux 重绘出**同一会话**的画面；向上翻能看到断线期间的输出。
6. 远端确认会话唯一：`ssh <host> "tmux ls"` 应只有一个对应 session。

## C. 关客户端重开
7. 直接关闭客户端窗口（不点"结束会话"）。
8. 重新启动应用，重新点同一 host / agent。
   - 期望：接回同一会话，历史仍在。

## D. 失败分支
9. 故意把 host 改成不存在的地址 → 期望：状态进入并保持 `retrying`（网络类失败持续退避重连，间隔最长 30s）；确认它不会静默变回 `connected`，也不会变成 `exited`。停止方式：关闭该会话。
10. 把配置里的 host 指向一个没有 tmux 的机器 → 期望：出现『服务端缺 tmux，请先安装：…』提示，状态最终为 `exited`，且不会无限重试。
11. 人为 `ssh <host> "tmux kill-session -t <session>"` 后再重连 → 出现"原会话已不在，已新建"提示。注意：该提示只出现在首次启动之后被 kill 的会话；全新会话首次启动时探测不到旧会话属正常行为，不会出现该提示。
