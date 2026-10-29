import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	// Matches python, python3, python3.12 and `uv run python` as a command word.
	const python = /(^|[;&|`(\s])(uv\s+run\s+)?python[0-9.]*(\s|$)/;

	pi.on("tool_call", async (event) => {
		if (event.toolName !== "bash") return undefined;
		const command = String(event.input.command ?? "");
		if (!python.test(command)) return undefined;

		return {
			block: true,
			reason:
				"Python is not allowed in this project. Don't script file changes with it. " +
				"Use the edit tool for precise changes to existing files, or write for new files.",
		};
	});
}
