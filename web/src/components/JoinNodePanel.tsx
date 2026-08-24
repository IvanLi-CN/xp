import { cn } from "@/lib/utils";
import { highlightShell } from "../utils/highlightShell";
import { Button } from "./Button";
import { CopyButton } from "./CopyButton";
import { useUiPrefs } from "./UiPrefs";
import { inputClass as inputControlClass } from "./ui-helpers";
import { Input } from "./ui/input";

export function JoinNodePanel(props: {
	ttlSeconds: number;
	onTtlSecondsChange: (value: number) => void;
	isCreatingJoinToken: boolean;
	canCreateToken: boolean;
	onCreateJoinToken: () => void;
	joinTokenError: string | null;
	joinToken: string | null;
	joinCommand: string;
	deployCommand: string;
}) {
	const prefs = useUiPrefs();
	return (
		<section className="space-y-4">
			<div>
				<h2 className="text-lg font-semibold">Join token</h2>
				<p className="text-sm text-muted-foreground">
					Generate a token and share it with the node you want to join.
				</p>
			</div>
			<div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_auto] md:items-end">
				<div className="xp-field-stack">
					<span className="text-sm font-medium">TTL (seconds)</span>
					<Input
						aria-label="TTL (seconds)"
						type="number"
						min={60}
						step={60}
						className={inputControlClass(prefs.density, "font-mono")}
						value={props.ttlSeconds}
						onChange={(event) => {
							const next = Number(event.target.value);
							props.onTtlSecondsChange(Number.isFinite(next) ? next : 0);
						}}
					/>
				</div>
				<div className="flex md:justify-end">
					<Button
						variant="secondary"
						loading={props.isCreatingJoinToken}
						disabled={props.ttlSeconds <= 0 || !props.canCreateToken}
						onClick={props.onCreateJoinToken}
					>
						Create token
					</Button>
				</div>
			</div>
			{props.joinTokenError ? (
				<p className="font-mono text-sm text-destructive">
					{props.joinTokenError}
				</p>
			) : null}
			{props.joinToken ? (
				<div className="space-y-4 border-t border-border/70 pt-4">
					<div className="grid gap-4 lg:grid-cols-12">
						<div
							className={cn(
								"space-y-3 border-b border-border/70 pb-4 lg:col-span-6",
								"lg:border-b-0 lg:border-r lg:pb-0 lg:pr-4",
							)}
						>
							<div className="flex items-center justify-between gap-2">
								<p className="text-xs uppercase tracking-wide text-muted-foreground">
									Join token
								</p>
								<CopyButton
									text={props.joinToken}
									ariaLabel="Copy join token"
									iconOnly
									variant="ghost"
									size="sm"
								/>
							</div>
							<p className="break-all font-mono text-sm">{props.joinToken}</p>
						</div>

						<div className="space-y-3 border-b border-border/70 pb-4 lg:col-span-6 lg:border-b-0 lg:pb-0">
							<div className="flex items-center justify-between gap-2">
								<p className="text-xs uppercase tracking-wide text-muted-foreground">
									xp join command (legacy)
								</p>
								<CopyButton
									text={props.joinCommand}
									ariaLabel="Copy join command"
									iconOnly
									variant="ghost"
									size="sm"
								/>
							</div>
							<p className="break-all font-mono text-sm">{props.joinCommand}</p>
						</div>

						<div className="space-y-3 lg:col-span-12">
							<div className="space-y-1 min-w-0">
								<div className="flex items-center justify-between gap-2">
									<p className="text-xs uppercase tracking-wide text-muted-foreground">
										xp-ops deploy command (recommended)
									</p>
									<CopyButton
										text={props.deployCommand}
										ariaLabel="Copy deploy command"
										iconOnly
										variant="ghost"
										size="sm"
									/>
								</div>
								{props.deployCommand ? (
									<pre
										className={cn(
											"max-h-72 overflow-auto rounded-xl border border-border/60",
											"bg-muted/35 p-3 font-mono text-sm leading-5",
										)}
									>
										{highlightShell(props.deployCommand)}
									</pre>
								) : (
									<p className="text-sm text-muted-foreground">
										Loading cluster version...
									</p>
								)}
							</div>
						</div>
					</div>
					<div className="text-sm text-muted-foreground">
						<p>
							Notes: you can override <span className="font-mono">XP_REPO</span>
							, <span className="font-mono">NODE_NAME</span>,{" "}
							<span className="font-mono">ACCESS_HOST</span>, and{" "}
							<span className="font-mono">API_BASE_URL</span> before running the
							deploy command.
						</p>
					</div>
				</div>
			) : null}
		</section>
	);
}
