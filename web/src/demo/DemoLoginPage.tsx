import { useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { Button } from "../components/Button";
import { Icon } from "../components/Icon";
import { useToast } from "../components/Toast";
import { Input } from "../components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "../components/ui/select";
import { DEMO_SCENARIOS } from "./fixtures";
import { useDemo } from "./store";
import type { DemoRole, DemoScenarioId } from "./types";

export function DemoLoginPage() {
	const navigate = useNavigate();
	const { login } = useDemo();
	const { pushToast } = useToast();
	const [operatorName, setOperatorName] = useState("Demo Operator");
	const [role, setRole] = useState<DemoRole>("admin");
	const [scenarioId, setScenarioId] = useState<DemoScenarioId>("normal");
	const selectedScenario =
		DEMO_SCENARIOS.find((scenario) => scenario.id === scenarioId) ??
		DEMO_SCENARIOS[0];

	return (
		<div className="min-h-screen bg-muted/35 px-6 py-10">
			<div className="mx-auto grid min-h-[calc(100vh-5rem)] w-full max-w-6xl items-center gap-6 lg:grid-cols-[minmax(0,1fr)_26rem]">
				<section className="space-y-6">
					<div className="flex items-center gap-3">
						<img
							src="/xp-mark.png"
							alt=""
							aria-hidden="true"
							className="size-12 shrink-0"
						/>
						<div>
							<h1 className="text-3xl font-semibold tracking-tight">
								xp Demo Site
							</h1>
							<p className="mt-1 max-w-2xl text-sm text-muted-foreground">
								A high-fidelity mock control plane for walking through login,
								cluster review, endpoint creation, user assignment, and failure
								recovery.
							</p>
						</div>
					</div>

					<div className="grid gap-3 md:grid-cols-2">
						{DEMO_SCENARIOS.map((scenario) => (
							<button
								key={scenario.id}
								type="button"
								className={`rounded-2xl border px-4 py-4 text-left shadow-sm transition-colors ${
									scenario.id === scenarioId
										? "border-primary/35 bg-primary/10"
										: "border-border/70 bg-card hover:bg-muted/45"
								}`}
								onClick={() => setScenarioId(scenario.id)}
							>
								<div className="flex items-center justify-between gap-3">
									<h2 className="font-semibold">{scenario.name}</h2>
									{scenario.id === scenarioId ? (
										<Icon
											name="tabler:check"
											className="size-5 text-primary"
											ariaLabel="Selected"
										/>
									) : null}
								</div>
								<p className="mt-2 text-sm text-muted-foreground">
									{scenario.description}
								</p>
								<p className="mt-3 text-xs text-muted-foreground">
									{scenario.intent}
								</p>
							</button>
						))}
					</div>
				</section>

				<section className="xp-card">
					<div className="xp-card-body">
						<div>
							<p className="text-xs uppercase tracking-[0.18em] text-muted-foreground">
								Demo login
							</p>
							<h2 className="mt-2 text-xl font-semibold">
								Start from a reproducible seed
							</h2>
						</div>

						<div className="xp-field-stack">
							<label
								className="text-sm font-medium"
								htmlFor="demo-operator-name"
							>
								Operator name
							</label>
							<Input
								id="demo-operator-name"
								value={operatorName}
								onChange={(event) => setOperatorName(event.target.value)}
								placeholder="Demo Operator"
							/>
						</div>

						<div className="xp-field-stack">
							<label className="text-sm font-medium" htmlFor="demo-role">
								Role
							</label>
							<Select
								value={role}
								onValueChange={(value) => setRole(value as DemoRole)}
							>
								<SelectTrigger id="demo-role">
									<SelectValue />
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="admin">Admin</SelectItem>
									<SelectItem value="operator">Operator</SelectItem>
									<SelectItem value="viewer">Viewer</SelectItem>
								</SelectContent>
							</Select>
							<span className="text-xs text-muted-foreground">
								Viewer can inspect but cannot create or delete records.
							</span>
						</div>

						<div className="rounded-2xl border border-border/70 bg-muted/35 px-4 py-3">
							<p className="text-sm font-medium">{selectedScenario.name}</p>
							<p className="mt-1 text-sm text-muted-foreground">
								{selectedScenario.description}
							</p>
						</div>

						<Button
							className="w-full"
							onClick={() => {
								login({ role, operatorName, scenarioId });
								pushToast({
									variant: "success",
									message: `Loaded ${selectedScenario.name}.`,
								});
								navigate({ to: "/demo" });
							}}
						>
							Enter demo
						</Button>
					</div>
				</section>
			</div>
		</div>
	);
}
