import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import globals from 'globals';
import ts from 'typescript-eslint';
import svelteConfig from './svelte.config.js';

export default [
  { ignores: ['src/lib/ts-bindings/**'] },
  ...ts.config(
    js.configs.recommended,
    ...ts.configs.recommended,
    ...svelte.configs.recommended,
    {
      languageOptions: {
        globals: {
          ...globals.browser,
          ...globals.node
        }
      },
      settings: {
        svelte: {
          ignoreWarnings: ['/^ts\\d+/'],
          compilerOptions: {
            runes: true
          }
        }
      },
      linterOptions: {
        reportUnusedDisableDirectives: 0
      },
      rules: {
        'svelte/valid-compile': 'error'
      }
    },
    {
      files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
      languageOptions: {
        parserOptions: {
          project: ['./tsconfig.eslint.json'],
          extraFileExtensions: ['.svelte'],
          parser: ts.parser,
          svelteConfig
        }
      }
    }
  )
];
