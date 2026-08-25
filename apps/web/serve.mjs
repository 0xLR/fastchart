import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, resolve } from "node:path";

const root = resolve("dist");
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".wasm": "application/wasm" };
createServer((request, response) => {
  const requested = new URL(request.url, "http://localhost").pathname;
  const path = resolve(root, `.${requested === "/" ? "/index.html" : requested}`);
  if (!path.startsWith(root)) { response.writeHead(403).end(); return; }
  try {
    if (!statSync(path).isFile()) throw new Error();
    response.writeHead(200, { "Content-Type": types[extname(path)] || "application/octet-stream" });
    createReadStream(path).pipe(response);
  } catch { response.writeHead(404).end("Not found"); }
}).listen(4173, "127.0.0.1", () => console.log("FastChart: http://127.0.0.1:4173"));
