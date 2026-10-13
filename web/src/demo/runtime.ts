import { createDemoApi } from "./apiModel";
import {
	isDemoRuntime,
	readDemoAuthentication,
	setDemoAuthentication,
} from "./runtimeMode";
import {
	DEFAULT_DEMO_CONTROLS,
	type DemoControls,
	readDemoControls,
} from "./shareState";

export type DemoOperation = {
	id: number;
	method: string;
	route: string;
	status: number;
};
type DemoSnapshot = {
	controls: DemoControls;
	operations: DemoOperation[];
	generation: number;
};

let snapshot: DemoSnapshot = {
	controls: DEFAULT_DEMO_CONTROLS,
	operations: [],
	generation: 0,
};
const listeners = new Set<() => void>();
let api = createDemoApi(snapshot.controls);
let operationId = 0;
let installed = false;

export function getDemoSnapshot(): DemoSnapshot {
	return snapshot;
}
export function subscribeDemo(listener: () => void): () => void {
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}

function publish(next: DemoSnapshot) {
	snapshot = next;
	for (const listener of listeners) listener();
}

export function syncDemoControls(search: string, reset = false) {
	const controls = readDemoControls(search);
	const old = snapshot.controls;
	if (JSON.stringify(old) === JSON.stringify(controls) && !reset) return;
	if (reset || controls.scene !== old.scene || controls.profile !== old.profile)
		api = createDemoApi(controls);
	setDemoAuthentication(controls.persona === "admin");
	publish({
		controls,
		operations: reset ? [] : snapshot.operations,
		generation: snapshot.generation + 1,
	});
}

export function resetDemoModel() {
	syncDemoControls(window.location.search, true);
}

function journalRoute(path: string): string {
	// Record route shapes only: never a bearer token, user input, query or body.
	return path
		.replace(/\/sub\/[^/]+/, "/sub/:token")
		.replace(/\/(users|nodes|endpoints|monitors)\/[^/]+/, "/$1/:id");
}

function failure(status: number, code: string, message: string) {
	return Response.json({ error: { code, message } }, { status });
}

async function delay(milliseconds: number, signal: AbortSignal) {
	if (signal.aborted) throw new DOMException("Request aborted", "AbortError");
	await new Promise<void>((resolve, reject) => {
		const abort = () => {
			clearTimeout(timer);
			reject(new DOMException("Request aborted", "AbortError"));
		};
		const timer = setTimeout(() => {
			signal.removeEventListener("abort", abort);
			resolve();
		}, milliseconds);
		signal.addEventListener("abort", abort, { once: true });
	});
}

export async function handleDemoRequest(request: Request): Promise<Response> {
	const path = new URL(request.url).pathname;
	const { network } = snapshot.controls;
	await delay(network === "slow" ? 1500 : 80, request.signal);
	if (network === "offline") throw new TypeError("Simulated offline network.");
	let response: Response;
	if (network === "error")
		response = failure(503, "demo_network_error", "Simulated backend failure.");
	else if (path.startsWith("/api/admin/") && !readDemoAuthentication()) {
		response = failure(
			401,
			"unauthorized",
			"Simulated administrator authentication required.",
		);
	} else response = await api.handle(request);
	if (!["GET", "HEAD", "OPTIONS"].includes(request.method)) {
		publish({
			...snapshot,
			operations: [
				{
					id: ++operationId,
					method: request.method,
					route: journalRoute(path),
					status: response.status,
				},
				...snapshot.operations,
			].slice(0, 12),
		});
	}
	return response;
}

export async function bootstrapDemoRuntime(): Promise<void> {
	if (!isDemoRuntime() || installed) return;
	const controls = readDemoControls(window.location.search);
	api = createDemoApi(controls);
	setDemoAuthentication(controls.persona === "admin");
	publish({ controls, operations: [], generation: snapshot.generation + 1 });
	const nativeFetch = window.fetch.bind(window);
	window.fetch = async (input, init) => {
		const normalized =
			typeof input === "string"
				? new URL(input, window.location.origin)
				: input;
		const request = new Request(normalized, init);
		const url = new URL(request.url);
		if (url.pathname === "/api" || url.pathname.startsWith("/api/"))
			return handleDemoRequest(request);
		if (
			url.origin === window.location.origin &&
			request.method === "GET" &&
			/\.(js|css|svg|png|ico|woff2?)(?:$)/.test(url.pathname)
		)
			return nativeFetch(request);
		return failure(
			503,
			"demo_request_blocked",
			"Demo never sends unhandled requests to a real service.",
		);
	};
	installed = true;
}
