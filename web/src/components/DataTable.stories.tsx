import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";

import { DataTable } from "./DataTable";
import { useUiPrefs } from "./UiPrefs";

function SetDensity({ density }: { density: "comfortable" | "compact" }) {
	const prefs = useUiPrefs();
	useEffect(() => {
		prefs.setDensity(density);
	}, [density, prefs]);
	return null;
}

const meta: Meta<typeof DataTable> = {
	title: "Components/DataTable",
	component: DataTable,
};

export default meta;

type Story = StoryObj<typeof DataTable>;

export const Comfortable: Story = {
	render: () => (
		<>
			<SetDensity density="comfortable" />
			<DataTable
				headers={[
					{ key: "id", label: "ID" },
					{ key: "name", label: "Name" },
					{ key: "status", label: "Status", align: "right" },
				]}
				caption={<span>3 items</span>}
			>
				<tr>
					<td className="font-mono text-xs">node-1</td>
					<td>alpha</td>
					<td className="text-right">
						<span className="badge badge-success badge-sm">ready</span>
					</td>
				</tr>
				<tr>
					<td className="font-mono text-xs">node-2</td>
					<td>beta</td>
					<td className="text-right">
						<span className="badge badge-warning badge-sm">degraded</span>
					</td>
				</tr>
			</DataTable>
		</>
	),
};

export const Compact: Story = {
	render: () => (
		<>
			<SetDensity density="compact" />
			<DataTable
				headers={[
					{ key: "id", label: "ID" },
					{ key: "name", label: "Name" },
					{ key: "status", label: "Status", align: "right" },
				]}
			>
				<tr>
					<td className="font-mono text-xs">node-1</td>
					<td>alpha</td>
					<td className="text-right">
						<span className="badge badge-success badge-sm">ready</span>
					</td>
				</tr>
			</DataTable>
		</>
	),
};
