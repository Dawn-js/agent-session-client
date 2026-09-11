# 手工冒烟清单

## 准备配置

应用从配置文件读取 host 与 agent 列表，配置路径的解析方式取决于运行模式：

**警告：真实配置值（服务器地址、用户名、agent 命令）永远不要提交到仓库。** 仓库中的 `examples/config.example.json` 始终保持 `REPLACE_WITH_*` 占位符。

### 模式一：开发模式（`npm run tauri dev`）

1. 在项目根目录创建一个本地 `config.json`，填入**真实值**（不要提交它；可先把它加进 `.gitignore`）。
2. 通过环境变量 `VITE_AGENT_SESSION_CONFIG` 指向该文件后，再运行 dev 命令。注意这是 Vite 的**构建期**环境变量，必须在运行 dev/build 命令的同一个 shell 里设置：
   ```powershell
   $env:VITE_AGENT_SESSION_CONFIG="config.json"
   npm run tauri dev
   ```

### 模式二：打包后的 exe（从 Releases 下载）

`load_config` 的路径**相对于进程工作目录**解析。对于下载的 exe，请把真实配置放在 exe 同级的 `examples\config.example.json`（即保持与默认相对路径一致）：

```
<exe 目录>\examples\config.example.json   ← 真实值
```

### 配置校验

后端会逐字段校验配置。若某个字段为空或格式错误，启动时会看到形如 `hosts[0].name: must not be empty` 的报错——这是配置错误时的预期表现，请按提示修正对应字段。

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
