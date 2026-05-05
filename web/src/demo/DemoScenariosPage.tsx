import { Link, useNavigate } from "@tanstack/react-router";

import { Badge } from "@/components/ui/badge";

import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { DEMO_SCENARIOS } from "./fixtures";
import { useDemo } from "./store";

const scripts = [
	{
		title: "普通管理员路径",
		scenarioId: "normal" as const,
		steps: [
			"登录为 Admin。",
			"查看 Dashboard 的节点和 quota 状态。",
			"创建 endpoint，进入详情页运行 probe。",
			"创建 user，分配 endpoint，复制 subscription URL。",
		],
		startTo: "/demo/endpoints/new" as const,
	},
	{
		title: "异常路径",
		scenarioId: "incident" as const,
		steps: [
			"切换 Partial outage seed。",
			"检查 Dashboard alert 和 degraded/offline node。",
			"进入 endpoint 详情运行 probe，观察 warning/error toast。",
			"回到 Nodes 查看受影响节点。",
		],
		startTo: "/demo" as const,
	},
	{
		title: "只读审计路径",
		scenarioId: "large" as const,
		steps: [
			"登录为 Viewer。",
			"在 Users 使用 search/filter/sort/pagination。",
			"打开用户详情，确认创建、删除和保存操作被禁用。",
			"复制 subscription URL 做交付检查。",
		],
		startTo: "/demo/users" as const,
	},
	{
		title: "空数据路径",
		scenarioId: "empty" as const,
		steps: [
			"切换 Fresh install seed。",
			"在 Endpoints 和 Users 页面查看 empty state。",
			"创建第一个 endpoint。",
			"创建第一个 user 并分配 endpoint。",
		],
		startTo: "/demo/endpoints" as const,
	},
];

export function DemoScenariosPage() {
	const navigate = useNavigate();
	const { state, resetScenario } = useDemo();

	return (
		<div className="space-y-6">
			<PageHeader
				title="Demo scripts"
				description="Reproducible paths for review, failure handling, admin work, and empty data."
				meta={<Badge variant="ghost">current: {state.scenarioId}</Badge>}
			/>

			<div className="grid gap-4 md:grid-cols-2">
				{DEMO_SCENARIOS.map((scenario) => (
					<section key={scenario.id} className="xp-card">
						<div className="xp-card-body">
							<div className="flex items-start justify-between gap-3">
								<div>
									<h2 className="xp-card-title">{scenario.name}</h2>
									<p className="mt-1 text-sm text-muted-foreground">
										{scenario.description}
									</p>
								</div>
								{scenario.id === state.scenarioId ? (
									<Badge variant="success">loaded</Badge>
								) : null}
							</div>
							<p className="text-sm">{scenario.intent}</p>
							<Button
								variant="secondary"
								onClick={() => {
									resetScenario(scenario.id);
									navigate({ to: "/demo" });
								}}
							>
								Load seed
							</Button>
						</div>
					</section>
				))}
			</div>

			<section className="xp-card">
				<div className="xp-card-body">
					<h2 className="xp-card-title">Playback paths</h2>
					<div className="grid gap-4 lg:grid-cols-2">
						{scripts.map((script) => (
							<div
								key={script.title}
								className="rounded-2xl border border-border/70 bg-muted/35 p-4"
							>
								<div className="flex items-start justify-between gap-3">
									<h3 className="font-semibold">{script.title}</h3>
									<Badge variant="ghost">{script.scenarioId}</Badge>
								</div>
								<ol className="mt-3 list-decimal space-y-2 pl-5 text-sm text-muted-foreground">
									{script.steps.map((step) => (
										<li key={step}>{step}</li>
									))}
								</ol>
								<div className="mt-4 flex flex-wrap gap-2">
									<Button
										variant="secondary"
										onClick={() => {
											resetScenario(script.scenarioId);
											navigate({ to: script.startTo });
										}}
									>
										Load and start
									</Button>
									<Link
										className="xp-link self-center text-sm"
										to={script.startTo}
									>
										Open without reset
									</Link>
								</div>
							</div>
						))}
					</div>
				</div>
			</section>
		</div>
	);
}
