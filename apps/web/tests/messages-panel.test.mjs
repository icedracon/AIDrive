import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const component = ts.transpileModule(
  readFileSync(new URL("../src/app/messages-panel.tsx", import.meta.url), "utf8"),
  { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } },
).outputText;

function message(overrides = {}) {
  return {
    id: "incoming", thread_id: "thread", reply_to: null, from_email: null,
    from_agent_name: "claude", to: "assistants", to_agent_name: null,
    subject: null, body: "Please check this", created_at: "2026-11-26T10:00:00Z",
    read_by_me: false, sources: [], ...overrides,
  };
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function mount({ active = true, visibility = "visible", messages = [], chats = [], send } = {}) {
  const hooks = [];
  const requests = [];
  const intervals = new Map();
  const timeouts = new Map();
  const document = { visibilityState: visibility };
  let cursor = 0, dirty = true, tree, pendingEffects = [], uuidCount = 0, timerId = 0;
  let props = { workspace: { id: "workspace", name: "Project", role: "owner" }, active };
  const changed = (before, after) => !before || !after || before.length !== after.length || after.some((v, i) => !Object.is(v, before[i]));
  const react = {
    useRef(value) {
      const index = cursor++;
      return (hooks[index] ??= { current: value });
    },
    useState(initial) {
      const index = cursor++;
      hooks[index] ??= { value: typeof initial === "function" ? initial() : initial };
      return [hooks[index].value, (next) => {
        const value = typeof next === "function" ? next(hooks[index].value) : next;
        if (!Object.is(value, hooks[index].value)) { hooks[index].value = value; dirty = true; }
      }];
    },
    useMemo(factory, deps) {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) hooks[index] = { value: factory(), deps };
      return hooks[index].value;
    },
    useEffect(effect, deps) {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) {
        pendingEffects.push(() => {
          hooks[index]?.cleanup?.();
          hooks[index] = { deps, cleanup: effect() };
        });
      }
    },
  };
  const jsx = (type, props) => ({ type, props: props ?? {} });
  const componentModule = { exports: {} };
  vm.runInNewContext(component, {
    module: componentModule, exports: componentModule.exports,
    require(name) {
      if (name === "react") return react;
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx, Fragment: "fragment" };
      if (name === "./ui") return { ClampText: "ClampText", utc: (value) => value };
      throw new Error(`Unexpected component dependency: ${name}`);
    },
    document, AbortController,
    crypto: { randomUUID: () => `00000000-0000-4000-8000-${String(++uuidCount).padStart(12, "0")}` },
    setInterval: (callback) => { const id = ++timerId; intervals.set(id, callback); return id; },
    clearInterval: (id) => intervals.delete(id),
    window: {
      setTimeout: (callback) => { const id = ++timerId; timeouts.set(id, callback); return id; },
      clearTimeout: (id) => timeouts.delete(id),
    },
    async fetch(url, options = {}) {
      const request = { url, method: options.method ?? "GET", body: options.body };
      requests.push(request);
      if (request.method === "POST" && url.endsWith("/messages")) {
        assert.ok(send, "Unexpected message send");
        return send(request);
      }
      const body = url.includes("/messages?") ? { messages: structuredClone(messages) }
        : url.endsWith("/chats") ? { chats: structuredClone(chats) } : {};
      return { ok: true, status: 200, json: async () => body };
    },
  }, { filename: "messages-panel.transpiled.cjs" });

  function render() {
    dirty = false;
    cursor = 0;
    tree = componentModule.exports.default(props);
    const effects = pendingEffects;
    pendingEffects = [];
    effects.forEach((effect) => effect());
  }

  return {
    requests, document,
    get uuidCount() { return uuidCount; },
    setProps(next) { props = { ...props, ...next }; dirty = true; },
    async flush() {
      for (let i = 0; i < 30; i++) {
        if (dirty) render();
        await new Promise((resolve) => setImmediate(resolve));
        if (!dirty) return;
      }
      assert.fail("Component effects did not settle");
    },
    poll() { [...intervals.values()].forEach((callback) => callback()); },
    find(predicate) {
      function* walk(node) {
        if (Array.isArray(node)) { for (const child of node) yield* walk(child); }
        else if (node && typeof node === "object") { yield node; yield* walk(node.props?.children); }
      }
      const found = [...walk(tree)].find(predicate);
      assert.ok(found, "Expected rendered element was missing");
      return found;
    },
    change(type, value) { this.find((node) => node.type === type).props.onChange({ target: { value } }); },
    submit() { this.find((node) => node.type === "form").props.onSubmit({ preventDefault() {} }); },
    unmount() { hooks.forEach((hook) => hook?.cleanup?.()); },
  };
}

test("inactive MessagesPanel never acknowledges unread messages; activating it acknowledges only unread owner messages", async (t) => {
  const panel = mount({ active: false, messages: [
    message({ id: "owner-unread", to: "owner" }),
    message({ id: "broadcast" }),
    message({ id: "owner-read", to: "owner", read_by_me: true }),
  ] });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.poll();
  await panel.flush();
  assert.ok(panel.requests.some((r) => r.url.includes("/messages?")), "Unread data must actually reach the hidden component");
  assert.equal(panel.requests.filter((r) => r.method === "POST").length, 0);

  panel.setProps({ active: true });
  await panel.flush();
  const acknowledgements = panel.requests.filter((r) => r.method === "POST");
  assert.equal(acknowledgements.length, 1);
  assert.equal(acknowledgements[0].url, "/api/workspaces/workspace/messages/read");
  assert.deepEqual(JSON.parse(acknowledgements[0].body), { ids: ["owner-unread"] });

  panel.setProps({ active: false });
  await panel.flush();
  panel.poll();
  await panel.flush();
  assert.equal(panel.requests.filter((r) => r.method === "POST").length, 1, "Polling an inactive panel must not acknowledge new response data");
});

test("an active panel in a hidden document waits until visible data is rendered before acknowledging", async (t) => {
  const panel = mount({ visibility: "hidden", messages: [message({ to: "owner" })] });
  t.after(() => panel.unmount());
  await panel.flush();
  assert.equal(panel.requests.filter((r) => r.method === "POST").length, 0);
  panel.document.visibilityState = "visible";
  panel.poll();
  await panel.flush();
  assert.equal(panel.requests.filter((r) => r.url.endsWith("/read")).length, 1);
});

test("a lost response retries the same request ID and complete payload, while editing the body starts a new attempt", async (t) => {
  const pending = [];
  const panel = mount({
    messages: [message()],
    chats: [{ id: "chat", agent_name: "claude", client: "claude-code", cwd: "/project", last_seen: "2026-11-26T10:00:00Z" }],
    send: () => { const result = deferred(); pending.push(result); return result.promise; },
  });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.find((node) => node.type === "button" && Array.isArray(node.props.children) && node.props.children[0] === "Reply").props.onClick();
  await panel.flush();
  panel.change("select", "chat");
  panel.change("textarea", "  Check the release  ");
  await panel.flush();
  panel.submit();
  await panel.flush();
  const sends = () => panel.requests.filter((r) => r.method === "POST" && r.url.endsWith("/messages"));
  assert.equal(sends().length, 1);
  assert.deepEqual(JSON.parse(sends()[0].body), {
    request_id: "00000000-0000-4000-8000-000000000001",
    to: "claude", body: "Check the release", reply_to: "incoming", to_chat: "chat",
  });
  pending[0].reject(new TypeError("Connection lost after server accepted the message"));
  await panel.flush();
  assert.ok(panel.find((node) => node.props.role === "alert"));
  panel.submit();
  await panel.flush();
  assert.equal(sends().length, 2);
  assert.equal(sends()[1].body, sends()[0].body, "A lost-response retry must preserve the exact serialized payload, including request_id");
  assert.equal(panel.uuidCount, 1);

  pending[1].reject(new TypeError("Retry response also lost"));
  await panel.flush();
  panel.change("textarea", "Check the updated release");
  await panel.flush();
  panel.submit();
  await panel.flush();
  const edited = JSON.parse(sends()[2].body);
  assert.equal(edited.body, "Check the updated release");
  assert.notEqual(edited.request_id, JSON.parse(sends()[0].body).request_id);
  assert.equal(panel.uuidCount, 2);
  assert.deepEqual({ ...edited, request_id: undefined, body: undefined }, {
    request_id: undefined, body: undefined, to: "claude", reply_to: "incoming", to_chat: "chat",
  });
  pending[2].resolve({ status: 201, json: async () => ({ id: "sent" }) });
  await panel.flush();
  assert.equal(panel.find((node) => node.type === "textarea").props.value, "");
});

test("the real submit handler blocks double submission before a rerender and while a request is busy", async (t) => {
  const pending = deferred();
  const panel = mount({
    messages: [message()],
    chats: [{ id: "chat", agent_name: "claude", client: "claude-code", cwd: "/project", last_seen: "2026-11-26T10:00:00Z" }],
    send: () => pending.promise,
  });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.find((node) => node.type === "button" && Array.isArray(node.props.children) && node.props.children[0] === "Reply").props.onClick();
  await panel.flush();
  panel.change("textarea", "Send once");
  await panel.flush();
  const mutationControls = [
    ["recipient", (node) => node.type === "input"],
    ["chat", (node) => node.type === "select"],
    ["body", (node) => node.type === "textarea"],
    ["cancel reply", (node) => node.type === "button" && node.props.children === "Cancel reply"],
    ["reply", (node) => node.type === "button" && Array.isArray(node.props.children) && node.props.children[0] === "Reply"],
  ];
  for (const [name, predicate] of mutationControls) {
    assert.equal(panel.find(predicate).props.disabled, false, `${name} must be editable before sending`);
  }
  const submit = panel.find((node) => node.type === "form").props.onSubmit;
  submit({ preventDefault() {} });
  submit({ preventDefault() {} });
  await panel.flush();
  assert.equal(panel.find((node) => node.type === "button" && node.props.type === "submit").props.disabled, true);
  for (const [name, predicate] of mutationControls) {
    assert.equal(panel.find(predicate).props.disabled, true, `${name} must be disabled while the send is pending`);
  }
  panel.submit();
  assert.equal(panel.requests.filter((r) => r.method === "POST" && r.url.endsWith("/messages")).length, 1);
  assert.equal(panel.uuidCount, 1);
  pending.resolve({ status: 201, json: async () => ({ id: "sent" }) });
  await panel.flush();
  assert.equal(panel.find((node) => node.type === "textarea").props.value, "");
});
