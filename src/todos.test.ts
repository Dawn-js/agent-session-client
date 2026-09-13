import { describe, expect, it } from "vitest";
import { addTodo, removeTodo, toggleTodo, type Todo } from "./todos";

describe("addTodo", () => {
  it("appends a new todo as not done", () => {
    const got = addTodo([], "看看 dsh 的 profile 参数", "t1");
    expect(got).toEqual([{ id: "t1", text: "看看 dsh 的 profile 参数", done: false }]);
  });

  it("keeps existing items and trims the text", () => {
    const before: Todo[] = [{ id: "a", text: "旧的", done: true }];
    const got = addTodo(before, "  新的  ", "t2");
    expect(got.map((t) => t.text)).toEqual(["旧的", "新的"]);
    expect(before).toHaveLength(1);
  });

  it("ignores blank input", () => {
    expect(addTodo([], "   ", "t1")).toEqual([]);
  });
});

describe("toggleTodo", () => {
  it("flips only the matching item", () => {
    const before: Todo[] = [
      { id: "a", text: "一", done: false },
      { id: "b", text: "二", done: false },
    ];
    const got = toggleTodo(before, "b");
    expect(got.map((t) => t.done)).toEqual([false, true]);
  });
});

describe("removeTodo", () => {
  it("drops the matching item", () => {
    const before: Todo[] = [
      { id: "a", text: "一", done: false },
      { id: "b", text: "二", done: false },
    ];
    expect(removeTodo(before, "a").map((t) => t.id)).toEqual(["b"]);
  });
});
