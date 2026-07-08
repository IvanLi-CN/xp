import { useRegisterSW } from "virtual:pwa-register/react";

import { Button } from "./Button";
import { Icon } from "./Icon";

export function PwaStatusPrompt() {
	const {
		offlineReady: [, setOfflineReady],
		needRefresh: [needRefresh, setNeedRefresh],
		updateServiceWorker,
	} = useRegisterSW({
		onRegisterError(error) {
			console.error("Failed to register service worker", error);
		},
	});

	if (!needRefresh) {
		return null;
	}

	return (
		<div
			className={
				"fixed bottom-4 right-4 z-50 max-w-sm rounded-2xl " +
				"border border-border/80 bg-popover/95 p-4 shadow-2xl backdrop-blur"
			}
		>
			<div className="space-y-3">
				<div className="flex items-start gap-3">
					<div className="mt-0.5 rounded-full bg-primary/10 p-2 text-primary">
						<Icon
							name={needRefresh ? "tabler:download" : "tabler:wifi-off"}
							size={18}
						/>
					</div>
					<div className="space-y-1">
						<p className="text-sm font-semibold text-foreground">
							A newer web bundle is ready.
						</p>
						<p className="text-sm text-muted-foreground">
							Reload to switch to the newest frontend assets without waiting for
							a full browser refresh.
						</p>
					</div>
				</div>
				<div className="flex flex-wrap justify-end gap-2">
					<Button
						variant="ghost"
						size="sm"
						onClick={() => {
							setOfflineReady(false);
							setNeedRefresh(false);
						}}
					>
						Close
					</Button>
					<Button
						size="sm"
						onClick={() => {
							void updateServiceWorker(true);
						}}
					>
						Reload
					</Button>
				</div>
			</div>
		</div>
	);
}
