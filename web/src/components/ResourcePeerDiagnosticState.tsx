import { BackendApiError } from "../api/backendError";
import { parseResourcePeerDiagnostic } from "../utils/resourcePeerDiagnostic";
import { CopyButton } from "./CopyButton";
import { QueryErrorState } from "./QueryErrorState";
import { QueryRetryAction } from "./QueryRetryAction";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "./ui/card";

type ResourcePeerDiagnosticStateProps = {
	error: unknown;
	isFetching: boolean;
	isOnline: boolean;
	onRetry: () => void;
};

const failureLabels: Record<string, string> = {
	circuit_open: "断路器已打开",
	pre_response_timeout: "签名响应前超时",
	pre_response_transport: "签名响应前传输失败",
	unsigned_response: "响应缺少可验证签名",
	acknowledgement_missing: "缺少签名确认",
	acknowledgement_invalid: "签名确认无效",
	outcome_unknown: "请求结果未知",
};

const routeLabels: Record<string, string> = {
	direct_mesh: "Direct Mesh",
	reverse_relay: "Reverse relay",
	public: "Public",
};

function boundaryText(
	failure: string,
	originName: string,
	route?: string,
): string {
	switch (failure) {
		case "circuit_open":
			if (route === "direct_mesh") {
				return `${originName} 的 Direct Mesh 断路器当前处于冷却中，本次请求未通过 Direct Mesh。`;
			}
			if (route === "reverse_relay") {
				return "Reverse relay 路径当前不可用，本次请求未通过该路径。";
			}
			return [
				`${originName} 的 Public 备用路径当前处于冷却中，`,
				"本次 Public 请求未发送。",
			].join("");
		case "pre_response_timeout":
			return [
				`${originName} 未在请求预算内收到签名响应，`,
				"无法确认请求是否到达目标节点。",
			].join("");
		case "pre_response_transport":
			return [
				`${originName} 在收到签名响应前发生传输失败，`,
				"无法判断故障位于本地、网络、Tunnel 或目标 XP。",
			].join("");
		case "unsigned_response":
			return [
				"收到了响应，但没有可验证的签名确认；",
				"不能据此判断目标 XP 是否完成请求。",
			].join("");
		case "acknowledgement_missing":
			return [
				"收到了响应，但响应缺少签名确认；",
				"不能据此判断目标 XP 是否完成请求。",
			].join("");
		case "acknowledgement_invalid":
			return [
				"收到了响应，但签名确认无法验证；",
				"不能据此判断目标 XP 是否完成请求。",
			].join("");
		default:
			return [
				`${originName} 未获得可确认的结果，不能把故障归因于目标 XP、`,
				"Tunnel 或 Cloudflare。",
			].join("");
	}
}

function circuitLabel(state: string): string {
	return (
		{
			closed: "正常（未打开）",
			open: "冷却中",
			half_open: "探测中",
			disabled: "未启用",
		}[state] ?? "未知"
	);
}

function acknowledgementLabel(state: string): string {
	return (
		{
			not_observed: "未观察到",
			missing: "缺少",
			invalid: "无效",
		}[state] ?? "未知"
	);
}

export function ResourcePeerDiagnosticState({
	error,
	isFetching,
	isOnline,
	onRetry,
}: ResourcePeerDiagnosticStateProps) {
	const diagnostic = parseResourcePeerDiagnostic(error);
	if (!diagnostic) {
		return (
			<QueryErrorState
				title="Failed to load resources"
				description={
					error instanceof BackendApiError
						? `${error.status}: ${error.message}`
						: "The resource snapshot could not be loaded."
				}
				error={error}
				loading={isFetching}
				disabled={!isOnline}
				onRetry={onRetry}
			/>
		);
	}

	const lastAttempt = diagnostic.route_attempts.at(-1);
	const publicWasNotSent = diagnostic.route_attempts.some(
		(attempt) =>
			attempt.route === "public" && attempt.failure === "circuit_open",
	);

	return (
		<Card>
			<CardHeader className="gap-2">
				<CardTitle className="text-destructive">
					Failed to load resources
				</CardTitle>
				<CardDescription>
					{diagnostic.origin.node_name} 无法从 {diagnostic.target.node_name}{" "}
					获取可验证的资源快照。
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-4">
				<div className="grid gap-3 text-sm sm:grid-cols-2">
					<div className="min-w-0">
						<div className="text-muted-foreground">来源 → 目标</div>
						<div className="break-words font-medium">
							{diagnostic.origin.node_name} → {diagnostic.target.node_name}
						</div>
					</div>
					<div>
						<div className="text-muted-foreground">
							{diagnostic.origin.node_name} Public 断路器
						</div>
						<div className="font-medium">
							{circuitLabel(diagnostic.public_circuit)}
						</div>
					</div>
				</div>

				<div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
					<div className="font-medium">
						{boundaryText(
							lastAttempt?.failure ?? "outcome_unknown",
							diagnostic.origin.node_name,
							lastAttempt?.route,
						)}
					</div>
					{publicWasNotSent ? (
						<div className="mt-1 text-muted-foreground">
							Public 备用路径未发送；这条记录不能证明目标节点或 Cloudflare
							故障。
						</div>
					) : null}
				</div>

				<div>
					<div className="mb-2 text-sm font-medium">本次路径追踪</div>
					{diagnostic.route_attempts.length > 0 ? (
						<ol className="space-y-2 text-sm">
							{diagnostic.route_attempts.map((attempt, index) => (
								<li
									className={[
										"grid min-w-0 gap-1 rounded-md border border-border/60 p-2",
										"sm:grid-cols-[auto_1fr_auto] sm:items-center",
									].join(" ")}
									key={`${attempt.request_id}-${index}`}
								>
									<span className="font-medium">
										{index + 1}. {routeLabels[attempt.route]}
									</span>
									<span className="min-w-0 break-words text-muted-foreground">
										{failureLabels[attempt.failure] ?? "未知失败"} · 确认：
										{acknowledgementLabel(attempt.acknowledgement)}
									</span>
									<span className="break-all text-xs text-muted-foreground">
										{attempt.elapsed_ms} ms · {attempt.observed_at}
									</span>
								</li>
							))}
						</ol>
					) : (
						<p className="text-sm text-muted-foreground">
							没有可公开的路径尝试记录。
						</p>
					)}
				</div>
				{diagnostic.last_public_failure ? (
					<div className="rounded-md border border-border/60 p-3 text-sm">
						<div className="font-medium">最近一次 Public 故障</div>
						<div className="mt-1 break-words text-muted-foreground">
							{failureLabels[diagnostic.last_public_failure.failure] ??
								"未知失败"}{" "}
							· 确认：
							{acknowledgementLabel(
								diagnostic.last_public_failure.acknowledgement,
							)}{" "}
							· {diagnostic.last_public_failure.observed_at}
						</div>
					</div>
				) : null}

				<div className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-3">
					<div className="min-w-0 flex-1 text-sm">
						<div className="text-muted-foreground">关联 ID</div>
						<div className="break-all font-mono text-xs">
							{diagnostic.request_id}
						</div>
					</div>
					<CopyButton
						text={diagnostic.request_id}
						label="复制 ID"
						copiedLabel="已复制"
						errorLabel="复制失败"
						size="sm"
					/>
					<QueryRetryAction
						loading={isFetching}
						disabled={!isOnline}
						onRetry={onRetry}
					/>
				</div>
			</CardContent>
		</Card>
	);
}
