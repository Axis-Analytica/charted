// Module worker proving that .wasm files load inside WebKitGTK workers, first
// via streaming compilation (requires an application/wasm MIME type) and
// falling back to instantiating from an ArrayBuffer.
self.onmessage = async () => {
  const url = new URL("/spike-add.wasm", self.location.origin).href;
  let mode = "streaming";
  let instance: WebAssembly.Instance;
  try {
    ({ instance } = await WebAssembly.instantiateStreaming(fetch(url)));
  } catch (e) {
    mode = `arraybuffer (streaming failed: ${String(e)})`;
    const bytes = await (await fetch(url)).arrayBuffer();
    ({ instance } = await WebAssembly.instantiate(bytes));
  }
  const add = instance.exports.add as (a: number, b: number) => number;
  self.postMessage({ mode, result: add(40, 2) });
};
