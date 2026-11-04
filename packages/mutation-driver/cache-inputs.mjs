// Acquire resolved compilation values, including environment-specific overrides.
// User expressions/provided values stay raw; only known filesystem locations can
// receive the caller's explicitly enabled relocation identity.
const builtinCompilationPlugins = new Set(`
errspec-mutation alias vite:optimized-deps vite:pre-alias vitest:resolve-builtin
vitest:css-disable vitest:resolve-core vitest:meta-env-replacer
vite:modulepreload-polyfill vite:bundled-dev-client-imports vite:resolve-dev
vite:html-inline-proxy vite:css builtin:oxc-runtime vite:oxc builtin:vite-json
vite:wasm-helper vite:worker vite:asset vite:define vite:css-post vite:build-html
vite:worker-import-meta-url vite:asset-import-meta-url vite:dynamic-import-vars
vite:import-glob vitest:css-empty-post vitest:mocks vitest:automock
vitest:coverage-transform vitest:normalize-url vite:client-inject
vite:css-analysis vite:import-analysis builtin:vite-resolve
`.trim().split(/\s+/));
function knownCompilation(plugins = []) {
  const seen=new Set();
  const order=[];
  for(const plugin of plugins) {
    if(!plugin.transform && !plugin.load && !plugin.resolveId)continue;
    if(!builtinCompilationPlugins.has(plugin.name) || seen.has(plugin.name))
      throw new Error('Unacquired compilation plugin');
    seen.add(plugin.name);
    order.push(plugin.name);
  }
  return order;
}
export function compilerInputs(server, test, relocatedPath) {
  if(Object.keys(test._scriptDefines || {}).length)throw new Error('Runtime define scripts cannot be fingerprinted');
  const css = test.css;
  const processCss = css === true || css && typeof css === 'object' && css.include?.length > 0;
  const assets = config => {
    if(typeof config.assetsInclude !== 'function')return config.assetsInclude;
    const implementation=Function.prototype.toString.call(config.assetsInclude).replace(/\s+/g,'');
    if(Array.isArray(config.rawAssetsInclude) && /^assetsInclude\(file\)\{returnDEFAULT_ASSETS_RE(?:\$\d+)?\.test\(file\)\|\|assetsFilter\(file\);?\}$/.test(implementation))
      return {builtin:'vite-assets',patterns:config.rawAssetsInclude};
    throw new Error('Asset matching cannot be fingerprinted');
  };
  const resolve = config => ({
    ...config,
    ...(Array.isArray(config?.alias) ? {alias: config.alias.map(alias => ({
      ...alias,
      ...(typeof alias.replacement === 'string' ? {replacement: resolutionPath(alias.replacement)} : {}),
    }))} : {}),
  });
  function resolutionPath(path) {
    if(path.startsWith('/@fs/')) {
      const location=relocatedPath(path.slice('/@fs'.length));
      if(location && typeof location === 'object')return {viteFilesystemPath:location};
      return path;
    }
    return relocatedPath(path);
  }
  const compilation = config => {
    const compilationPlugins=knownCompilation(config.plugins);
    return {
      compilationPlugins,
      define: config.define, mode: config.mode, isProduction: config.isProduction,
      base: config.base, appType: config.appType, assetsInclude: assets(config),
      env: config.env, envPrefix: config.envPrefix,
      publicDir: typeof config.publicDir === 'string' ? relocatedPath(config.publicDir) : config.publicDir,
      envDir: typeof config.envDir === 'string' ? relocatedPath(config.envDir) : config.envDir,
      resolve: resolve(config.resolve), esbuild: config.esbuild, oxc: config.oxc,
      json: config.json, ssr: config.ssr, experimental: config.experimental,
      optimizeDeps: config.optimizeDeps,
      // Vitest disables CSS by default and installs a callback for its CSS module
      // proxy. The serialized test CSS policy fully describes that disabled path.
      // Processing CSS with unserializable callbacks stays in the fresh path.
      testCss: css, ...(processCss ? {css: config.css} : {}),
    };
  };
  return {
    runtimeDefines: test.defines,
    root: compilation(server.config),
    environments: Object.fromEntries(Object.entries(server.environments || {})
      .sort(([a],[b]) => a < b ? -1 : a > b ? 1 : 0)
      .map(([name, environment]) => [name, compilation(environment.config)])),
  };
}
