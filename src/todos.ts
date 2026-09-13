/** 待办清单。纯逻辑，持久化交给调用方（localStorage）。 */

export interface Todo {
  id: string;
  text: string;
  done: boolean;
}

const STORAGE_KEY = "agent-sessions.todos";

export function addTodo(list: Todo[], text: string, id: string): Todo[] {
  const trimmed = text.trim();
  if (!trimmed) return list;
  return [...list, { id, text: trimmed, done: false }];
}

export function toggleTodo(list: Todo[], id: string): Todo[] {
  return list.map((t) => (t.id === id ? { ...t, done: !t.done } : t));
}

export function removeTodo(list: Todo[], id: string): Todo[] {
  return list.filter((t) => t.id !== id);
}

export function loadTodos(): Todo[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // 存储格式是手改过的也可能坏掉，逐条校验而不是整份丢弃
    return parsed.filter(
      (t): t is Todo =>
        typeof t === "object" &&
        t !== null &&
        typeof (t as Todo).id === "string" &&
        typeof (t as Todo).text === "string" &&
        typeof (t as Todo).done === "boolean",
    );
  } catch {
    return [];
  }
}

export function saveTodos(list: Todo[]): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list));
  } catch {
    /* 存不下（隐私模式/配额）不该打断输入 */
  }
}

export function newTodoId(): string {
  return crypto.randomUUID();
}
