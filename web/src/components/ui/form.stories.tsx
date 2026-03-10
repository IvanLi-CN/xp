import { zodResolver } from "@hookform/resolvers/zod";
import type { Meta, StoryObj } from "@storybook/react";
import { useEffect } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";

import { Button } from "./button";
import { Checkbox } from "./checkbox";
import {
	Form,
	FormControl,
	FormDescription,
	FormField,
	FormItem,
	FormLabel,
	FormMessage,
} from "./form";
import { Input } from "./input";

const formSchema = z.object({
	name: z.string().min(2, "Name must be at least 2 characters."),
	autoUpdate: z.boolean(),
});

type DemoValues = z.infer<typeof formSchema>;

function DemoForm(props: { showErrors?: boolean }) {
	const form = useForm<DemoValues>({
		resolver: zodResolver(formSchema),
		defaultValues: {
			name: props.showErrors ? "" : "Tokyo",
			autoUpdate: true,
		},
	});

	useEffect(() => {
		if (props.showErrors) {
			void form.trigger();
		}
	}, [form, props.showErrors]);

	return (
		<Form {...form}>
			<form
				className="w-[360px] space-y-4"
				onSubmit={form.handleSubmit(() => undefined)}
			>
				<FormField
					control={form.control}
					name="name"
					render={({ field }) => (
						<FormItem>
							<FormLabel>Node name</FormLabel>
							<FormControl>
								<Input placeholder="Tokyo" {...field} />
							</FormControl>
							<FormDescription>
								Shown in runtime and quota surfaces.
							</FormDescription>
							<FormMessage />
						</FormItem>
					)}
				/>
				<FormField
					control={form.control}
					name="autoUpdate"
					render={({ field }) => (
						<FormItem className="rounded-2xl border border-border/70 p-4">
							<div className="flex items-start gap-3">
								<FormControl>
									<Checkbox
										checked={field.value}
										onCheckedChange={(next) => field.onChange(next === true)}
									/>
								</FormControl>
								<div className="space-y-1">
									<FormLabel>Automatic updates</FormLabel>
									<FormDescription>
										Keep managed geo databases in sync.
									</FormDescription>
								</div>
							</div>
						</FormItem>
					)}
				/>
				<div className="flex justify-end gap-2">
					<Button variant="outline" type="button">
						Reset
					</Button>
					<Button type="submit">Save</Button>
				</div>
			</form>
		</Form>
	);
}

const meta = {
	title: "UI/Form",
	component: DemoForm,
	tags: ["autodocs", "coverage-ui"],
	parameters: {
		layout: "centered",
		docs: {
			description: {
				component:
					"Form composition helpers that wire labels, descriptions, and messages to controls. The stories demonstrate the RHF + Zod stack used by migrated admin forms, including an error-first edge state.",
			},
		},
	},
} satisfies Meta<typeof DemoForm>;

export default meta;

type Story = StoryObj<typeof meta>;

export const ValidState: Story = {
	render: () => <DemoForm />,
};

export const InvalidState: Story = {
	render: () => <DemoForm showErrors />,
};
