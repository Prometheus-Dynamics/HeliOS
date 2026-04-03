import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import ts from 'typescript';

const MARKUP_SKIP_RE = /<(script|style)\b[^>]*>[\s\S]*?<\/\1>/gi;

function transpileExpression(expression) {
	const wrapped = `const __svelte_expr__ = (${expression});`;
	const result = ts.transpileModule(wrapped, {
		compilerOptions: {
			module: ts.ModuleKind.ESNext,
			target: ts.ScriptTarget.ESNext,
			verbatimModuleSyntax: true,
			preserveValueImports: true
		}
	});

	const output = result.outputText.replace(/^["']use strict["'];\s*/u, '');
	const match =
		output.match(/^const __svelte_expr__ = \(([\s\S]*)\);\s*$/) ??
		output.match(/^const __svelte_expr__ = ([\s\S]*);\s*$/);

	return match ? match[1] : expression;
}

function transpileConstExpression(expression) {
	const wrapped = `const ${expression};`;
	const result = ts.transpileModule(wrapped, {
		compilerOptions: {
			module: ts.ModuleKind.ESNext,
			target: ts.ScriptTarget.ESNext,
			verbatimModuleSyntax: true,
			preserveValueImports: true
		}
	});

	const output = result.outputText.replace(/^["']use strict["'];\s*/u, '');
	const match = output.match(/^const ([\s\S]*);\s*$/);
	return match ? match[1] : expression;
}

function readSvelteExpression(source, start) {
	let depth = 1;
	let inString = null;
	let escaped = false;

	for (let index = start + 1; index < source.length; index += 1) {
		const char = source[index];
		if (inString) {
			if (escaped) {
				escaped = false;
				continue;
			}
			if (char === '\\') {
				escaped = true;
				continue;
			}
			if (char === inString) {
				inString = null;
			}
			continue;
		}

		if (char === "'" || char === '"' || char === '`') {
			inString = char;
			continue;
		}

		if (char === '{') {
			depth += 1;
			continue;
		}

		if (char === '}') {
			depth -= 1;
			if (depth === 0) {
				return {
					end: index,
					body: source.slice(start + 1, index)
				};
			}
		}
	}

	return null;
}

function transformMarkupExpression(body) {
	if (body.startsWith('/')) {
		return body;
	}
	if (
		body.startsWith('...') ||
		body.startsWith('#key ') ||
		body.startsWith('#snippet ') ||
		body.startsWith('#each ') ||
		body.startsWith('#await ') ||
		body.startsWith(':then ') ||
		body.startsWith(':catch ') ||
		body.startsWith(':else') ||
		body.startsWith('@render ') ||
		body.startsWith('@html ') ||
		body.startsWith('@attach ') ||
		body.startsWith('@debug ')
	) {
		return body;
	}
	if (body.startsWith('@const ')) {
		return `@const ${transpileConstExpression(body.slice(7))}`;
	}
	if (body.startsWith('#if ')) {
		return `#if ${transpileExpression(body.slice(4))}`;
	}
	if (body.startsWith(':else if ')) {
		return `:else if ${transpileExpression(body.slice(9))}`;
	}
	return transpileExpression(body);
}

function transformMarkupSegment(segment) {
	let result = '';
	let index = 0;

	while (index < segment.length) {
		if (segment[index] !== '{') {
			result += segment[index];
			index += 1;
			continue;
		}

		const expression = readSvelteExpression(segment, index);
		if (!expression) {
			result += segment[index];
			index += 1;
			continue;
		}

		result += `{${transformMarkupExpression(expression.body)}}`;
		index = expression.end + 1;
	}

	return result;
}

const markupPreprocess = {
	markup({ content }) {
		let result = '';
		let lastIndex = 0;
		let match;

		while ((match = MARKUP_SKIP_RE.exec(content)) !== null) {
			result += transformMarkupSegment(content.slice(lastIndex, match.index));
			result += match[0];
			lastIndex = match.index + match[0].length;
		}

		result += transformMarkupSegment(content.slice(lastIndex));
		return { code: result };
	}
};

const scriptPreprocess = {
	script({ attributes, content, filename = '' }) {
		if (attributes.lang !== 'ts') {
			return undefined;
		}

		const result = ts.transpileModule(content, {
			fileName: filename,
			compilerOptions: {
				module: ts.ModuleKind.ESNext,
				target: ts.ScriptTarget.ESNext,
				sourceMap: true,
				verbatimModuleSyntax: true,
				preserveValueImports: true
			}
		});

		return {
			code: `\n${result.outputText}`,
			map: result.sourceMapText ? JSON.parse(result.sourceMapText) : undefined,
			attributes: Object.fromEntries(Object.entries(attributes).filter(([name]) => name !== 'lang'))
		};
	}
};

/** @type {import('@sveltejs/kit').Config} */
const config = {
	// Consult https://svelte.dev/docs/kit/integrations
	// for more information about preprocessors
	preprocess: [markupPreprocess, scriptPreprocess, vitePreprocess()],
	compilerOptions: {
		runes: true
	},
	kit: {
		adapter: adapter({
			fallback: 'index.html'
		}),
		version: {
			pollInterval: 15000
		},
		prerender: {
			entries: ['*']
		}
	}
};

export default config;
