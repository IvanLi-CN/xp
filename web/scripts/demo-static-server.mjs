import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const liveRoot = resolve(fileURLToPath(new URL("../dist", import.meta.url)));
const demoRoot = resolve(
	fileURLToPath(new URL("../dist-demo", import.meta.url)),
);
const portIndex = process.argv.indexOf("--port");
const port = Number(portIndex >= 0 ? process.argv[portIndex + 1] : 4173);
const contentTypes = {
	".css": "text/css",
	".html": "text/html",
	".js": "text/javascript",
	".json": "application/json",
};
const legacyDemoWorkerSource = [
	"self.addEventListener('install', (event) => event.waitUntil(self.skipWaiting()));",
	"self.addEventListener('activate', (event) => event.waitUntil(self.clients.claim()));",
	"self.addEventListener('fetch', (event) => {",
	"const url = new URL(event.request.url);",
	"if (event.request.mode === 'navigate' && url.pathname.startsWith('/demo'))",
	"event.respondWith(fetch(new URL('/index.html', url)));",
	"});",
].join("\n");

function filePathFor(requestPath) {
	if (requestPath === "/") return join(demoRoot, "index.html");
	if (requestPath.startsWith("/demo")) {
		const relative = requestPath.slice("/demo".length) || "/";
		const candidate = resolve(demoRoot, `.${relative}`);
		if (!candidate.startsWith(`${demoRoot}/`)) return null;
		if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
		if (!extname(relative)) return join(demoRoot, "404.html");
		return null;
	}
	const candidate = resolve(liveRoot, `.${requestPath}`);
	if (!candidate.startsWith(`${liveRoot}/`)) return null;
	if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
	return null;
}

createServer((request, response) => {
	const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
	if (requestUrl.pathname === "/__e2e_legacy_demo_worker__.js") {
		response.writeHead(200, {
			"Cache-Control": "no-store",
			"Content-Type": "text/javascript",
			"Service-Worker-Allowed": "/",
		});
		response.end(legacyDemoWorkerSource);
		return;
	}
	const filePath = filePathFor(normalize(requestUrl.pathname));
	if (!filePath) {
		response.writeHead(404).end();
		return;
	}
	response.setHeader(
		"Content-Type",
		contentTypes[extname(filePath)] ?? "application/octet-stream",
	);
	createReadStream(filePath).pipe(response);
}).listen(port, "127.0.0.1");
