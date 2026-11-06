// 本地 cargo 镜像桥：把 crates.io 的 sparse 索引与 .crate 下载转发为本地 HTTP，
// 从而绕开 Windows schannel 在沙箱内的凭证问题（Node 使用自带 TLS 访问上游）。
const http = require("http");

const PORT = 18080;
const UPSTREAM_INDEX = "https://index.crates.io";
const UPSTREAM_CRATES = "https://static.crates.io/crates";

const server = http.createServer(async (req, res) => {
  const url = req.url;
  try {
    if (url === "/index/config.json") {
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ dl: `http://127.0.0.1:${PORT}/crates` }));
      return;
    }
    if (url.startsWith("/index/")) {
      const path = url.slice("/index".length);
      const r = await fetch(UPSTREAM_INDEX + path);
      if (!r.ok) { res.statusCode = r.status; res.end("upstream " + r.status); return; }
      const buf = Buffer.from(await r.arrayBuffer());
      res.setHeader("content-type", r.headers.get("content-type") || "application/octet-stream");
      res.end(buf);
      return;
    }
    if (url.startsWith("/crates/")) {
      const m = url.match(/^\/crates\/([^/]+)\/([^/]+)\/download$/);
      if (!m) { res.statusCode = 404; res.end("bad path: " + url); return; }
      const crate = m[1], version = m[2];
      const r = await fetch(`${UPSTREAM_CRATES}/${crate}/${crate}-${version}.crate`, { redirect: "follow" });
      if (!r.ok) { res.statusCode = r.status; res.end("upstream " + r.status); return; }
      const buf = Buffer.from(await r.arrayBuffer());
      res.setHeader("content-type", "application/octet-stream");
      res.end(buf);
      return;
    }
    res.statusCode = 404;
    res.end("not found");
  } catch (e) {
    res.statusCode = 500;
    res.end(String((e && e.stack) || e));
  }
});

server.listen(PORT, "127.0.0.1", () => {
  console.log("cargo-bridge listening on " + PORT);
});
