import init, { parse_document, sha256_document } from "./worker_pkg/ocs_web_worker.js?v=5";

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
    const encoded = parse_document(
      data.name,
      new Uint8Array(data.bytes),
      data.recoveryMode === true,
      data.initialError || "",
      (next) => {
        stage = next;
      },
    );
    // Rust returns Uint8Array::from(slice), which already owns a JS buffer.
    // Transfer it directly: copying the whole serialized drawing again doubles
    // the output-buffer peak for large documents.
    self.postMessage({ ok: true, data: encoded.buffer }, [encoded.buffer]);
  } catch (error) {
    self.postMessage({
      ok: false,
      error: `${stage}: ${error instanceof Error ? error.message : String(error)}`,
    });
  }
};
