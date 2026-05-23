import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';

/** @type {import('eslint').Linter.Config[]} */
export default [
	js.configs.recommended,
	...ts.configs.recommended,
	...svelte.configs['flat/recommended'],
	{
		languageOptions: {
			globals: {
				...globals.browser,
				...globals.node,
			},
		},
	},
	{
		files: ['**/*.svelte'],
		languageOptions: {
			parserOptions: {
				parser: ts.parser,
			},
		},
		rules: {
			'svelte/no-navigation-without-resolve': 'off',
		},
	},
	{
		files: ['**/*.svelte.ts'],
		languageOptions: {
			parser: ts.parser,
			globals: {
				'$state': 'readonly',
				'$derived': 'readonly',
				'$effect': 'readonly',
				'$props': 'readonly',
			},
		},
		rules: {
			'prefer-const': 'off',
		},
	},
	{
		ignores: ['dist/', '.svelte-kit/', 'node_modules/'],
	},
];
