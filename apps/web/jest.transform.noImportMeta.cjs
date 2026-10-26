/**
 * jest transform for the one client module that reads Vite's `import.meta.env`
 * (src/api/client.ts). jest runs TypeScript as CommonJS, where `import.meta`
 * is a syntax error, so no unit test can even load the real client through
 * the default ts-jest transform. This transform pre-replaces the Vite-ism with
 * an empty env object and then transpiles exactly like ts-jest's isolatedModules
 * mode does (see jest.config.cjs): under jest the client therefore takes the
 * same `window.location.origin` / localhost fallback the browser build takes
 * when Vite injects no server URL. Everything else in the module -- getJson,
 * apiUrl, every endpoint -- runs unmocked, so tests observe the request URLs
 * the real client issues. Extend the transform pattern in jest.config.cjs if
 * another import.meta-bearing module (e.g. ws/WebSocketContext.tsx) ever
 * needs unit tests.
 */
const ts = require('typescript');

const COMPILER_OPTIONS = {
  module: ts.ModuleKind.CommonJS,
  target: ts.ScriptTarget.ES2022,
  esModuleInterop: true,
  isolatedModules: true
};

module.exports = {
  process(sourceText) {
    const transpiled = ts.transpileModule(sourceText.replace(/\bimport\.meta\b/g, '({ env: {} })'), {
      compilerOptions: COMPILER_OPTIONS
    });
    return {
      code: transpiled.outputText,
      ...(transpiled.sourceMapText ? { map: JSON.parse(transpiled.sourceMapText) } : {})
    };
  }
};
