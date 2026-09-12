import type { ConfigView } from "./config";

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
