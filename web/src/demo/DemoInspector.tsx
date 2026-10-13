import { useQueryClient } from "@tanstack/react-query";
import { useRouter, useRouterState } from "@tanstack/react-router";
import { type ReactNode, useEffect, useRef, useState } from "react";

import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { Icon } from "../components/Icon";
import {
	Sheet,
	SheetContent,
	SheetHeader,
	SheetTitle,
} from "../components/ui/sheet";
import { AppRuntimeProvider } from "../offline/appRuntime";
import { DEMO_SCENARIOS } from "./fixtures";
import {
	getDemoSnapshot,
	resetDemoModel,
	subscribeDemo,
	syncDemoControls,
} from "./runtime";
import { type DemoControls, demoControlSearch } from "./shareState";

function readPanelPreference(): boolean {
	try {
		return localStorage.getItem("xp_demo_inspector_open") === "true";
	} catch {
		return false;
	}
}

function useViewport() {
	const [width, setWidth] = useState(() =>
		typeof window === "undefined" ? 1024 : window.innerWidth,
	);
	useEffect(() => {
		if (typeof window === "undefined") return;
		const resize = () => setWidth(window.innerWidth);
		window.addEventListener("resize", resize);
		return () => window.removeEventListener("resize", resize);
	}, []);
	return width;
}

export function DemoFrame({ children }: { children: ReactNode }) {
	const [snapshot, setSnapshot] = useState(getDemoSnapshot);
	useEffect(() => subscribeDemo(() => setSnapshot(getDemoSnapshot())), []);
	const router = useRouter();
	const queryClient = useQueryClient();
	const search = useRouterState({
		select: (state) => state.location.searchStr,
	});
	const previous = useRef(snapshot);
	const [open, setOpenState] = useState(readPanelPreference);
	const [placement, setPlacement] = useState<"auto" | "floating">("auto");
	const width = useViewport();
	const mobile = width < 768;
	const docked = open && width >= 1800 && placement === "auto";
	const setOpen = (value: boolean) => {
		setOpenState(value);
		try {
			localStorage.setItem("xp_demo_inspector_open", String(value));
		} catch {
			/* Optional preference. */
		}
	};
	useEffect(() => {
		syncDemoControls(search);
	}, [search]);
	useEffect(() => {
		if (previous.current.generation === snapshot.generation) return;
		const before = previous.current;
		previous.current = snapshot;
		void router.invalidate();
		if (
			snapshot.controls.scene !== before.controls.scene ||
			snapshot.controls.profile !== before.controls.profile ||
			snapshot.controls.persona !== before.controls.persona ||
			snapshot.operations.length === 0
		) {
			void queryClient.resetQueries();
		} else if (snapshot.controls.network !== "offline")
			void queryClient.invalidateQueries();
	}, [snapshot, router, queryClient]);
	const update = (patch: Partial<DemoControls>) => {
		const controls = { ...snapshot.controls, ...patch };
		void router.navigate({
			search: (current) =>
				({ ...current, ...demoControlSearch(controls) }) as never,
			replace: true,
		});
	};
	const controls = (
		<DemoInspectorControls
			controls={snapshot.controls}
			operations={snapshot.operations}
			shareUrl={window.location.href}
			onChange={update}
			onReset={resetDemoModel}
			onClose={() => setOpen(false)}
			onWalkthrough={() => {
				void router.navigate({
					to: "/users/$userId",
					params: { userId: "user-fixture-c" },
					search: {
						...demoControlSearch({
							...snapshot.controls,
							scene: "normal",
							persona: "admin",
							profile: "valid",
						}),
						d_format: "mihomo",
						d_mirror: "mirror",
						d_preview: "preview",
					},
				});
				setOpen(false);
			}}
		/>
	);
	return (
		<div
			className={
				docked ? "grid min-h-dvh grid-cols-[20rem_minmax(0,1fr)]" : "min-h-dvh"
			}
			data-demo-runtime="mock-only"
		>
			{docked && (
				<aside
					aria-label="Demo Inspector"
					className="sticky top-0 h-dvh overflow-y-auto border-r bg-background p-4"
				>
					{controls}
					<Button variant="ghost" onClick={() => setPlacement("floating")}>
						Float panel
					</Button>
				</aside>
			)}
			<div className="min-w-0">
				<AppRuntimeProvider
					onlineOverride={snapshot.controls.network !== "offline"}
				>
					<div
						key={`${snapshot.controls.scene}-${snapshot.controls.persona}-${snapshot.controls.profile}`}
					>
						{children}
					</div>
				</AppRuntimeProvider>
			</div>
			{(!open || (!docked && !mobile)) && (
				<Button
					aria-label={open ? "Close Demo Inspector" : "Open Demo Inspector"}
					className="fixed bottom-4 right-4 z-40 shadow-lg"
					onClick={() => setOpen(!open)}
				>
					<Icon name="tabler:adjustments" />
					Demo · Simulated
				</Button>
			)}
			{open && !docked && !mobile && (
				<aside
					aria-label="Demo Inspector"
					className={[
						"fixed right-4 bottom-20 z-40",
						"max-h-[calc(100dvh-6rem)] w-80 overflow-y-auto",
						"rounded-2xl border bg-background p-4 shadow-xl",
					].join(" ")}
				>
					{controls}
					{width >= 1800 && (
						<Button variant="ghost" onClick={() => setPlacement("auto")}>
							Dock panel
						</Button>
					)}
				</aside>
			)}
			{mobile && (
				<Sheet open={open} onOpenChange={setOpen}>
					<SheetContent side="bottom" className="max-h-[85dvh] overflow-y-auto">
						<SheetHeader>
							<SheetTitle className="sr-only">Demo Inspector</SheetTitle>
						</SheetHeader>
						{controls}
					</SheetContent>
				</Sheet>
			)}
		</div>
	);
}

export type DemoInspectorControlsProps = {
	controls: DemoControls;
	operations: { id: number; method: string; route: string; status: number }[];
	shareUrl: string;
	onChange: (patch: Partial<DemoControls>) => void;
	onReset: () => void;
	onClose: () => void;
	onWalkthrough: () => void;
};

export function DemoInspectorControls(props: DemoInspectorControlsProps) {
	return (
		<div className="space-y-5">
			<div className="flex items-start justify-between gap-3">
				<div>
					<h2 className="font-semibold">Demo Inspector</h2>
					<p className="text-xs text-muted-foreground">
						All actions are simulated. No real services.
					</p>
				</div>
				<Button
					variant="ghost"
					size="sm"
					aria-label="Collapse Inspector"
					onClick={props.onClose}
				>
					<Icon name="tabler:x" />
				</Button>
			</div>
			<label className="block space-y-1 text-sm">
				<span>Scene</span>
				<select
					className="w-full rounded-lg border bg-background p-2"
					value={props.controls.scene}
					onChange={(event) =>
						props.onChange({
							scene: event.target.value as DemoControls["scene"],
						})
					}
				>
					{DEMO_SCENARIOS.map((scene) => (
						<option key={scene.id} value={scene.id}>
							{scene.name}
						</option>
					))}
				</select>
			</label>
			<label className="block space-y-1 text-sm">
				<span>Persona / authentication</span>
				<select
					className="w-full rounded-lg border bg-background p-2"
					value={props.controls.persona}
					onChange={(event) =>
						props.onChange({
							persona: event.target.value as DemoControls["persona"],
						})
					}
				>
					<option value="admin">Simulated administrator</option>
					<option value="anonymous">Unauthenticated</option>
				</select>
			</label>
			<label className="block space-y-1 text-sm">
				<span>Network</span>
				<select
					className="w-full rounded-lg border bg-background p-2"
					value={props.controls.network}
					onChange={(event) =>
						props.onChange({
							network: event.target.value as DemoControls["network"],
						})
					}
				>
					<option value="online">Online</option>
					<option value="slow">Slow · 1.5 seconds</option>
					<option value="offline">Offline / read-only</option>
					<option value="error">Backend error · 503</option>
				</select>
			</label>
			<label className="block space-y-1 text-sm">
				<span>Data · Mihomo profile</span>
				<select
					className="w-full rounded-lg border bg-background p-2"
					value={props.controls.profile}
					onChange={(event) =>
						props.onChange({
							profile: event.target.value as DemoControls["profile"],
						})
					}
				>
					<option value="valid">Valid HTTPS resources</option>
					<option value="missing">Missing profile</option>
					<option value="invalid">Invalid HTTP resource</option>
				</select>
			</label>
			<section className="space-y-2">
				<h3 className="text-sm font-medium">Simulated actions</h3>
				<Button
					variant="secondary"
					className="w-full"
					onClick={props.onWalkthrough}
				>
					Mihomo Mirror walkthrough
				</Button>
				<Button variant="ghost" className="w-full" onClick={props.onReset}>
					Reset in-memory changes
				</Button>
			</section>
			<section className="space-y-2">
				<h3 className="text-sm font-medium">Share this state</h3>
				<CopyButton text={props.shareUrl} label="Copy demo link" />
				<p className="text-xs text-muted-foreground">
					Links preserve controls and the page. Edited data resets on refresh.
				</p>
			</section>
			<section className="space-y-2">
				<h3 className="text-sm font-medium">Recent simulated operations</h3>
				{props.operations.length === 0 ? (
					<p className="text-xs text-muted-foreground">
						No simulated writes yet.
					</p>
				) : (
					<ol className="space-y-2 text-xs">
						{props.operations.map((operation) => (
							<li key={operation.id} className="break-all font-mono">
								{operation.method} {operation.route} · {operation.status}
							</li>
						))}
					</ol>
				)}
			</section>
		</div>
	);
}
