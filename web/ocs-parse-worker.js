import init, { parse_document, serialize_document, sha256_document } from "./worker_pkg/ocs_web_worker.js?v=5";

const ready = init(
  new URL("./worker_pkg/ocs_web_worker_bg.wasm?v=5", import.meta.url),
);

self.onmessage = async ({ data }) => {
  let stage = "initialize worker";
  try {
    await ready;
    if (data.action === "hash") {
      const digest = sha256_document(new Uint8Array(data.bytes));
      self.postMessage({ ok: true, digest });
      return;
    }
    if (data.action === "serialize") {
      // SAVE/AUTOSAVE: decode the prepared document, run the DWG/DXF writer
      // here, and hand the encoded result buffer back. Mirrors the parse
      // branch's copy-before-transfer (wasm-bindgen returns a view into
      // WebAssembly.Memory, which must not detach).
      const encoded = serialize_document(new Uint8Array(data.document), (next) => {
        stage = next;
      });
      const transferable = encoded.slice();
      self.postMessage({ ok: true, data: transferable.buffer }, [transferable.buffer]);
      return;
    }
    const encoded = parse_document(
      data.name,
      new Uint8Array(data.bytes),
      data.recoveryMode === true,
      data.initialError || "",
      (next) => {
        stage = next;
      },
    );
    // wasm-bindgen returns a view into WebAssembly.Memory. Copy to a standalone
    // ArrayBuffer before transferring it, otherwise the worker's wasm memory
    // itself would be detached.
    const transferable = encoded.slice();
    self.postMessage({ ok: true, data: transferable.buffer }, [transferable.buffer]);
  } catch (error) {
    self.postMessage({
      ok: false,
      error: `${stage}: ${error instanceof Error ? error.message : String(error)}`,
    });
  }
};
