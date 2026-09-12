# WORKLOG

> 倒序排列，最新在顶部。每次会话收工前更新（见 AGENTS.md「收工规矩」）。

## 2026-09-12（Windows 本机同步链路打通）

**本次做了什么**

- 打通 Windows 开发机的 git ↔ GitHub 同步链路（跨机器协作基础设施，非代码改动）：
  - 排查出本机 git 直连 GitHub HTTPS 报 `Empty reply from server`，而本机 `127.0.0.1:7890` 有代理在跑；为 git 配置 `http.proxy` / `https.proxy` 后恢复
  - 配置全局身份 `Dawn-js <55617812+Dawn-js@users.noreply.github.com>`
  - `credential.helper=manager`，首次 push 完成浏览器授权，凭据已持久化
  - 仓库 clone 至 `C:\Users\sunbo\workbuddy-ai\ssh开发\agent-session-client`

**当前状态**

- Windows 本机可直接 `git pull` / `git push`，无需重复授权。

**下一步计划**

- 建议把仓库默认分支从 `feat/core` 改为 `master`（见下方「踩过的坑」）。
- `~/.ssh/id_ed25519_dawn` 尚未注册到 GitHub 账号；如需 SSH 免交互推送再补。

**踩过的坑**

- **默认分支是 `feat/core`，但它落后 `master` 7 个 merge commit**：新机器 clone 时会报 `remote HEAD refers to nonexistent ref` 且不检出任何文件，必须手动 `git checkout master`。建议在 GitHub 仓库设置里把默认分支改为 `master`。
- 排查网络问题时注意：`curl` 会走 Windows 系统代理而 `git` 不会，两者结果不一致容易误判为「GitHub 挂了」。

## 2026-09-12

**本次做了什么**

- 完成用户反馈的三件事并提交（`5836d6c`）：
  - 应用内配置编辑：后端新增 `save_config` 命令（core 校验 + 写用户配置目录），`ConfigView` 补齐 `user`/`extra_ssh_args`/`cmd` 字段；前端新增设置面板（hosts/agents 增删改）
  - 内置 agent 品牌图标：simple-icons SVG（Claude/DeepSeek/Gemini/OpenAI/Cursor），按 id/label 关键字自动匹配，未知 agent 用首字母+稳定配色字母头像兜底
  - UI 精修：agent 分组卡片、终端标题栏、欢迎页图标、设置入口、滚动条等
- 建立跨机器协作约定：AGENTS.md 增加「常用命令」与「收工规矩」，创建本 WORKLOG。

**当前状态**

- 工作分支 `feat/core`，版本 `0.1.3`；`cargo test`（50）、`npm test`（14）、`npm run build` 全绿。
- clippy 仅一条改动前就存在的警告（`run_session` 参数 8/7），未处理。

**下一步计划**

- 用户在 Windows 上 `npx tauri dev` 目视确认新界面（本机 headless 无法验证 GUI），不满意处再调。
- 确认后提升版本号（如 `0.2.0`）走 CI 发 Release。
- 继续收集真实使用中的摩擦，作为后续 roadmap 来源。

**踩过的坑**

- （历史沉淀见 AGENTS.md「前人踩过的坑」，本次无新增。）
- 注意：本机 headless Linux 无法构建/运行 GUI，界面改动只能靠 tsc/vitest/`cargo test` 保证逻辑，视觉效果必须由有 GUI 环境的机器确认。
- 图标关键字匹配有误伤可能（如 agent id 恰好含 `gpt`），目前可接受，出现再加配置字段。
