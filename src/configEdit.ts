import type { AgentView, ConfigView } from "./config";

/** 设置面板里的可编辑 host 行（表单用字符串，保存时才转结构化 JSON）。 */
export interface EditableHost {
  name: string;
  host: string;
  user: string;
  /** 额外 ssh 参数，空格分隔，如 "-p 2222" */
  extra: string;
}

/** 设置面板里的可编辑 agent 行。 */
export interface EditableAgent {
  id: string;
  label: string;
  cmd: string;
}

export function toEditableHosts(config: ConfigView): EditableHost[] {
  return config.hosts.map((h) => ({
    name: h.name,
    host: h.host,
    user: h.user ?? "",
    extra: h.extra_ssh_args.join(" "),
  }));
}

export function toEditableAgents(config: ConfigView): EditableAgent[] {
  return config.agents.map((a) => ({ id: a.id, label: a.label, cmd: a.cmd }));
}

/**
 * 把服务器探测到的 agent 追加进可编辑列表。已存在（按 trim 后的 id 比较）的不重复加，
 * 空 id 直接跳过 —— 加入后仍要过一遍后端校验，这里只做去重。
 */
export function mergeDiscoveredAgents(
  existing: EditableAgent[],
  discovered: AgentView[],
): EditableAgent[] {
  const known = new Set(existing.map((a) => a.id.trim()));
  const merged = [...existing];
  for (const agent of discovered) {
    const id = agent.id.trim();
    if (!id || known.has(id)) continue;
    known.add(id);
    merged.push({ id: agent.id, label: agent.label, cmd: agent.cmd });
  }
  return merged;
}

/**
 * 把编辑行转成后端 `save_config` 接受的配置 JSON。字段全部 trim；空 user /
 * 空额外参数直接省略（与后端序列化格式一致）。行内字段为空是允许的——校验
 * 交给后端，错误信息里的 hosts[i]/agents[i] 下标与本数组的顺序一致。
 */
export function buildConfigJson(hosts: EditableHost[], agents: EditableAgent[]): string {
  return JSON.stringify(
    {
      hosts: hosts.map((h) => {
        const user = h.user.trim();
        const extra = h.extra.trim().split(/\s+/).filter(Boolean);
        return {
          name: h.name.trim(),
          host: h.host.trim(),
          ...(user ? { user } : {}),
          ...(extra.length > 0 ? { extra_ssh_args: extra } : {}),
        };
      }),
      agents: agents.map((a) => ({
        id: a.id.trim(),
        label: a.label.trim(),
        cmd: a.cmd.trim(),
      })),
    },
    null,
    2,
  );
}

/**
 * 首次启动时，用一个本机 ssh 别名 + **该主机上实际探测到的** agent 生成初始配置。
 * host 直接取别名本身、`user` 留空 —— 连接时 ssh 会自己去 `~/.ssh/config`
 * 解释 HostName/User/Port。
 *
 * 一个 agent 都没探到就返回 `null`：后端校验要求 `agents` 非空，写一份必然被拒的
 * 配置只会把用户送进一个点了就报错的界面。绝对不要退回「把注册表全集写进去」——
 * 那会在没装该 agent 的服务器上留下一个点了必挂的按钮，也无法经过配置校验。
 */
export function bootstrapConfigJson(
  alias: string,
  discovered: AgentView[],
): string | null {
  if (discovered.length === 0) return null;
  return buildConfigJson([{ name: alias, host: alias, user: "", extra: "" }], discovered);
}
