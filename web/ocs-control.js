// Web automation convenience API (#1349).
//
// The wasm module exposes the control bridge through `window.wasmBindings`
// (ocs_control_submit / ocs_control_take). This module wraps it in promises:
//
//   await window.ocsControl.ready            resolves once the bridge exists
//   window.addEventListener("ocs-ready", …)  fired once, at the same moment
//   await window.ocsControl.request({...})   one control request -> final reply
//
// Hosts that intend to automate from the start should set
// localStorage["opencadstudio.automation"] = "1" before the page loads so the
// startup donation prompt never appears (see docs/automation/README.md).
(() => {
  if (window.ocsControl) return;
  const bridge = () => {
    const api = window.wasmBindings;
    return api && api.ocs_control_submit && api.ocs_control_take ? api : null;
  };
  const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

  let readyResolve;
  const ready = new Promise(resolve => { readyResolve = resolve; });
  const settle = () => {
    // apiObject is assigned to window.ocsControl at the END of this IIFE;
    // resolve with the local reference so a synchronous bridge (script
    // injected late) cannot settle `ready` with undefined.
    readyResolve(apiObject);
    window.dispatchEvent(new Event("ocs-ready"));
  };
  const poll = setInterval(() => {
    if (bridge()) {
      clearInterval(poll);
      settle();
    }
  }, 50);
  if (bridge()) {
    clearInterval(poll);
    settle();
  }

  // Submit one request and poll take() until the app stores its reply.
  async function exchange(api, payload, pollMs, deadline) {
    const ticket = api.ocs_control_submit(JSON.stringify(payload));
    for (;;) {
      const raw = api.ocs_control_take(ticket);
      if (raw) return JSON.parse(raw);
      if (performance.now() >= deadline) {
        throw new Error("ocs-control: timed out: " + JSON.stringify(payload));
      }
      await sleep(pollMs);
    }
  }

  // One control request. Replies that are still `accepted`/`running` (an
  // async open, capture or save) are followed through `op:"operation"` until
  // a final status arrives. Resolves with the final reply object; rejects
  // only on timeout or a missing bridge.
  async function request(payload, options = {}) {
    const pollMs = options.pollMs ?? 50;
    const deadline = performance.now() + (options.timeoutMs ?? 120000);
    const api = bridge();
    if (!api) {
      throw new Error("ocs-control: bridge not ready (await ocsControl.ready)");
    }
    let reply = await exchange(api, payload, pollMs, deadline);
    while (["accepted", "running"].includes(reply.status) && reply.request_id) {
      await sleep(pollMs);
      reply = await exchange(
        api, {op: "operation", request_id: reply.request_id}, pollMs, deadline);
    }
    return reply;
  }

  const apiObject = {ready, request};

  window.ocsControl = apiObject;
})();
