import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const component = ts.transpileModule(
  readFileSync(new URL("../src/app/session-transcript.tsx", import.meta.url), "utf8"),
  { compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2022 } },
).outputText;

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function event(ordinal, text = `Captured message ${ordinal}`) {
  return { ordinal, key: `event-${ordinal}`, role: "assistant", at: null, text };
}

function page(events, { next = null, count = events.length, sessionId = "session" } = {}) {
  return {
    session: { id: sessionId, client: "claude-code", label: "Native session", coverage: "captured", event_count: count, started_at: null, ended_at: null },
    events, next_cursor: next,
  };
}

function response(body, status = 200) {
  return { ok: status >= 200 && status < 300, status, json: async () => structuredClone(body) };
}

// Like messages-panel.test.mjs, this executes TSX with hook and host stubs, not a DOM or browser.
function mount({ fetch: handleFetch = () => response(page([event(0)])), visibility = "visible", hidden = false } = {}) {
  const hooks = [], requests = [], intervals = new Map(), listeners = new Map();
  const overview = { hidden };
  const document = {
    visibilityState: visibility,
    getElementById(id) { assert.equal(id, "panel-overview"); return overview; },
    addEventListener(name, callback) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name).add(callback);
    },
    removeEventListener(name, callback) { listeners.get(name)?.delete(callback); },
  };
  let cursor = 0, dirty = true, tree, pendingEffects = [], timerId = 0, now = 0, mounted = true, lateWrites = 0;
  let props = { workspaceId: "workspace", chatId: "chat" };
  const changed = (before, after) => !before || !after || before.length !== after.length || after.some((value, index) => !Object.is(value, before[index]));
  const react = {
    useRef(value) { return (hooks[cursor++] ??= { current: value }); },
    useState(initial) {
      const index = cursor++;
      hooks[index] ??= { value: typeof initial === "function" ? initial() : initial };
      return [hooks[index].value, (next) => {
        if (!mounted) { lateWrites++; return; }
        const value = typeof next === "function" ? next(hooks[index].value) : next;
        if (!Object.is(value, hooks[index].value)) { hooks[index].value = value; dirty = true; }
      }];
    },
    useCallback(callback, deps) {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) hooks[index] = { value: callback, deps };
      return hooks[index].value;
    },
    useEffect(effect, deps) {
      const index = cursor++;
      if (changed(hooks[index]?.deps, deps)) pendingEffects.push({ index, effect, deps });
    },
  };
  const jsx = (type, props, key) => ({ type, props: props ?? {}, key });
  const componentModule = { exports: {} };
  const setInterval = (callback, delay) => {
    assert.ok(delay > 0);
    const id = ++timerId;
    intervals.set(id, { callback, delay, due: now + delay });
    return id;
  };
  const clearInterval = (id) => intervals.delete(id);
  vm.runInNewContext(component, {
    module: componentModule, exports: componentModule.exports,
    require(name) {
      if (name === "react") return react;
      if (name === "react/jsx-runtime") return { jsx, jsxs: jsx, Fragment: "fragment" };
      if (name === "./session-transcript.module.css") return { default: new Proxy({}, { get: (_, key) => key }) };
      throw new Error(`Unexpected component dependency: ${name}`);
    },
    document, AbortController, URLSearchParams, Error, setInterval, clearInterval,
    window: { setInterval, clearInterval },
    fetch(url, options = {}) {
      const request = { url, ...options };
      const index = requests.length;
      requests.push(request);
      return Promise.resolve().then(() => handleFetch(request, index));
    },
  }, { filename: "session-transcript.transpiled.cjs" });

  function* walk(node) {
    if (Array.isArray(node)) { for (const child of node) yield* walk(child); }
    else if (node && typeof node === "object") { yield node; yield* walk(node.props?.children); }
  }
  function text(node) {
    if (Array.isArray(node)) return node.map(text).join("");
    if (node && typeof node === "object") return text(node.props?.children);
    return node === null || node === undefined || typeof node === "boolean" ? "" : String(node);
  }
  return {
    requests, document, overview,
    get timerCount() { return intervals.size; },
    get listenerCount() { return [...listeners.values()].reduce((sum, entries) => sum + entries.size, 0); },
    get lateWrites() { return lateWrites; },
    get text() { return text(tree); },
    get articles() { return [...walk(tree)].filter((node) => node.type === "article"); },
    get alerts() { return [...walk(tree)].filter((node) => node.props.role === "alert").map(text); },
    setProps(next) { props = { ...props, ...next }; dirty = true; },
    async flush() {
      for (let i = 0; i < 30; i++) {
        if (dirty && mounted) {
          dirty = false;
          cursor = 0;
          tree = componentModule.exports.default(props);
          const effects = pendingEffects;
          pendingEffects = [];
          effects.forEach(({ index }) => hooks[index]?.cleanup?.());
          effects.forEach(({ index, effect, deps }) => { hooks[index] = { deps, cleanup: effect() }; });
        }
        await new Promise((resolve) => setImmediate(resolve));
        if (!dirty || !mounted) return;
      }
      assert.fail("Component effects did not settle");
    },
    advance(milliseconds) {
      const target = now + milliseconds;
      for (;;) {
        const next = [...intervals.values()].sort((a, b) => a.due - b.due)[0];
        if (!next || next.due > target) break;
        now = next.due;
        next.due += next.delay;
        next.callback();
      }
      now = target;
    },
    visibility(value) {
      document.visibilityState = value;
      [...(listeners.get("visibilitychange") ?? [])].forEach((callback) => callback());
    },
    click(label) {
      const button = [...walk(tree)].find((node) => node.type === "button" && text(node) === label);
      assert.ok(button, `Missing button: ${label}`);
      assert.ok(!button.props.disabled, `Disabled button: ${label}`);
      button.props.onClick();
    },
    unmount() {
      if (!mounted) return;
      mounted = false;
      hooks.forEach((hook) => hook?.cleanup?.());
    },
  };
}

test("five-second automatic refresh renders new long text and repairs existing keys without a manual click", async (t) => {
  const longText = "Full captured answer. ".repeat(600) + "END OF CAPTURE";
  let snapshot = page([event(0, "Earlier shortened answer")]);
  const panel = mount({ fetch: () => response(snapshot) });
  t.after(() => panel.unmount());
  await panel.flush();
  assert.equal(panel.articles.length, 1);
  snapshot = page([event(0, longText), event(1, "New captured answer")]);
  panel.advance(4999);
  await panel.flush();
  assert.equal(panel.requests.length, 1);
  panel.advance(1);
  await panel.flush();
  assert.equal(panel.requests.length, 2);
  assert.equal(panel.articles.length, 2);
  assert.equal(panel.articles[0].props.children[1].props.children, longText);
  assert.equal(panel.articles[1].props.children[1].props.children, "New captured answer");
  assert.equal(panel.articles[0].key, "event-0");
  assert.ok(!panel.text.includes("Earlier shortened answer"));
  assert.deepEqual(panel.alerts, []);
  for (const request of panel.requests) {
    assert.equal(request.url, "/api/workspaces/workspace/chats/chat/transcript?limit=100");
    assert.equal(request.cache, "no-store");
  }
});

test("automatic refresh refetches the loaded prefix plus 100 events and keeps repairing earlier text", async (t) => {
  const events = Array.from({ length: 450 }, (_, ordinal) => event(ordinal));
  const panel = mount({ fetch: ({ url }) => {
    const query = new URL(url, "http://test.local").searchParams;
    assert.equal(query.get("limit"), "100");
    const after = query.get("after_ordinal");
    const start = after === null ? 0 : Number(after) + 1;
    if (after !== null) assert.equal(query.get("after_key"), `event-${start - 1}`);
    const batch = events.slice(start, start + 100);
    const last = batch.at(-1);
    return response(page(batch, { count: events.length, next: start + 100 < events.length ? { ordinal: last.ordinal, key: last.key } : null }));
  } });
  t.after(() => panel.unmount());
  await panel.flush();
  assert.equal(panel.articles.length, 100);
  for (const expected of [200, 300, 400, 450]) {
    events[0] = event(0, `Repaired prefix at ${expected}`);
    const before = panel.requests.length;
    panel.advance(5000);
    await panel.flush();
    assert.equal(panel.articles.length, expected);
    assert.equal(panel.requests.length - before, Math.ceil(expected / 100));
    assert.equal(new URL(panel.requests[before].url, "http://test.local").searchParams.has("after_ordinal"), false);
    assert.equal(panel.articles[0].props.children[1].props.children, `Repaired prefix at ${expected}`);
    assert.equal(new Set(panel.articles.map((node) => node.key)).size, expected);
  }
});

test("hidden overview and hidden browser tab suppress polling; visibility resumes fresh data", async (t) => {
  let snapshot = page([event(0)]);
  const panel = mount({ fetch: () => response(snapshot) });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.overview.hidden = true;
  panel.advance(15000);
  panel.visibility("visible");
  await panel.flush();
  assert.equal(panel.requests.length, 1, "Hidden overview must suppress both timer and visibility-triggered refreshes");
  panel.overview.hidden = false;
  panel.visibility("hidden");
  panel.advance(15000);
  await panel.flush();
  assert.equal(panel.requests.length, 1, "A hidden tab must not poll even when overview is selected");
  snapshot = page([event(0), event(1, "Visible again")]);
  panel.visibility("visible");
  await panel.flush();
  assert.equal(panel.requests.length, 2);
  assert.ok(panel.text.includes("Visible again"));
  panel.advance(5000);
  await panel.flush();
  assert.equal(panel.requests.length, 3);
});

for (const hiddenSurface of ["overview", "tab"]) {
  test(`hiding the ${hiddenSurface} during a prefix request stops pagination, preserves history, and resumes from the beginning`, async (t) => {
    const pending = deferred();
    const prefix = page([event(0, "Updated prefix")], { next: { ordinal: 0, key: "event-0" }, count: 2 });
    const panel = mount({ fetch: ({ url }, index) => {
      if (index === 0) return response(page([event(0, "Previously displayed history")]));
      if (index === 1) return pending.promise;
      return response(url.includes("after_ordinal") ? page([event(1, "New suffix")], { count: 2 }) : prefix);
    } });
    t.after(() => panel.unmount());
    await panel.flush();
    panel.advance(5000);
    await panel.flush();
    assert.equal(panel.requests.length, 2);

    if (hiddenSurface === "overview") panel.overview.hidden = true;
    else panel.visibility("hidden");
    pending.resolve(response(prefix));
    await panel.flush();
    assert.equal(panel.requests.length, 2, "The next page must not start after visibility is lost");
    assert.equal(panel.articles.length, 1);
    assert.ok(panel.text.includes("Previously displayed history"));
    assert.ok(!panel.text.includes("Updated prefix"), "An interrupted prefix must not replace displayed history");
    assert.deepEqual(panel.alerts, []);
    panel.advance(15000);
    await panel.flush();
    assert.equal(panel.requests.length, 2, "Hidden timers must leave the interrupted sweep stopped");

    if (hiddenSurface === "overview") {
      panel.overview.hidden = false;
      panel.advance(5000);
    } else panel.visibility("visible");
    await panel.flush();
    assert.equal(panel.requests.length, 4, "Visibility must release the pending guard and allow a complete sweep");
    assert.equal(panel.requests[2].url, "/api/workspaces/workspace/chats/chat/transcript?limit=100");
    const cursor = new URL(panel.requests[3].url, "http://test.local").searchParams;
    assert.equal(cursor.get("after_ordinal"), "0");
    assert.equal(cursor.get("after_key"), "event-0");
    assert.equal(panel.articles.length, 2);
    assert.ok(panel.text.includes("Updated prefix"));
    assert.ok(panel.text.includes("New suffix"));
    assert.ok(!panel.text.includes("Previously displayed history"));
    assert.deepEqual(panel.alerts, []);
  });
}

test("a pending automatic update cannot overlap another timer or visibility refresh", async (t) => {
  const pending = deferred();
  const panel = mount({ fetch: (_, index) => index === 0 ? response(page([event(0)])) : pending.promise });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.advance(5000);
  await panel.flush();
  panel.advance(15000);
  panel.visibility("visible");
  await panel.flush();
  assert.equal(panel.requests.length, 2);
  pending.resolve(response(page([event(0), event(1)])));
  await panel.flush();
  assert.equal(panel.articles.length, 2);
});

test("network loss midway through prefix refresh retains all prior text and recovers on the next timer", async (t) => {
  let polling = false, fail = true;
  const original = page([event(0, "Keep this history")]);
  const panel = mount({ fetch: ({ url }) => {
    if (!polling) return response(original);
    if (!url.includes("after_ordinal")) return response(page([event(0, "Incomplete replacement")], { next: { ordinal: 0, key: "event-0" }, count: 2 }));
    if (fail) throw new TypeError("Network disconnected");
    return response(page([event(1, "Recovered update")], { count: 2 }));
  } });
  t.after(() => panel.unmount());
  await panel.flush();
  polling = true;
  panel.advance(5000);
  await panel.flush();
  assert.ok(panel.text.includes("Keep this history"));
  assert.ok(!panel.text.includes("Incomplete replacement"));
  assert.equal(panel.alerts.length, 1);
  assert.ok(panel.alerts[0].includes("Previously loaded events remain below."));
  fail = false;
  panel.advance(5000);
  await panel.flush();
  assert.ok(panel.text.includes("Recovered update"));
  assert.deepEqual(panel.alerts, []);
});

test("manual refresh aborts an automatic request and its late error cannot overwrite manual state", async (t) => {
  const automatic = deferred(), manual = deferred();
  const panel = mount({ fetch: (_, index) => index === 0 ? response(page([event(0)])) : index === 1 ? automatic.promise : manual.promise });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.advance(5000);
  await panel.flush();
  panel.click("Refresh transcript");
  await panel.flush();
  assert.equal(panel.requests[1].signal.aborted, true);
  panel.advance(10000);
  await panel.flush();
  assert.equal(panel.requests.length, 3, "Timer must leave an in-flight manual refresh alone");
  assert.equal(panel.requests[2].signal.aborted, false);
  manual.resolve(response(page([event(5, "Manual result wins")])));
  await panel.flush();
  automatic.reject(new TypeError("Late aborted automatic failure"));
  await panel.flush();
  assert.ok(panel.text.includes("Manual result wins"));
  assert.deepEqual(panel.alerts, []);
});

for (const changedProp of ["workspaceId", "chatId"]) {
  test(`changing ${changedProp} aborts stale automatic work and ignores its late result`, async (t) => {
    const stale = deferred();
    const panel = mount({ fetch: (_, index) => index === 0 ? response(page([event(0)])) : index === 1 ? stale.promise : response(page([event(8, "New scope")], { sessionId: "new-session" })) });
    t.after(() => panel.unmount());
    await panel.flush();
    panel.advance(5000);
    await panel.flush();
    panel.setProps({ [changedProp]: "new/scope" });
    await panel.flush();
    assert.equal(panel.requests[1].signal.aborted, true);
    assert.ok(panel.requests[2].url.includes("new%2Fscope"));
    stale.resolve(response(page([event(7, "Stale scope")])));
    await panel.flush();
    assert.ok(panel.text.includes("New scope"));
    assert.ok(!panel.text.includes("Stale scope"));
    assert.deepEqual(panel.alerts, []);
    assert.equal(panel.timerCount, 1);
    assert.equal(panel.listenerCount, 1);
    panel.advance(5000);
    await panel.flush();
    assert.equal(panel.requests.length, 4);
    assert.ok(panel.requests[3].url.includes("new%2Fscope"));
  });
}

test("unmount aborts an in-flight refresh and removes timers and visibility listeners", async () => {
  const pending = deferred();
  const panel = mount({ fetch: (_, index) => index === 0 ? response(page([event(0)])) : pending.promise });
  await panel.flush();
  panel.advance(5000);
  await panel.flush();
  assert.equal(panel.timerCount, 1);
  assert.equal(panel.listenerCount, 1);
  panel.unmount();
  assert.equal(panel.requests[1].signal.aborted, true);
  assert.equal(panel.timerCount, 0);
  assert.equal(panel.listenerCount, 0);
  pending.reject(new TypeError("Request settled after unmount"));
  await panel.flush();
  panel.advance(15000);
  panel.visibility("visible");
  await panel.flush();
  assert.equal(panel.requests.length, 2);
  assert.equal(panel.lateWrites, 0);
});

test("manual refresh during automatic JSON decoding prevents further aborted pagination", async (t) => {
  const body = deferred();
  const panel = mount({ fetch: (_, index) => index === 0 ? response(page([event(0)])) : index === 1
    ? { ok: true, status: 200, json: () => body.promise }
    : response(page([event(9, "Manual replacement")])) });
  t.after(() => panel.unmount());
  await panel.flush();
  panel.advance(5000);
  await panel.flush();
  panel.click("Refresh transcript");
  await panel.flush();
  assert.equal(panel.requests[1].signal.aborted, true);
  body.resolve(page([event(0, "Stale body")], { next: { ordinal: 0, key: "event-0" }, count: 200 }));
  await panel.flush();
  assert.ok(panel.text.includes("Manual replacement"));
  assert.deepEqual(panel.alerts, []);
  assert.equal(panel.requests.length, 3, "Aborted JSON decoding must not issue the next page request");
});
