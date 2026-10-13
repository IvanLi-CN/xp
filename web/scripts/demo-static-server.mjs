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
