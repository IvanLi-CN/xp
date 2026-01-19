import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { fetchAdminConfig } from "../api/adminConfig";
import { isBackendApiError } from "../api/backendError";
import { fetchClusterInfo } from "../api/clusterInfo";
import { fetchHealth } from "../api/health";
import { Button } from "../components/Button";
import { CopyButton } from "../components/CopyButton";
import { PageHeader } from "../components/PageHeader";
import { PageState } from "../components/PageState";
import { useToast } from "../components/Toast";
import { readAdminToken } from "../components/auth";

function formatErrorMessage(error: unknown): string {
	if (isBackendApiError(error)) {
		const code = error.code ? ` ${error.code}` : "";
		return `${error.status}${code}: ${error.message}`;
	}
	return String(error);
}

function displayValue(value: string): string {
	return value.trim().length === 0 ? "(empty)" : value;
}

function formatTimestamp(value: number): string {
	if (!Number.isFinite(value) || value <= 0) return "(empty)";
	const date = new Date(value);
	const yyyy = String(date.getFullYear()).padStart(4, "0");
	const mm = String(date.getMonth() + 1).padStart(2, "0");
	const dd = String(date.getDate()).padStart(2, "0");
	const hh = String(date.getHours()).padStart(2, "0");
	const min = String(date.getMinutes()).padStart(2, "0");
	return `${yyyy}-${mm}-${dd} ${hh}:${min}`;
}

function shortId(value: string, prefixLength = 3): string {
	if (value.length <= prefixLength) return value;
	return `${value.slice(0, prefixLength)}...`;
}

type SummaryChipProps = {
	label: string;
	value: string;
	tone?: "neutral" | "warning";
};

function SummaryChip({ label, value, tone = "neutral" }: SummaryChipProps) {
	return (
		<div
			className={[
				"rounded-box border bg-base-100 px-4 py-3",
				tone === "warning" ? "border-warning/40" : "border-base-200",
			].join(" ")}
		>
			<div className="text-xs uppercase tracking-widest opacity-60">
				{label}
			</div>
			<div className="mt-1 font-semibold">{value}</div>
		</div>
	);
}

type FieldBlockProps = {
	label: string;
	value: string;
	copyText?: string;
};

function FieldBlock({ label, value, copyText }: FieldBlockProps) {
	const text = displayValue(value);

	return (
		<div className="rounded-box border border-base-200 bg-base-100 px-3 py-3">
			<div className="flex items-start justify-between gap-3">
				<div className="min-w-0 flex-1">
					<div className="text-xs uppercase tracking-widest opacity-60">
						{label}
					</div>
					<div
						className="mt-1 font-mono text-sm whitespace-nowrap overflow-x-auto"
						title={value}
					>
						{text}
					</div>
				</div>
				{copyText ? (
					<CopyButton
						text={copyText}
						label="copy"
						copiedLabel="copied"
						errorLabel="error"
						variant="secondary"
						size="sm"
						className="btn-xs btn-info uppercase tracking-wide"
					/>
				) : null}
			</div>
		</div>
	);
}

export function ServiceConfigPage() {
	const [adminToken] = useState(() => readAdminToken());
	const toast = useToast();

	const health = useQuery({
		queryKey: ["health"],
		queryFn: ({ signal }) => fetchHealth(signal),
	});

	const clusterInfo = useQuery({
		queryKey: ["clusterInfo"],
		queryFn: ({ signal }) => fetchClusterInfo(signal),
	});

	const configQuery = useQuery({
		queryKey: ["adminConfig", adminToken],
		enabled: adminToken.length > 0,
		queryFn: ({ signal }) => fetchAdminConfig(adminToken, signal),
	});

	const headerActions = (
		<>
			<Button
				variant="secondary"
				size="sm"
				disabled={!configQuery.data}
				onClick={async () => {
					if (!configQuery.data) return;
					const payload = JSON.stringify(configQuery.data, null, 2);
					try {
						await navigator.clipboard.writeText(payload);
						toast.pushToast({ variant: "success", message: "Copied JSON" });
					} catch {
						toast.pushToast({ variant: "error", message: "Copy failed" });
					}
				}}
			>
				Copy JSON
			</Button>
			<Button
				variant="primary"
				size="sm"
				loading={configQuery.isFetching}
				disabled={adminToken.length === 0}
				onClick={() => configQuery.refetch()}
			>
				Refresh
			</Button>
		</>
	);

	const content = (() => {
		if (adminToken.length === 0) {
			return (
				<PageState
					variant="empty"
					title="需要管理员 Token"
					description="请先在 Dashboard 页面设置 admin token，再查看服务配置。"
				/>
			);
		}

		if (configQuery.isLoading) {
			return (
				<PageState
					variant="loading"
					title="正在加载服务配置"
					description="正在从控制面拉取当前进程的生效配置。"
				/>
			);
		}

		if (configQuery.isError) {
			return (
				<PageState
					variant="error"
					title="加载失败"
					description={formatErrorMessage(configQuery.error)}
					action={
						<Button
							variant="secondary"
							loading={configQuery.isFetching}
							onClick={() => configQuery.refetch()}
						>
							Retry
						</Button>
					}
				/>
			);
		}

		const data = configQuery.data;
		if (!data) {
			return (
				<PageState
					variant="empty"
					title="没有可展示的数据"
					description="当前没有服务配置数据。"
				/>
			);
		}

		const healthOk = health.isSuccess && health.data?.status === "ok";
		const role = clusterInfo.isSuccess ? clusterInfo.data.role : null;
		const statusValue = `${healthOk ? "Healthy" : "Unhealthy"}${
			role ? ` · ${role}` : ""
		}`;
		const nodeValue = `${
			data.node_name.trim().length > 0 ? data.node_name : "(empty)"
		}${clusterInfo.isSuccess ? ` (${shortId(clusterInfo.data.node_id)})` : ""}`;

		return (
			<div className="space-y-4">
				<div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
					<SummaryChip label="status" value={statusValue} />
					<SummaryChip label="node" value={nodeValue} />
					<SummaryChip
						label="last refresh"
						value={formatTimestamp(configQuery.dataUpdatedAt)}
					/>
					<SummaryChip
						label="access host"
						value={displayValue(data.access_host)}
					/>
				</div>

				<div className="grid gap-4 lg:grid-cols-2">
					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-3 p-4">
							<div>
								<h2 className="text-base font-semibold">Network</h2>
								<p className="text-sm opacity-70">
									控制面监听与对外 API 地址。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="BIND"
									value={data.bind}
									copyText={data.bind}
								/>
								<FieldBlock
									label="XRAY API ADDR"
									value={data.xray_api_addr}
									copyText={data.xray_api_addr}
								/>
								<FieldBlock
									label="API BASE URL"
									value={data.api_base_url}
									copyText={data.api_base_url}
								/>
							</div>
						</div>
					</div>

					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-3 p-4">
							<div>
								<h2 className="text-base font-semibold">Node</h2>
								<p className="text-sm opacity-70">
									用于订阅与客户端连接的对外 host。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="NODE NAME"
									value={data.node_name}
									copyText={data.node_name}
								/>
								<FieldBlock
									label="ACCESS HOST"
									value={data.access_host}
									copyText={data.access_host}
								/>
								<FieldBlock
									label="DATA DIR"
									value={data.data_dir}
									copyText={data.data_dir}
								/>
							</div>
						</div>
					</div>

					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-3 p-4">
							<div>
								<h2 className="text-base font-semibold">Quota</h2>
								<p className="text-sm opacity-70">流量统计与自动解封策略。</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="POLL INTERVAL"
									value={`${data.quota_poll_interval_secs} sec`}
									copyText={`${data.quota_poll_interval_secs}`}
								/>
								<FieldBlock
									label="AUTO UNBAN"
									value={data.quota_auto_unban ? "true" : "false"}
									copyText={data.quota_auto_unban ? "true" : "false"}
								/>
							</div>
						</div>
					</div>

					<div className="card bg-base-100 shadow">
						<div className="card-body space-y-3 p-4">
							<div>
								<h2 className="text-base font-semibold">Security</h2>
								<p className="text-sm opacity-70">
									仅展示可公开信息，敏感字段全量脱敏。
								</p>
							</div>
							<div className="space-y-3">
								<FieldBlock
									label="ADMIN TOKEN"
									value={data.admin_token_masked}
									copyText={data.admin_token_masked}
								/>
								<FieldBlock
									label="ADMIN TOKEN PRESENT"
									value={data.admin_token_present ? "true" : "false"}
									copyText={data.admin_token_present ? "true" : "false"}
								/>
							</div>
						</div>
					</div>
				</div>

				<div className="flex items-center justify-between gap-3 text-xs opacity-60">
					<div>Settings / Service config</div>
					<div>All fields are read-only · JSON export logs hidden</div>
				</div>
			</div>
		);
	})();

	return (
		<div className="space-y-5">
			<PageHeader
				title="服务配置"
				description="只读展示当前进程配置与订阅入口，便于部署核对与排障。"
				actions={headerActions}
			/>
			{content}
		</div>
	);
}
