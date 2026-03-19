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
      files: ['src/lib/api/**/*.ts', 'src/lib/api/**/*.svelte.ts'],
      ignores: ['src/lib/api/core/http.ts', 'src/lib/api/core/ws.ts', 'src/lib/api/requestUtils.ts'],
      rules: {
        'no-restricted-globals': [
          'error',
          {
            name: 'fetch',
            message: 'Use $lib/api/core/http instead of raw fetch in the API layer.'
          },
          {
            name: 'WebSocket',
            message: 'Use $lib/api/core/ws instead of constructing sockets directly in the API layer.'
          }
        ]
      }
    },
    {
      files: ['src/lib/**/*.ts', 'src/lib/**/*.svelte', 'src/routes/**/*.ts', 'src/routes/**/*.svelte'],
      ignores: [
        'src/lib/ts-bindings/**',
        'src/lib/api/core/http.ts',
        'src/lib/api/core/ws.ts',
        'src/lib/api/requestUtils.ts',
        'src/lib/components/EncodedStreamPlayer.svelte',
        'src/lib/3d/rig.ts'
      ],
      rules: {
        'no-restricted-globals': [
          'error',
          {
            name: 'fetch',
            message: 'Use the shared API layer instead of raw fetch in app code. Raw fetch is only allowed in low-level transport/media primitives.'
          },
          {
            name: 'WebSocket',
            message: 'Use $lib/api/core/ws instead of constructing sockets directly in app code.'
          }
        ]
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
