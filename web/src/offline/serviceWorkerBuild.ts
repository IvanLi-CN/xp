let installed = false;

export function declareServiceWorkerBuild(buildId = __XP_WEB_BUILD_ID__): void {
	if (typeof navigator === "undefined" || !("serviceWorker" in navigator))
		return;
	if (installed) return;
	installed = true;

	const sendDeclaration = () => {
		navigator.serviceWorker.controller?.postMessage({
			type: "XP_DECLARE_BUILD",
			buildId,
		});
	};
	const handleMessage = (event: MessageEvent) => {
		if (
			event.data?.type === "XP_REQUEST_BUILD_DECLARATION" ||
			event.data?.type === "XP_CACHE_MISS"
		) {
			sendDeclaration();
		}
	};

	navigator.serviceWorker.addEventListener("message", handleMessage);
	navigator.serviceWorker.addEventListener("controllerchange", sendDeclaration);
	sendDeclaration();
	void navigator.serviceWorker.ready.then(sendDeclaration);
}
