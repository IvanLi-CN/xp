export function inlineBootstrapFallback(buildId: string): string {
	const shellStyle = [
		"min-height:100vh",
		"box-sizing:border-box",
		"display:grid",
		"place-items:center",
		"padding:24px",
		"background:#f5fbfc",
		"color:#16313b",
		"font:16px/1.5 system-ui,sans-serif",
	].join(";");
	const articleStyle = [
		"width:min(100%,560px)",
		"box-sizing:border-box",
		"padding:32px",
		"border:1px solid #c7dfe4",
		"border-radius:20px",
		"background:#fff",
	].join(";");
	const buttonStyle = [
		"min-height:44px",
		"border:0",
		"border-radius:10px",
		"padding:0 18px",
		"background:#079bb5",
		"color:#fff",
		"font:inherit",
		"font-weight:700",
		"cursor:pointer",
	].join(";");
	const markup = [
		`<main data-xp-document-fallback style="${shellStyle}">`,
		`<article role="alert" style="${articleStyle}">`,
		'<p style="margin:0 0 8px;color:#087f94;font-weight:700">xp</p>',
		'<h1 style="margin:0 0 12px;font-size:28px">xp could not start</h1>',
		'<p style="margin:0 0 20px;color:#4b6871">',
		"The application files could not be loaded. Reload after checking your connection.",
		"</p>",
		`<button type="button" data-action="reload" style="${buttonStyle}">`,
		"Reload app</button>",
		'<details style="margin-top:24px;color:#4b6871;font-size:13px">',
		"<summary>Technical details</summary>",
		'<pre style="white-space:pre-wrap;overflow-wrap:anywhere">',
		`build: ${buildId}\\nerror: entry resource failed to load</pre>`,
		"</details></article></main>",
	].join("");
	const render = [
		"if(shown)return;",
		'const root=document.getElementById("root")||document.body;',
		'if(root?.dataset.xpReactReady==="true")return;',
		"if(!root){",
		'window.addEventListener("DOMContentLoaded",render,{once:true});',
		"return}",
		"shown=true;",
		`root.innerHTML=${JSON.stringify(markup)};`,
		'root.querySelector("[data-action=reload]")',
		'?.addEventListener("click",()=>location.reload())',
	].join("");
	const errorHandler = [
		"const target=event.target;",
		"if(event.error||event.message||target===window||target instanceof HTMLScriptElement",
		'||(target instanceof HTMLLinkElement&&target.rel==="stylesheet"))render()',
	].join("");
	return [
		"(()=>{let shown=false;",
		`const render=()=>{${render}};`,
		'window.addEventListener("error",event=>{',
		errorHandler,
		"},true)})();",
	].join("");
}
