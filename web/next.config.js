/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  webpack: (config) => {
    // Solana / Arcium libs reference Node built-ins; provide browser fallbacks.
    config.resolve.fallback = {
      ...config.resolve.fallback,
      fs: false,
      net: false,
      tls: false,
      crypto: require.resolve("crypto-browserify"),
      stream: require.resolve("stream-browserify"),
      buffer: require.resolve("buffer/"),
    };
    config.experiments = { ...config.experiments, asyncWebAssembly: true, topLevelAwait: true };

    // Optional Node-only deps pulled in transitively by the WalletConnect /
    // @reown / pino chain (via @solana/wallet-adapter-wallets). They are never
    // exercised in the browser bundle, so mark them external to stop webpack
    // from trying to resolve them ("Can't resolve 'pino-pretty'").
    config.externals.push("pino-pretty", "lokijs", "encoding");

    // viem/ox use dynamic `import()` expressions webpack cannot statically
    // analyze, emitting "Critical dependency: the request of a dependency is an
    // expression". Benign — silence it rather than letting it clutter output.
    config.ignoreWarnings = [
      ...(config.ignoreWarnings || []),
      { module: /node_modules\/(ox|viem|@reown|@walletconnect)/ },
      { message: /Critical dependency: the request of a dependency is an expression/ },
    ];

    return config;
  },
};
module.exports = nextConfig;
