import { createReadStream, existsSync, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(fileURLToPath(new URL("../dist-demo", import.meta.url)));
const portIndex = process.argv.indexOf("--port");
const port = Number(portIndex >= 0 ? process.argv[portIndex + 1] : 4173);
const contentTypes = {
	".css": "text/css",
	".html": "text/html",
	".js": "text/javascript",
	".json": "application/json",
};

function filePathFor(requestPath) {
	if (requestPath === "/") return join(root, "index.html");
	if (!requestPath.startsWith("/demo")) return null;
	const relative = requestPath.slice("/demo".length) || "/";
	const candidate = resolve(root, `.${relative}`);
	if (!candidate.startsWith(`${root}/`)) return null;
	if (existsSync(candidate) && statSync(candidate).isFile()) return candidate;
	if (!extname(relative)) return join(root, "404.html");
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
